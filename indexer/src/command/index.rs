pub mod create;
pub mod populate;

use clap::{Parser, Subcommand};
use reqwest::Url;

use crate::{
    IndexApp,
    command::index::{create::CreateIndexOptions, populate::PopulateOptions},
};

#[derive(Debug, Parser)]
pub struct IndexOptions {
    #[clap(subcommand)]
    pub command: IndexCommand,
    /// The node the index lives on.
    #[clap(long, env = "VESIRO_INDEXER_NODE_URL")]
    pub node_url: Url,
    /// HTTPS user credentials in the format `username:password`.
    #[clap(long, env = "VESIRO_INDEXER_USER", hide_env_values = true)]
    pub user: Option<String>,
}

/// The commands that act on an index on the node. Every one of them needs a node to talk to,
/// which is why the node url is an option of the parent command rather than of each of these.
#[derive(Debug, Subcommand)]
pub enum IndexCommand {
    /// Create a new index.
    Create(#[clap(flatten)] CreateIndexOptions),
    /// Append documents to an index.
    Populate(#[clap(flatten)] PopulateOptions),
}

impl IndexApp {
    pub async fn handle_index_command(self, command: IndexCommand) -> anyhow::Result<()> {
        match command {
            IndexCommand::Create(options) => {
                let (uuid, index_name) = self.create_index(options).await?;
                tracing::info!(%uuid, %index_name, "created index");
                Ok(())
            }
            IndexCommand::Populate(options) => self.populate(options).await,
        }
    }
}
