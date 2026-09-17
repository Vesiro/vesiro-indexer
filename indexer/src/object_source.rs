//! Where the collection objects come from.
//!
//! An object is either pulled off our own server, which keeps a copy of everything it has
//! downloaded, or read straight out of Common Crawl's S3 bucket. Both sources hand back the same
//! thing - the object's bytes, still gzip compressed - and address objects by the same
//! [`DbCollectionObjectKey`], so the rest of the indexer does not care which one it got.

use clap::ValueEnum;
use reqwest::Url;
use tracing::instrument;
use vesiro_indexer_protocol::{
    collection::Collection,
    db::{DbCollectionObjectKey, DbObjectEntryStatus},
    http,
};

/// Where `populate` reads collection objects from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ObjectSource {
    /// The vesiro-indexer server, which serves the objects it has finished downloading.
    Server,
    /// Common Crawl's S3 bucket, read directly.
    S3,
}

/// One collection object, however it was listed.
#[derive(Debug, Clone)]
pub struct CollectionObject {
    pub db_key: DbCollectionObjectKey,
    pub s3_key: String,
}

/// An [`ObjectSource`] with the client it needs to actually talk to.
#[derive(Debug, Clone)]
pub enum ObjectFetcher {
    Server {
        client: reqwest::Client,
        server_url: Url,
    },
    S3 {
        client: cc::S3Client,
    },
}

impl ObjectFetcher {
    /// Connects to the vesiro-indexer server.
    #[instrument(skip(client))]
    pub async fn server(client: reqwest::Client, server_url: Url) -> anyhow::Result<Self> {
        let response = client.get(server_url.clone()).send().await?;
        tracing::info!(?response, "server response");
        Ok(ObjectFetcher::Server { client, server_url })
    }

    /// Connects to Common Crawl's S3 bucket.
    #[instrument]
    pub async fn s3() -> anyhow::Result<Self> {
        Ok(ObjectFetcher::S3 {
            client: cc::s3_client().await?,
        })
    }

    /// The objects of `collection` that can be indexed, in the order they should be processed.
    #[instrument(skip(self))]
    pub async fn list(&self, collection: &Collection) -> anyhow::Result<Vec<CollectionObject>> {
        match self {
            ObjectFetcher::Server { client, server_url } => {
                let response = http::get_collection_objects(client, server_url, collection).await?;
                let objects: http::GetCollectionObjectsResponse =
                    response.error_for_status()?.json().await?;

                // The server knows about objects it has not finished downloading yet, and cannot
                // serve those.
                Ok(objects
                    .into_iter()
                    .filter(|object| matches!(object.status, DbObjectEntryStatus::Completed { .. }))
                    .map(|object| CollectionObject {
                        db_key: object.db_key,
                        s3_key: object.s3_key,
                    })
                    .collect())
            }
            ObjectFetcher::S3 { client } => {
                // The server numbers an object by its position in this very list, and `s3_keys`
                // is deterministic - the shuffle runs off a fixed seed - so the `db_key` derived
                // here is the one the server would have handed out for the same object. That is
                // what lets progress recorded under one source be picked up under the other.
                Ok(cc::s3_keys(client, collection)
                    .await?
                    .into_iter()
                    .enumerate()
                    .map(|(index, s3_key)| CollectionObject {
                        db_key: DbCollectionObjectKey::new(collection, index as u32),
                        s3_key,
                    })
                    .collect())
            }
        }
    }

    /// The bytes of one object, still gzip compressed.
    #[instrument(skip(self))]
    pub async fn download(&self, object: &CollectionObject) -> anyhow::Result<Vec<u8>> {
        match self {
            ObjectFetcher::Server { client, server_url } => {
                let response =
                    http::get_collection_object(client, server_url, &object.db_key).await?;
                tracing::debug!(?response, "response content");
                Ok(response.error_for_status()?.bytes().await?.to_vec())
            }
            ObjectFetcher::S3 { client } => cc::download(client, &object.s3_key).await,
        }
    }
}
