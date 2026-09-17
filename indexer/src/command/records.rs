pub mod delete;
pub mod list;
pub mod rekey;

use clap::Subcommand;

use crate::{
    RecordsApp,
    command::records::{delete::DeleteOptions, list::ListIndicesOptions, rekey::RekeyOptions},
};

/// The commands that read and repair the indexer's own records.
///
/// Only `delete` ever leaves the machine, and only when it is told which node to reach.
#[derive(Debug, Subcommand)]
pub enum RecordsCommand {
    /// Lists the uuids of the indices the indexer has a record of.
    List(#[clap(flatten)] ListIndicesOptions),
    /// Change the uuid an index is recorded under.
    Rekey(#[clap(flatten)] RekeyOptions),
    /// Drop the record of an index, and optionally the index itself.
    Delete(#[clap(flatten)] DeleteOptions),
}

impl RecordsApp {
    pub async fn handle_records_command(&self, command: RecordsCommand) -> anyhow::Result<()> {
        match command {
            RecordsCommand::List(options) => self.list_indices(options),
            RecordsCommand::Rekey(options) => self.rekey(options),
            RecordsCommand::Delete(options) => self.delete(options).await,
        }
    }
}
