use chrono::{DateTime, Utc};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use vesiro_indexer_protocol::collection::Collection;

use crate::cc::mapping::{Mapping, parse::parse_urn_uuid, raw};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WarcInfo {
    content: String,
    content_length: u64,
    content_type: String,
    version: String,
    date: DateTime<Utc>,
    filename: String,
    record_id: Uuid,
    warc_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Document {
    content: String,
    content_length: u64,
    content_type: String,
    version: String,
    block_digest: String,
    date: DateTime<Utc>,
    identified_content_language: Vec<String>,
    record_id: Uuid,
    refers_to: Uuid,
    target_uri: Url,
    warc_type: String,
    warc_info: WarcInfo,
}

pub struct CcWetMapping;

impl Mapping for CcWetMapping {
    fn allows_collection(&self, collection: &Collection) -> bool {
        use Collection::*;
        [CcWet2024, CcWet2024Shuffled].contains(collection)
    }

    fn transform_document(&self, raw: &Value) -> anyhow::Result<(String, Value)> {
        let raw = serde_json::from_value::<raw::RawWetDocument>(raw.clone())?;
        let identified_content_language = raw
            .entry
            .warc_identified_content_language
            .map(|lang| lang.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or(vec![]);
        let uri = match Url::parse(&raw.entry.warc_target_uri) {
            Ok(uri) => uri,
            Err(err) => {
                return Err(anyhow::anyhow!(
                    "Failed to parse URL '{}': {}",
                    &raw.entry.warc_target_uri,
                    err
                ));
            }
        };
        let document = Document {
            content: raw.entry.content,
            content_length: raw.entry.content_length.parse()?,
            content_type: raw.entry.content_type,
            version: raw.entry.version,
            block_digest: raw.entry.warc_block_digest,
            date: DateTime::parse_from_rfc3339(&raw.entry.warc_date)?.with_timezone(&Utc),
            identified_content_language: identified_content_language,
            record_id: parse_urn_uuid(&raw.entry.warc_record_id)?,
            refers_to: parse_urn_uuid(&raw.entry.warc_refers_to)?,
            target_uri: uri,
            warc_type: raw.entry.warc_type,
            warc_info: WarcInfo {
                content: raw.warc_info.content,
                content_length: raw.warc_info.content_length.parse()?,
                content_type: raw.warc_info.content_type,
                version: raw.warc_info.version,
                date: DateTime::parse_from_rfc3339(&raw.warc_info.warc_date)?.with_timezone(&Utc),
                filename: raw.warc_info.warc_filename,
                record_id: parse_urn_uuid(&raw.warc_info.warc_record_id)?,
                warc_type: raw.warc_info.warc_type,
            },
        };
        let id = document.record_id.clone();
        let value = serde_json::to_value(&document)?;
        Ok((id.to_string(), value))
    }

    fn mappings(&self) -> Value {
        json!(
            {
                "properties": {
                    "content": {
                        "type": "text"
                    },
                    "content_length": {
                        "type": "long"
                    },
                    "content_type": {
                        "type": "keyword"
                    },
                    "version": {
                        "type": "keyword"
                    },
                    "block_digest": {
                        "type": "keyword"
                    },
                    "date": {
                        "type": "date"
                    },
                    "identified_content_language": {
                        "type": "keyword"
                    },
                    "record_id": {
                        "type": "keyword"
                    },
                    "refers_to": {
                        "type": "keyword"
                    },
                    "target_uri": {
                        "type": "text",
                        "fields": {
                            "keyword": {
                                "type": "keyword",
                                "ignore_above": 256
                            }
                        }
                    },
                    "warc_type": {
                        "type": "keyword"
                    },
                    "warc_info": {
                        "properties": {
                            "content": {
                                "type": "text"
                            },
                            "content_length": {
                                "type": "long"
                            },
                            "content_type": {
                                "type": "keyword"
                            },
                            "version": {
                                "type": "keyword"
                            },
                            "date": {
                                "type": "date"
                            },
                            "filename": {
                                "type": "text",
                                "fields": {
                                    "keyword": {
                                        "type": "keyword",
                                        "ignore_above": 256
                                    }
                                }
                            },
                            "record_id": {
                                "type": "keyword"
                            },
                            "warc_type": {
                                "type": "keyword"
                            }
                        }
                    }
                }
            }
        )
    }
}
