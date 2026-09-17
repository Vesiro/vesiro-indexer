use serde::Deserialize;
use serde_json::Value;

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct BulkCreateItem {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_index")]
    pub index: String,
    pub error: Option<Value>,
    pub status: u32,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct BulkItem {
    pub create: BulkCreateItem,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct BulkResponse {
    pub errors: bool,
    pub items: Vec<BulkItem>,
}
