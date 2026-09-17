use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::instrument;
use vesiro_indexer_protocol::collection::Collection;

use crate::cc::mapping::Mapping;

pub struct RawMapping;

impl Mapping for RawMapping {
    fn allows_collection(&self, _: &Collection) -> bool {
        true
    }

    #[instrument(skip(self, document))]
    fn transform_document(&self, document: &Value) -> anyhow::Result<(String, Value)> {
        tracing::trace!(document = ?document);
        let id = document["entry"]["WARC-Record-ID"]
            .as_str()
            .context("Field entry.WARC-Record-ID either missing or null")?;
        Ok((id.to_string(), document.clone()))
    }

    fn mappings(&self) -> Value {
        serde_json::json!({})
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawWetDocumentEntry {
    #[serde(rename = "Content")]
    pub content: String,
    #[serde(rename = "Content-Length")]
    pub content_length: String,
    #[serde(rename = "Content-Type")]
    pub content_type: String,
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "WARC-Block-Digest")]
    pub warc_block_digest: String,
    #[serde(rename = "WARC-Date")]
    pub warc_date: String,
    #[serde(rename = "WARC-Identified-Content-Language")]
    pub warc_identified_content_language: Option<String>,
    #[serde(rename = "WARC-Record-ID")]
    pub warc_record_id: String,
    #[serde(rename = "WARC-Refers-To")]
    pub warc_refers_to: String,
    #[serde(rename = "WARC-Target-URI")]
    pub warc_target_uri: String,
    #[serde(rename = "WARC-Type")]
    pub warc_type: String,
}

/// The `warcinfo` record heading a WARC or WET object. Both carry the same headers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawWarcInfo {
    #[serde(rename = "Content")]
    pub content: String,
    #[serde(rename = "Content-Length")]
    pub content_length: String,
    #[serde(rename = "Content-Type")]
    pub content_type: String,
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "WARC-Date")]
    pub warc_date: String,
    #[serde(rename = "WARC-Filename")]
    pub warc_filename: String,
    #[serde(rename = "WARC-Record-ID")]
    pub warc_record_id: String,
    #[serde(rename = "WARC-Type")]
    pub warc_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawWetDocument {
    pub entry: RawWetDocumentEntry,
    pub warc_info: RawWarcInfo,
}

/// A record from a WARC object.
///
/// Only the headers every record type carries are required. A WARC object holds `request`,
/// `response` and `metadata` records, and the rest of these headers appear on some of them and
/// not others - a missing required header makes the whole document fail to transform, so
/// anything not guaranteed is optional here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawWarcDocumentEntry {
    #[serde(rename = "Content")]
    pub content: String,
    #[serde(rename = "Content-Length")]
    pub content_length: String,
    #[serde(rename = "Content-Type")]
    pub content_type: String,
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "WARC-Date")]
    pub warc_date: String,
    #[serde(rename = "WARC-Record-ID")]
    pub warc_record_id: String,
    #[serde(rename = "WARC-Type")]
    pub warc_type: String,
    #[serde(rename = "WARC-Block-Digest")]
    pub warc_block_digest: Option<String>,
    #[serde(rename = "WARC-Cipher-Suite")]
    pub warc_cipher_suite: Option<String>,
    #[serde(rename = "WARC-Concurrent-To")]
    pub warc_concurrent_to: Option<String>,
    #[serde(rename = "WARC-IP-Address")]
    pub warc_ip_address: Option<String>,
    #[serde(rename = "WARC-Identified-Payload-Type")]
    pub warc_identified_payload_type: Option<String>,
    #[serde(rename = "WARC-Payload-Digest")]
    pub warc_payload_digest: Option<String>,
    #[serde(rename = "WARC-Protocol")]
    pub warc_protocol: Option<String>,
    #[serde(rename = "WARC-Target-URI")]
    pub warc_target_uri: Option<String>,
    #[serde(rename = "WARC-Truncated")]
    pub warc_truncated: Option<String>,
    #[serde(rename = "WARC-Warcinfo-ID")]
    pub warc_warcinfo_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawWarcDocument {
    pub entry: RawWarcDocumentEntry,
    pub warc_info: RawWarcInfo,
}
