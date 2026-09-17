use clap::Parser;

use crate::{IndexUuid, RecordsApp};

#[derive(Debug, Parser)]
pub struct RekeyOptions {
    /// The uuid to rekey.
    #[clap(long, value_name = "UUID")]
    from: IndexUuid,
    /// The uuid to rekey it to.
    #[clap(long, value_name = "UUID")]
    to: IndexUuid,
}

impl RecordsApp {
    pub fn rekey(&self, options: RekeyOptions) -> anyhow::Result<()> {
        self.db.rekey(&options.from, &options.to)?;
        tracing::info!(
            from = %options.from,
            to = %options.to,
            "rekeyed index",
        );
        Ok(())
    }
}
