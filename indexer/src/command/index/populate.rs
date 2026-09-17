use clap::{ArgGroup, Parser};
use reqwest::Url;

use crate::{
    IndexApp, IndexNameFromUuidError,
    index_documents::spawn_collection_object_processors,
    object_source::{ObjectFetcher, ObjectSource},
};

#[derive(Debug, Parser)]
#[clap(group(ArgGroup::new("target").required(true)))]
pub struct PopulateOptions {
    /// The uuid of the index to append to.
    #[clap(long, group = "target")]
    uuid: Option<String>,
    /// The name of the index to append to.
    #[clap(long, group = "target", env = "VESIRO_INDEXER_INDEX_NAME")]
    index_name: Option<String>,
    /// Amount of documents that should be added to the index.
    #[clap(long)]
    doc_delta: Option<usize>,
    /// Concurrent workers.
    #[clap(long, default_value_t = 1, env = "VESIRO_INDEXER_WORKER_AMOUNT")]
    worker_amount: usize,
    /// Sends INDEX/_refresh to the cluster when done.
    #[clap(long)]
    refresh_when_done: bool,
    /// Where the collection objects are read from.
    #[clap(long, default_value = "s3", env = "VESIRO_INDEXER_OBJECT_SOURCE")]
    object_source: ObjectSource,
    /// URL of the vesiro-indexer server.
    #[clap(
        long,
        env = "VESIRO_INDEXER_SERVER_URL",
        required_if_eq("object_source", "server")
    )]
    server_url: Option<Url>,
}

impl PopulateOptions {
    pub async fn index_name(&self, app: &IndexApp) -> anyhow::Result<String> {
        if let Some(index_name) = &self.index_name {
            Ok(index_name.clone())
        } else if let Some(uuid) = &self.uuid {
            Ok(app.index_name_from_uuid(uuid).await?)
        } else {
            Err(IndexNameFromUuidError::TargetOptionMustBeSet)?
        }
    }

    pub async fn uuid(&self, app: &IndexApp) -> anyhow::Result<String> {
        if let Some(uuid) = &self.uuid {
            Ok(uuid.clone())
        } else if let Some(index_name) = &self.index_name {
            Ok(app.uuid_from_index_name(index_name).await?)
        } else {
            Err(IndexNameFromUuidError::TargetOptionMustBeSet)?
        }
    }
}

impl IndexApp {
    pub async fn populate(self, options: PopulateOptions) -> anyhow::Result<()> {
        let node_url = self.node_url.clone();

        // Resolve the target before reaching the source, so a mistake in the options is reported
        // as itself rather than as whatever the network does first.
        let index_name = options.index_name(&self).await?;
        let uuid = options.uuid(&self).await?;

        let fetcher = match options.object_source {
            // `required_if_eq` on the option guarantees the url is here.
            ObjectSource::Server => {
                let server_url = options
                    .server_url
                    .clone()
                    .expect("--server-url is required when --object-source is server");
                ObjectFetcher::server(self.client.clone(), server_url).await?
            }
            ObjectSource::S3 => ObjectFetcher::s3().await?,
        };

        let (index_info, index_db) = self.db.open_index(&uuid)?;
        tracing::info!(?index_info, "target index");
        let collection = index_info.collection;
        let objects = fetcher.list(&collection).await?;
        tracing::info!(
            object_source = ?options.object_source,
            objects = objects.len(),
            "listed collection objects"
        );

        let doc_delta = if let Some(doc_delta) = options.doc_delta {
            doc_delta as i64
        } else {
            i64::MAX
        };

        spawn_collection_object_processors(
            index_db.clone(),
            self.client.clone(),
            fetcher,
            node_url.clone(),
            index_name.clone(),
            index_info.mapping.mapping(),
            doc_delta,
            objects,
            options.worker_amount,
        )
        .await?;

        index_db.flush()?;

        if options.refresh_when_done {
            tracing::info!(%index_name, "refreshing index");
            let url = node_url;
            let url = url.join(&index_name)?;
            let url = url.join("/_refresh")?;
            self.client.post(url).send().await?;
        }

        Ok(())
    }
}
