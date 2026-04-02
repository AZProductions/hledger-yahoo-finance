#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use figment::{
    providers::{Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct App {
    #[clap(subcommand)]
    command: Command,

    /// Path to the hledger journal file. Defaults to $LEDGER_FILE.
    #[clap(short, long, env = "LEDGER_FILE")]
    file: PathBuf,

    /// Path to the directory that contains commodity price include-files.
    /// Defaults to `<journal-dir>/prices/`.
    #[clap(long)]
    commodity_path: Option<PathBuf>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Settings {
    file: Option<PathBuf>,
    commodity_path: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
enum Command {
    #[clap(about = "Fetch daily prices for all commodities in the journal")]
    Daily {
        #[clap(
            short,
            long,
            default_value = "EUR",
            help = "Base currency to use as the price reference (e.g., EUR, USD)"
        )]
        base_currency: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = App::parse();

    let journal_dir = app
        .file
        .parent()
        .context("journal file path has no parent directory")?;

    let config_toml = journal_dir.join("config.toml");

    let settings: Settings = Figment::new()
        .merge(Serialized::defaults(Settings {
            file: Some(app.file),
            commodity_path: app.commodity_path,
        }))
        .merge(Toml::file(&config_toml))
        .extract()
        .with_context(|| {
            format!(
                "failed to load configuration (looked for config.toml at {})",
                config_toml.display()
            )
        })?;

    let ledger_file = settings
        .file
        .context("no journal file specified (set LEDGER_FILE or pass --file)")?;

    let commodity_path = settings.commodity_path.unwrap_or_else(|| {
        ledger_file
            .parent()
            .expect("journal file path has no parent directory")
            .join("prices")
    });

    match app.command {
        Command::Daily { base_currency } => {
            hledger_tools::update_daily_prices(&base_currency, &commodity_path, &ledger_file)
                .await?;
        }
    }

    Ok(())
}
