use std::path::PathBuf;

use bincode::{Decode, Encode};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::collection::Collection;

#[allow(dead_code)]
enum DbKeySection {
    _Reserved = 0,
    Collection = 1,
}

#[derive(Debug, thiserror::Error)]
pub enum DbCollectionObjectKeyError {
    #[error("invalid selection: {0}")]
    InvalidSelection(u8),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DbCollectionObjectKey(pub [u8; 6]);

impl DbCollectionObjectKey {
    pub fn new(collection: &Collection, index: u32) -> Self {
        let mut key = [0u8; 6];
        key[0] = DbKeySection::Collection as u8;
        key[1] = *collection as u8;
        key[2..6].copy_from_slice(&index.to_be_bytes());
        DbCollectionObjectKey(key)
    }

    pub fn as_array(&self) -> &[u8; 6] {
        &self.0
    }

    pub fn collection(&self) -> anyhow::Result<Collection> {
        if self.0[0] != DbKeySection::Collection as u8 {
            Err(DbCollectionObjectKeyError::InvalidSelection(self.0[0]))?;
        }
        Ok(Collection::try_from(self.0[1])?)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        self.collection()?;
        Ok(())
    }
}

impl AsRef<[u8]> for DbCollectionObjectKey {
    fn as_ref(&self) -> &[u8] {
        self.as_array()
    }
}

impl Serialize for DbCollectionObjectKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for DbCollectionObjectKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 6 {
            return Err(serde::de::Error::custom(format!(
                "invalid length: expected 6, got {}",
                bytes.len()
            )));
        }
        let mut array = [0u8; 6];
        array.copy_from_slice(&bytes);
        Ok(DbCollectionObjectKey(array))
    }
}

impl ToString for DbCollectionObjectKey {
    fn to_string(&self) -> String {
        hex::encode(self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum DbObjectEntryDataLocation {
    FilePath(PathBuf),
    S3Key(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum DbObjectEntryStatus {
    Dirty {
        content_length: u64,
    },
    Downloading {
        #[bincode(with_serde)]
        time_start: DateTime<Utc>,
        content_length: u64,
    },
    Completed {
        #[bincode(with_serde)]
        time_start: DateTime<Utc>,
        #[bincode(with_serde)]
        time_end: DateTime<Utc>,

        content_length: u64,
        sha256: String,
        data_location: DbObjectEntryDataLocation,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub struct DbObjectEntry {
    pub s3_key: String,
    pub status: DbObjectEntryStatus,
}
