use reqwest::{Response, Url};
use serde::{Deserialize, Serialize};

use crate::{
    collection::Collection,
    db::{DbCollectionObjectKey, DbObjectEntryStatus},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetCollectionObjectsResponseItem {
    pub db_key: DbCollectionObjectKey,
    pub s3_key: String,
    pub status: DbObjectEntryStatus,
}

pub type GetCollectionObjectsResponse = Vec<GetCollectionObjectsResponseItem>;

pub async fn get_collection_objects(
    client: &reqwest::Client,
    url: &Url,
    collection: &Collection,
) -> anyhow::Result<Response> {
    Ok(client
        .get(
            url.clone()
                .join(&format!("collection/{}/objects", collection.to_string()))?,
        )
        .send()
        .await?)
}

pub async fn get_collection_object(
    client: &reqwest::Client,
    url: &Url,
    db_key: &DbCollectionObjectKey,
) -> anyhow::Result<Response> {
    Ok(client
        .get(url.join(&format!("download/{}", db_key.to_string()))?)
        .send()
        .await?)
}
