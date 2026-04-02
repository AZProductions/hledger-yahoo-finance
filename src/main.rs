#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use figment::{
    Figment,
    providers::{Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct App {
    #[clap(subcommand)]
    command: Command,

    #[clap(flatten)]
    config: Config,
}

#[derive(Parser, Debug, Default, Serialize, Deserialize)]
struct Config {
    /// Path to the hledger journal file. Defaults to $`LEDGER_FILE`.
    #[clap(short, long, env = "LEDGER_FILE", global = true)]
    file: Option<PathBuf>,

    /// Path to the directory that contains commodity price include-files.
    /// Defaults to `<journal-dir>/prices/`.
    #[clap(long, global = true)]
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

    let config_toml = app
        .config
        .file
        .as_deref()
        .and_then(|p| p.parent())
        .map(|dir| dir.join("config.toml"));

    let mut figment = Figment::new().merge(Serialized::defaults(app.config));

    if let Some(ref toml_path) = config_toml {
        figment = figment.merge(Toml::file(toml_path));
    }

    let config: Config = figment.extract().context("failed to load configuration")?;

    let ledger_file = config
        .file
        .context("no journal file specified (set LEDGER_FILE or pass --file)")?;

    let commodity_path = config.commodity_path.unwrap_or_else(|| {
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
