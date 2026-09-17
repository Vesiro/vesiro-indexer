use std::io::{self, Write};

use clap::{Parser, ValueEnum};

use crate::RecordsApp;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DisplayMode {
    Table,
    Json,
}

#[derive(Debug, Parser)]
pub struct ListIndicesOptions {
    /// Output format.
    #[clap(long, value_enum, default_value = "table")]
    display_mode: DisplayMode,
}

impl RecordsApp {
    pub fn list_indices(&self, options: ListIndicesOptions) -> anyhow::Result<()> {
        match options.display_mode {
            DisplayMode::Table => {
                for (index_info, document_count) in self.db.list()? {
                    println!(
                        "{}\t{}\t{}\t{:?}\t{}",
                        index_info.uuid,
                        index_info.index_name,
                        index_info.collection.to_string(),
                        index_info.mapping,
                        document_count
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "busy".into()),
                    );
                }
            }
            DisplayMode::Json => {
                let mut acc = Vec::new();
                for (index_info, document_count) in self.db.list()? {
                    acc.push(serde_json::json!({
                        "uuid": index_info.uuid,
                        "index_name": index_info.index_name,
                        "collection": index_info.collection.to_string(),
                        "mapping": format!("{:?}", index_info.mapping),
                        "document_count": document_count,
                        "busy": document_count.is_none(),
                    }));
                }
                let mut stdout = io::stdout().lock();
                serde_json::to_writer(&mut stdout, &acc)?;
                writeln!(stdout)?;
            }
        }
        Ok(())
    }
}
