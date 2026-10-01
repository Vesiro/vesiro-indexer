use aws_config::BehaviorVersion;
use aws_sdk_s3::config::Region;
use tracing::instrument;
use vesiro_indexer_protocol::collection::Collection;

/// The S3 client the functions here take.
pub use aws_sdk_s3::Client as S3Client;

/// The bucket the Common Crawl archives live in.
const BUCKET: &str = "commoncrawl";

/// Where Common Crawl serves the same archives over HTTPS. A key in the bucket is a path here.
const HTTPS_BASE_URL: &str = "https://data.commoncrawl.org/";

/// The two ways of reaching the Common Crawl archives. Both address an object by the same key.
#[derive(Debug, Clone)]
pub enum Client {
    /// The `commoncrawl` S3 bucket, which needs AWS credentials.
    S3(S3Client),
    /// `data.commoncrawl.org`, which needs nothing.
    Https(reqwest::Client),
}

/// A client pointed at the region the Common Crawl bucket lives in.
#[instrument]
pub async fn s3_client() -> anyhow::Result<Client> {
    let config = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .load()
        .await;
    let config = aws_sdk_s3::config::Builder::from(&config)
        .force_path_style(true)
        .build();
    Ok(Client::S3(aws_sdk_s3::Client::from_conf(config)))
}

#[derive(Debug, Clone)]
pub struct CcS3Collection(&'static str);

impl CcS3Collection {
    #[instrument(skip(client))]
    pub async fn wet_paths(&self, client: &Client) -> anyhow::Result<Vec<String>> {
        let key = format!("{}/wet.paths.gz", self.0);
        Ok(Self::fetch(client, &key).await?)
    }

    #[instrument(skip(client))]
    pub async fn warc_paths(&self, client: &Client) -> anyhow::Result<Vec<String>> {
        let key = format!("{}/warc.paths.gz", self.0);
        Ok(Self::fetch(client, &key).await?)
    }

    #[instrument(skip(client))]
    async fn fetch(client: &Client, key: &str) -> anyhow::Result<Vec<String>> {
        let body = download(client, key).await?;
        let mut decoder = flate2::read::GzDecoder::new(&body[..]);
        let mut s = String::new();
        std::io::Read::read_to_string(&mut decoder, &mut s)?;
        Ok(s.lines().map(|line| line.to_string()).collect())
    }
}

/// Downloads the Common Crawl object stored under `key`.
#[instrument(skip(client))]
pub async fn download(client: &Client, key: &str) -> anyhow::Result<Vec<u8>> {
    tracing::debug!(key = key, "downloading");
    let body = match client {
        Client::S3(aws_client) => {
            let object = aws_client
                .get_object()
                .bucket(BUCKET)
                .key(key)
                .send()
                .await?;
            object.body.collect().await?.into_bytes()
        }
        Client::Https(client) => {
            let url = reqwest::Url::parse(HTTPS_BASE_URL)?.join(key)?;
            client
                .get(url)
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?
        }
    };
    tracing::debug!(key = key, len = body.len(), "downloaded");
    Ok(body.to_vec())
}

pub fn cc_collections_2024() -> Vec<CcS3Collection> {
    vec![
        CcS3Collection("crawl-data/CC-MAIN-2024-10"),
        CcS3Collection("crawl-data/CC-MAIN-2024-18"),
        CcS3Collection("crawl-data/CC-MAIN-2024-22"),
        CcS3Collection("crawl-data/CC-MAIN-2024-26"),
        CcS3Collection("crawl-data/CC-MAIN-2024-30"),
        CcS3Collection("crawl-data/CC-MAIN-2024-33"),
        CcS3Collection("crawl-data/CC-MAIN-2024-38"),
        CcS3Collection("crawl-data/CC-MAIN-2024-42"),
        CcS3Collection("crawl-data/CC-MAIN-2024-46"),
        CcS3Collection("crawl-data/CC-MAIN-2024-51"),
    ]
}

/// Whether `collection` is visited in shuffled order.
fn should_shuffle(collection: &Collection) -> bool {
    matches!(
        collection,
        Collection::CcWarc2024Shuffled | Collection::CcWet2024Shuffled
    )
}

/// The keys of every object in `collection`, in the order they should be processed.
#[instrument(skip(client))]
pub async fn s3_keys(client: &Client, collection: &Collection) -> anyhow::Result<Vec<String>> {
    use Collection::*;

    let mut s3_keys = Vec::new();
    for cc_collection in cc_collections_2024() {
        let paths = match collection {
            CcWarc2024 | CcWarc2024Shuffled => cc_collection.warc_paths(client).await?,
            CcWet2024 | CcWet2024Shuffled => cc_collection.wet_paths(client).await?,
        };
        s3_keys.extend(paths);
    }
    if should_shuffle(collection) {
        let seed = 0x420000fefeca;
        shuffle(&mut s3_keys, seed);
    }

    Ok(s3_keys)
}

fn xnasam(ctr: u64, x: u64) -> u64 {
    let mut v = ctr;

    v ^= x;
    v ^= v.rotate_right(25) ^ v.rotate_right(47);
    v = v.wrapping_mul(0x9E6C63D0676A9A99);
    v ^= v >> 23 ^ v >> 51;
    v = v.wrapping_mul(0x9E6D62D06F6A9A9B);
    v ^= v >> 23 ^ v >> 51;

    v
}

fn shuffle<T>(xs: &mut [T], seed: u64) {
    let n = xs.len() as u64;
    for i in 0..n {
        let j = i + (xnasam(seed, i) % (n - i));
        xs.swap(i as usize, j as usize);
    }
}
