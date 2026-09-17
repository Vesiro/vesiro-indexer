use bincode::{Decode, Encode};
use clap::ValueEnum;
use num_enum::TryFromPrimitive;
use serde::{Deserialize, Serialize};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Encode,
    Decode,
    Serialize,
    Deserialize,
    TryFromPrimitive,
    ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
#[repr(u8)]
pub enum Collection {
    CcWarc2024 = 1,
    CcWarc2024Shuffled = 2,
    CcWet2024 = 3,
    CcWet2024Shuffled = 4,
}

impl TryFrom<String> for Collection {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "cc-warc-2024" => Ok(Collection::CcWarc2024),
            "cc-warc-2024-shuffled" => Ok(Collection::CcWarc2024Shuffled),
            "cc-wet-2024" => Ok(Collection::CcWet2024),
            "cc-wet-2024-shuffled" => Ok(Collection::CcWet2024Shuffled),
            _ => Err(anyhow::anyhow!("unknown collection: {}", value)),
        }
    }
}

impl ToString for Collection {
    fn to_string(&self) -> String {
        serde_json::to_string(self)
            .unwrap()
            .trim_matches('"')
            .to_string()
    }
}
