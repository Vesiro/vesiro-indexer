use clap::Parser;
use vesiro_indexer_protocol::collection::Collection;

use crate::{IndexApp, IndexUuid, cc::mapping::CcMappingOption, db};

#[derive(Debug, Parser)]
pub struct CreateIndexOptions {
    #[clap(long, env = "VESIRO_INDEXER_COLLECTION")]
    collection: Collection,
    #[clap(long, default_value_t = 1, env = "VESIRO_INDEXER_NUMBER_OF_SHARDS")]
    number_of_shards: u32,
    #[clap(long, default_value_t = 0, env = "VESIRO_INDEXER_NUMBER_OF_REPLICAS")]
    number_of_replicas: u32,
    #[clap(long, env = "VESIRO_INDEXER_CODEC")]
    codec: String,
    #[clap(long, env = "VESIRO_INDEXER_INDEX_NAME")]
    index_name: Option<String>,
    #[clap(long, env = "VESIRO_INDEXER_MAPPING")]
    mapping: CcMappingOption,
    #[clap(long, default_value_t = false, env = "VESIRO_INDEXER_ENABLE_SOURCE")]
    enable_source: bool,
}

impl IndexApp {
    pub async fn create_index(
        &self,
        options: CreateIndexOptions,
    ) -> anyhow::Result<(IndexUuid, String)> {
        let index_name = options
            .index_name
            .unwrap_or_else(|| options.collection.to_string());
        let mapping = options.mapping.mapping();

        tracing::info!(?options.mapping, "using mapping");
        if !mapping.allows_collection(&options.collection) {
            // TODO:
            //   Error type.
            Err(anyhow::anyhow!(
                "mapping does not allow collection: {:?}",
                options.collection
            ))?
        }

        let mut mappings = mapping.mappings();
        set_enable_source(&mut mappings, options.enable_source)?;
        let body = serde_json::json!(
            {
                "settings": {
                    "number_of_shards": options.number_of_shards,
                    "number_of_replicas": options.number_of_replicas,
                    "index": {
                        "codec": options.codec,
                    }
                },
                "mappings": mappings,
            }
        );

        let response = self
            .client
            .put(self.node_url.join(&index_name)?)
            .json(&body)
            .send()
            .await?;
        if response.status().is_success() {
            let uuid = self.uuid_from_index_name(&index_name).await?;
            tracing::info!(%uuid, %index_name, "created index");
            let index_info = db::IndexInfo {
                uuid: uuid.clone(),
                index_name: index_name.clone(),
                collection: options.collection,
                mapping: options.mapping,
            };
            self.db.create(index_info)?;
            Ok((uuid, index_name))
        } else {
            let body = response.text().await?;
            Err(anyhow::anyhow!("failed to create index: {}", body))?
        }
    }
}

fn set_enable_source(mappings: &mut serde_json::Value, enable: bool) -> anyhow::Result<()> {
    let obj = mappings
        .as_object_mut()
        .ok_or(anyhow::anyhow!("mappings is not an object"))?;
    obj.insert(
        "_source".to_string(),
        serde_json::json!({
            "enabled": enable,
        }),
    );
    Ok(())
}
