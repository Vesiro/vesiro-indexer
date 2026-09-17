use clap::Parser;
use reqwest::{StatusCode, Url};

use crate::{IndexUuid, RecordsApp, build_client};

/// Where to reach the node, and how to authenticate against it.
///
/// Flattened as an `Option`, so the handler cannot reach for a credential without also having a
/// node to send it to. `required = false` lets the whole group be left out, while `requires`
/// keeps `--user` from being given on its own.
#[derive(Debug, Parser)]
pub struct NodeOptions {
    /// The node the index lives on. Set it to delete the index there as well.
    #[clap(long, required = false)]
    node_url: Url,
    /// HTTPS user credentials in the format `username:password`.
    #[clap(
        long,
        requires = "node_url",
        env = "VESIRO_INDEXER_USER",
        hide_env_values = true
    )]
    user: Option<String>,
}

#[derive(Debug, Parser)]
pub struct DeleteOptions {
    /// The uuid of the index to delete.
    #[clap(long, value_name = "UUID")]
    uuid: IndexUuid,
    #[clap(flatten)]
    node: Option<NodeOptions>,
}

impl RecordsApp {
    pub async fn delete(&self, options: DeleteOptions) -> anyhow::Result<()> {
        // Read the record first: the index name lives there, and this refuses an index another
        // command is busy with before anything is deleted on the node.
        let info = self.db.index_info(&options.uuid)?;

        if let Some(node) = &options.node {
            delete_on_node(node, &info.index_name).await?;
        }

        self.db.delete(&options.uuid)?;
        tracing::info!(
            uuid = %info.uuid,
            index_name = %info.index_name,
            "deleted index record",
        );
        Ok(())
    }
}

async fn delete_on_node(node: &NodeOptions, index_name: &str) -> anyhow::Result<()> {
    let client = build_client(node.user.as_deref())?;
    let url = node.node_url.join(index_name)?;
    let response = client.delete(url).send().await?;
    let status = response.status();
    if status.is_success() {
        tracing::info!(%index_name, "deleted index on node");
    } else if status == StatusCode::NOT_FOUND {
        // Nothing to delete there; carry on and drop the record it left behind.
        tracing::warn!(%index_name, "index not found on node");
    } else {
        let body = response.text().await?;
        anyhow::bail!("failed to delete index on node ({status}): {body}");
    }
    Ok(())
}
