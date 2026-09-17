use std::net::IpAddr;

use anyhow::Context;
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

/// A WARC record as it is indexed.
///
/// Everything the record type does not guarantee is optional, so a `request` or `metadata`
/// record is indexed rather than dropped for lacking the headers a `response` carries.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Document {
    content: String,
    content_length: u64,
    content_type: String,
    version: String,
    date: DateTime<Utc>,
    record_id: Uuid,
    warc_type: String,
    block_digest: Option<String>,
    cipher_suite: Option<String>,
    concurrent_to: Option<Uuid>,
    identified_payload_type: Option<String>,
    ip_address: Option<IpAddr>,
    payload_digest: Option<String>,
    protocol: Option<String>,
    target_uri: Option<Url>,
    truncated: Option<String>,
    warcinfo_id: Option<Uuid>,
    warc_info: WarcInfo,
}

pub struct CcWarcMapping;

impl Mapping for CcWarcMapping {
    fn allows_collection(&self, collection: &Collection) -> bool {
        use Collection::*;
        [CcWarc2024, CcWarc2024Shuffled].contains(collection)
    }

    fn transform_document(&self, raw: &Value) -> anyhow::Result<(String, Value)> {
        let raw = serde_json::from_value::<raw::RawWarcDocument>(raw.clone())?;
        let target_uri = raw
            .entry
            .warc_target_uri
            .as_deref()
            .map(Url::parse)
            .transpose()
            .with_context(|| {
                format!(
                    "failed to parse URL '{}'",
                    raw.entry.warc_target_uri.as_deref().unwrap_or_default()
                )
            })?;
        let ip_address = raw
            .entry
            .warc_ip_address
            .as_deref()
            .map(str::parse::<IpAddr>)
            .transpose()
            .with_context(|| {
                format!(
                    "failed to parse IP address '{}'",
                    raw.entry.warc_ip_address.as_deref().unwrap_or_default()
                )
            })?;
        let document = Document {
            content: raw.entry.content,
            content_length: raw.entry.content_length.parse()?,
            content_type: raw.entry.content_type,
            version: raw.entry.version,
            date: DateTime::parse_from_rfc3339(&raw.entry.warc_date)?.with_timezone(&Utc),
            record_id: parse_urn_uuid(&raw.entry.warc_record_id)?,
            warc_type: raw.entry.warc_type,
            block_digest: raw.entry.warc_block_digest,
            cipher_suite: raw.entry.warc_cipher_suite,
            concurrent_to: raw
                .entry
                .warc_concurrent_to
                .as_deref()
                .map(parse_urn_uuid)
                .transpose()?,
            identified_payload_type: raw.entry.warc_identified_payload_type,
            ip_address,
            payload_digest: raw.entry.warc_payload_digest,
            protocol: raw.entry.warc_protocol,
            target_uri,
            truncated: raw.entry.warc_truncated,
            warcinfo_id: raw
                .entry
                .warc_warcinfo_id
                .as_deref()
                .map(parse_urn_uuid)
                .transpose()?,
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
        let id = document.record_id;
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
                    "date": {
                        "type": "date"
                    },
                    "record_id": {
                        "type": "keyword"
                    },
                    "warc_type": {
                        "type": "keyword"
                    },
                    "block_digest": {
                        "type": "keyword"
                    },
                    "cipher_suite": {
                        "type": "keyword"
                    },
                    "concurrent_to": {
                        "type": "keyword"
                    },
                    "identified_payload_type": {
                        "type": "keyword"
                    },
                    "ip_address": {
                        "type": "ip"
                    },
                    "payload_digest": {
                        "type": "keyword"
                    },
                    "protocol": {
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
                    "truncated": {
                        "type": "keyword"
                    },
                    "warcinfo_id": {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn warc_info() -> Value {
        json!({
            "Version": "WARC/1.0",
            "Content": "isPartOf: CC-MAIN-2024-10\r\n",
            "Content-Length": "27",
            "Content-Type": "application/warc-fields",
            "WARC-Date": "2024-02-20T00:00:00Z",
            "WARC-Filename": "CC-MAIN-20240220000000-20240220000001-00000.warc.gz",
            "WARC-Record-ID": "<urn:uuid:00000000-0000-0000-0000-00000000000a>",
            "WARC-Type": "warcinfo"
        })
    }

    fn transform(entry: Value) -> anyhow::Result<(String, Value)> {
        CcWarcMapping.transform_document(&json!({
            "warc_info": warc_info(),
            "entry": entry,
        }))
    }

    /// A `response` record over HTTPS, carrying every header the mapping knows about.
    #[test]
    fn transforms_a_full_response_record() {
        let (id, document) = transform(json!({
            "Version": "WARC/1.0",
            "Content": "HTTP/1.1 200 OK\r\n\r\n<html></html>",
            "Content-Length": "32",
            "Content-Type": "application/http; msgtype=response",
            "WARC-Block-Digest": "sha1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "WARC-Cipher-Suite": "TLS_AES_128_GCM_SHA256",
            "WARC-Concurrent-To": "<urn:uuid:00000000-0000-0000-0000-00000000000b>",
            "WARC-Date": "2024-02-20T12:34:56Z",
            "WARC-IP-Address": "93.184.216.34",
            "WARC-Identified-Payload-Type": "text/html",
            "WARC-Payload-Digest": "sha1:BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
            "WARC-Protocol": "h2",
            "WARC-Record-ID": "<urn:uuid:00000000-0000-0000-0000-00000000000c>",
            "WARC-Target-URI": "https://example.com/",
            "WARC-Truncated": "length",
            "WARC-Type": "response",
            "WARC-Warcinfo-ID": "<urn:uuid:00000000-0000-0000-0000-00000000000a>"
        }))
        .unwrap();

        assert_eq!(id, "00000000-0000-0000-0000-00000000000c");
        assert_eq!(document["content_length"], 32);
        assert_eq!(document["date"], "2024-02-20T12:34:56Z");
        assert_eq!(document["ip_address"], "93.184.216.34");
        assert_eq!(document["target_uri"], "https://example.com/");
        assert_eq!(
            document["concurrent_to"],
            "00000000-0000-0000-0000-00000000000b"
        );
        assert_eq!(
            document["warcinfo_id"],
            "00000000-0000-0000-0000-00000000000a"
        );
        assert_eq!(document["truncated"], "length");
        assert_eq!(
            document["warc_info"]["filename"].as_str().unwrap().len(),
            51
        );
    }

    /// `request` and `metadata` records carry far fewer headers, and must still index.
    #[test]
    fn transforms_records_that_omit_the_response_only_headers() {
        let (_, request) = transform(json!({
            "Version": "WARC/1.0",
            "Content": "GET / HTTP/1.1\r\n\r\n",
            "Content-Length": "18",
            "Content-Type": "application/http; msgtype=request",
            "WARC-Date": "2024-02-20T12:34:56Z",
            "WARC-Record-ID": "<urn:uuid:00000000-0000-0000-0000-00000000000d>",
            "WARC-Target-URI": "https://example.com/",
            "WARC-Type": "request"
        }))
        .unwrap();
        assert_eq!(request["warc_type"], "request");
        for absent in [
            "ip_address",
            "payload_digest",
            "cipher_suite",
            "truncated",
            "protocol",
            "identified_payload_type",
            "block_digest",
            "concurrent_to",
            "warcinfo_id",
        ] {
            assert!(request[absent].is_null(), "{absent} should be null");
        }

        // The bare minimum: no target URI either.
        let (_, metadata) = transform(json!({
            "Version": "WARC/1.0",
            "Content": "fetchTimeMs: 220\r\n",
            "Content-Length": "18",
            "Content-Type": "application/warc-fields",
            "WARC-Date": "2024-02-20T12:34:56Z",
            "WARC-Record-ID": "<urn:uuid:00000000-0000-0000-0000-00000000000e>",
            "WARC-Type": "metadata"
        }))
        .unwrap();
        assert!(metadata["target_uri"].is_null());
    }

    #[test]
    fn refuses_a_record_that_is_missing_a_guaranteed_header_or_has_a_malformed_value() {
        // No WARC-Record-ID: there would be no document id to index under.
        assert!(
            transform(json!({
                "Version": "WARC/1.0",
                "Content": "",
                "Content-Length": "0",
                "Content-Type": "application/warc-fields",
                "WARC-Date": "2024-02-20T12:34:56Z",
                "WARC-Type": "metadata"
            }))
            .is_err()
        );

        let malformed = |field: &str, value: &str| {
            let mut entry = json!({
                "Version": "WARC/1.0",
                "Content": "",
                "Content-Length": "0",
                "Content-Type": "application/warc-fields",
                "WARC-Date": "2024-02-20T12:34:56Z",
                "WARC-Record-ID": "<urn:uuid:00000000-0000-0000-0000-00000000000f>",
                "WARC-Type": "response"
            });
            entry[field] = json!(value);
            transform(entry)
        };
        assert!(malformed("WARC-IP-Address", "not-an-ip").is_err());
        assert!(malformed("WARC-Target-URI", "not a url").is_err());
        assert!(malformed("WARC-Concurrent-To", "not-a-urn").is_err());
        assert!(malformed("WARC-IP-Address", "2606:2800:220:1:248:1893:25c8:1946").is_ok());
    }

    #[test]
    fn allows_only_the_warc_collections() {
        use Collection::*;
        for collection in [CcWarc2024, CcWarc2024Shuffled] {
            assert!(CcWarcMapping.allows_collection(&collection));
        }
        for collection in [CcWet2024, CcWet2024Shuffled] {
            assert!(!CcWarcMapping.allows_collection(&collection));
        }
    }
}
