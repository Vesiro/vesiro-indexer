// `populate` awaits into the aws-sdk, whose futures nest deeply enough that computing the layout
// of `populate`'s own future overruns rustc's default query depth of 128.
#![recursion_limit = "256"]

mod cc;
mod command;
mod db;
mod index_documents;
mod object_source;
mod store;

use std::path::PathBuf;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use clap::{Parser, Subcommand};
use reqwest::{Url, header};
use tracing_subscriber::{EnvFilter, Registry, fmt, layer::SubscriberExt};

use crate::command::{index::IndexOptions, records::RecordsCommand};

type IndexUuid = String;

#[derive(thiserror::Error, Debug)]
enum UuidFromIndexNameError {
    #[error("the node at {node_url} has no index named `{index_name}`")]
    IndexNotFound { index_name: String, node_url: Url },
    #[error("invalid response from server")]
    InvalidResponse,
}

#[derive(thiserror::Error, Debug)]
enum IndexNameFromUuidError {
    #[error("no target index given: pass --index-name or --uuid")]
    TargetOptionMustBeSet,
    #[error("the node at {node_url} has no index with uuid `{uuid}`")]
    UuidNotFound { uuid: String, node_url: Url },
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create indices on the node and fill them with documents.
    Index(#[clap(flatten)] IndexOptions),
    /// Inspect and repair the records the indexer keeps of the indices it has created.
    ///
    /// These commands work on the indexer's own records. Only `delete` reaches the node, and
    /// only when it is given one.
    Records {
        #[clap(subcommand)]
        command: RecordsCommand,
    },
}

#[derive(Parser, Debug)]
struct Cli {
    #[clap(subcommand)]
    command: Command,
    /// Directory the indexer keeps its records in, created if it does not exist yet. Defaults
    /// to `indexer-store` in the user's data directory.
    #[clap(long, value_name = "PATH", env = "VESIRO_INDEXER_RECORDS_PATH")]
    records_path: Option<PathBuf>,
}

/// What the index commands run against: a node, a client to reach it with, and the records the
/// indexer keeps of what it has done there.
struct IndexApp {
    client: reqwest::Client,
    db: store::Store,
    node_url: Url,
}

/// What the record commands run against. They work from the indexer's own records, and build a
/// client of their own on the rare occasion one of them is asked to reach the node.
struct RecordsApp {
    db: store::Store,
}

impl IndexApp {
    async fn uuid_from_index_name(&self, index_name: &str) -> anyhow::Result<IndexUuid> {
        let url = self.node_url.join("_stats")?;
        let response = self.client.get(url).send().await?;
        let obj: serde_json::Value = response.json().await?;
        let indices = obj["indices"]
            .as_object()
            .ok_or(UuidFromIndexNameError::InvalidResponse)?;
        if let Some(index) = indices.get(index_name) {
            if let Some(uuid) = index["uuid"].as_str() {
                Ok(uuid.to_string())
            } else {
                Err(UuidFromIndexNameError::InvalidResponse)?
            }
        } else {
            Err(UuidFromIndexNameError::IndexNotFound {
                index_name: index_name.to_string(),
                node_url: self.node_url.clone(),
            })?
        }
    }

    pub async fn index_name_from_uuid(&self, uuid: &IndexUuid) -> anyhow::Result<String> {
        let url = self.node_url.join("_stats")?;
        let response = self.client.get(url).send().await?;
        let obj: serde_json::Value = response.json().await?;
        let indices = obj["indices"]
            .as_object()
            .ok_or(UuidFromIndexNameError::InvalidResponse)?;
        for (index, obj) in indices {
            if obj["uuid"] == *uuid {
                return Ok(index.clone());
            }
        }
        Err(IndexNameFromUuidError::UuidNotFound {
            uuid: uuid.clone(),
            node_url: self.node_url.clone(),
        })?
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Read `.env` before anything else looks at the environment, so both `RUST_LOG` below and
    // the options that take an `env` can come from it. What it did is logged once there is a
    // subscriber to log it to.
    let dotenv = dotenvy::dotenv();

    // Init logging.
    let subscriber = Registry::default()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            ["vesiro_indexer=info", "tower_http=info", "aws_sdk_s3=info"]
                .join(",")
                .into()
        }));
    tracing::subscriber::set_global_default(subscriber)?;

    match dotenv {
        Ok(path) => tracing::info!(path = %path.display(), "read environment file"),
        // Not having a `.env` is the ordinary case, and nothing to say anything about.
        Err(error) if error.not_found() => {}
        Err(error) => tracing::warn!(%error, "could not read environment file"),
    }

    let cli = Cli::parse();

    // Init sled database. Resolving the default here keeps `records_path` from creating a data
    // directory the command was never going to use.
    let records_path = match cli.records_path {
        Some(path) => path,
        None => db::default_path()?,
    };
    tracing::info!(path = %records_path.display(), "opening records store");
    let db = store::Store::new(records_path);
    db.initialize()?;

    let result = match cli.command {
        Command::Index(options) => {
            let client = build_client(options.user.as_deref())?;

            // Ping the node to ensure it's reachable.
            let response = client.get(options.node_url.clone()).send().await?;
            tracing::info!(?response, "node response");

            let app = IndexApp {
                client,
                db,
                node_url: options.node_url,
            };
            app.handle_index_command(options.command).await
        }
        Command::Records { command } => RecordsApp { db }.handle_records_command(command).await,
    };

    result
}

fn build_client(user: Option<&str>) -> anyhow::Result<reqwest::Client> {
    let Some(user) = user else {
        return Ok(reqwest::Client::new());
    };
    let mut headers = header::HeaderMap::new();
    let auth_value = format!("Basic {}", STANDARD.encode(user));
    headers.insert(
        header::AUTHORIZATION,
        header::HeaderValue::from_str(&auth_value)?,
    );
    Ok(reqwest::Client::builder()
        .default_headers(headers)
        .danger_accept_invalid_certs(true)
        .build()?)
}
