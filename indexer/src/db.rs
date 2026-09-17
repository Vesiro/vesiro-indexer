//! Record encodings for the master database.
use std::path::PathBuf;

use bincode::{Decode, Encode};
use directories::ProjectDirs;
use vesiro_indexer_protocol::collection::Collection;

use crate::{IndexUuid, cc::mapping::CcMappingOption};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DbPrefix {
    Version = 0,
    IndexInfo = 1,
}

impl DbPrefix {
    pub fn key(&self) -> [u8; 1] {
        [*self as u8]
    }
}

/// The layout every database in the store is written in.
pub const LAYOUT_VERSION: u32 = 1;

/// The directory the records are stored under, inside the data directory.
const STORE_DIR_NAME: &str = "indexer-store";

mod default_path {
    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        #[error("no valid home directory could be determined for this platform")]
        NoHomeDirectory,
    }
}

/// The directory the records are stored in when no other path is given.
///
/// Nothing is created here - the store creates its own directory when a command first reaches
/// it.
///
/// This is the platform's local data directory - `~/.local/share/vesiro-indexer` on Linux,
/// `~/Library/Application Support/com.vesiro.vesiro-indexer` on macOS and `%LOCALAPPDATA%` on
/// Windows.
pub fn default_path() -> anyhow::Result<PathBuf> {
    let project_dirs = ProjectDirs::from("com", "vesiro", "vesiro-indexer")
        .ok_or(default_path::Error::NoHomeDirectory)?;
    Ok(project_dirs.data_local_dir().join(STORE_DIR_NAME))
}

mod index_info_key {
    #[derive(Debug, thiserror::Error)]
    pub enum NewError {
        #[error("invalid uuid length: {0}")]
        InvalidUuidLength(usize),
    }
}
pub struct IndexInfoKey([u8; 23]);

impl IndexInfoKey {
    pub fn new(uuid: IndexUuid) -> anyhow::Result<IndexInfoKey> {
        if uuid.len() != 22 {
            Err(index_info_key::NewError::InvalidUuidLength(uuid.len()))
                .map_err(anyhow::Error::from)?;
        }
        let mut bytes = [0u8; 23];
        bytes[0] = DbPrefix::IndexInfo as u8;
        bytes[1..].copy_from_slice(uuid.as_bytes());
        Ok(Self(bytes))
    }
}

impl AsRef<[u8]> for IndexInfoKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Debug, Encode, Decode)]
pub struct IndexInfo {
    pub uuid: IndexUuid,
    pub index_name: String,
    pub collection: Collection,
    pub mapping: CcMappingOption,
}

#[derive(Debug, Encode, Decode)]
pub struct ObjectState {
    pub offset: u64,
    pub done: bool,
}

impl Default for ObjectState {
    fn default() -> Self {
        Self {
            offset: 0,
            done: false,
        }
    }
}
