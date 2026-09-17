use serde::Serialize;

use crate::cc;

pub struct CcObjectIterator<'a> {
    data: &'a [u8],
    warc_info: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct CcDocument {
    warc_info: serde_json::Value,
    entry: serde_json::Value,
}

#[derive(Debug, thiserror::Error)]
pub enum CcObjectIteratorError {
    #[error("invalid warc info")]
    InvalidWarcInfo,
}

impl CcObjectIterator<'_> {
    pub fn new(data: &[u8]) -> anyhow::Result<CcObjectIterator<'_>> {
        if let Some((data, entry)) = cc::parse::parse_entry(data)? {
            Ok(CcObjectIterator {
                data,
                warc_info: serde_json::to_value(entry)?,
            })
        } else {
            Err(CcObjectIteratorError::InvalidWarcInfo)?
        }
    }
}

impl Iterator for CcObjectIterator<'_> {
    type Item = anyhow::Result<CcDocument>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.data.is_empty() {
            return None;
        }

        match cc::parse::parse_entry(self.data) {
            Ok(Some((remaining, headers))) => {
                self.data = remaining;
                match serde_json::to_value(&headers) {
                    Ok(document) => Some(Ok(CcDocument {
                        warc_info: self.warc_info.clone(),
                        entry: document,
                    })),
                    Err(e) => Some(Err(e.into())),
                }
            }
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}
