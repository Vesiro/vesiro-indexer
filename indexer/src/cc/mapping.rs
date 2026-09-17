use bincode::{Decode, Encode};
use clap::ValueEnum;
use serde_json::Value;
use vesiro_indexer_protocol::collection::Collection;

mod cc_warc;
mod cc_wet;
mod parse;
mod raw;

#[derive(Debug, Clone, ValueEnum, Encode, Decode)]
pub enum CcMappingOption {
    Raw,
    CcWet,
    CcWarc,
}

impl CcMappingOption {
    pub fn mapping(&self) -> Box<dyn Mapping + Send + Sync> {
        match self {
            CcMappingOption::Raw => Box::new(raw::RawMapping),
            CcMappingOption::CcWet => Box::new(cc_wet::CcWetMapping),
            CcMappingOption::CcWarc => Box::new(cc_warc::CcWarcMapping),
        }
    }
}

pub trait Mapping {
    /// Checks if the given collection is permitted by this mapping.
    fn allows_collection(&self, collection: &Collection) -> bool;

    /// Transforms a CcDocument into a serde_json::Value according to the mapping rules.
    fn transform_document(&self, document: &Value) -> anyhow::Result<(String, Value)>;

    /// Returns the mappings.
    fn mappings(&self) -> Value;
}
