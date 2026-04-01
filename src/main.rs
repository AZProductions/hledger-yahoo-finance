#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct App {
    #[clap(subcommand)]
    command: Command,
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
    #[clap(about = "Import statements from a bank account")]
    Import { csv: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    match App::parse().command {
        Command::Daily { base_currency } => {
            hledger_tools::update_daily_prices(&base_currency).await?;
        }
        Command::Import { csv } => {
            hledger_tools::import_statements(&csv).await?;
        }
    }

    Ok(())
}
