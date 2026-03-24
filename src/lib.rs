#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]

use std::{
    collections::HashMap,
    convert::Infallible,
    fs::File,
    io::{BufRead, BufReader, ErrorKind, Write},
    path::{Path, PathBuf},
};

use yahoo_finance_api as yahoo;

#[allow(
    clippy::needless_pass_by_value,
    reason = "for every error type E, &E is also an error type, so passing `Option<&E>` is unnecessary complicated"
)]
fn report_application_bug<E: std::error::Error>(error_string: &str, error: Option<E>) -> ! {
    eprintln!(
        "An unexpected problem occured that the application can't recover from.\n\nDetails about the error are below. If you believe the invocation of hledger-get-market-prices is correct, I'd appreciate a bug report at {}/issues/new.\n\nError message: {error_string}\nError: {error:?}",
        env!("CARGO_PKG_REPOSITORY")
    );

    std::process::exit(1);
}

pub async fn search_stock_symbol(search_query: String) {
    let provider = yahoo::YahooConnector::new().unwrap_or_else(|error| {
        report_application_bug("Could not create YahooConnector", Some(error))
    });

    let resp = provider
        .search_ticker(&search_query)
        .await
        .unwrap_or_else(|error| {
            report_application_bug(
                "yahoo_finance_api returned error during search",
                Some(error),
            );
        });

    println!("{:>20} | {:>9} – {:40}", "Type", "Symbol", "Name");
    println!();
    for item in resp.quotes {
        let quote_type = item.quote_type;
        let symbol = item.symbol.clone();
        let name = item.long_name.clone();
        println!("{:>20} | {:>9} – {:40}", quote_type, symbol, name);
    }
}

#[must_use]
pub fn get_journal_file_data(journal_file: &Path) -> HashMap<String, String> {
    let file = File::open(journal_file);
    let file = match file {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            // new file => no entries
            return HashMap::new();
        }
        Err(e) => report_application_bug("Couldn't open journal file", Some(e)),
    };

    BufReader::new(file)
        .lines()
        .map(|line| {
            line.unwrap_or_else(|e| {
                report_application_bug("Getting line from journal file failed", Some(e))
            })
            .trim_start()
            .to_string()
        })
        .filter(|line| !line.starts_with(';')) // filter comment lines
        .map(|line| {
            let (first_part, last_part) = line.split_once(' ').unwrap_or_else(|| {
                report_application_bug::<Infallible>(&format!("Contains no space: {line}"), None);
            });
            if first_part != "P" {
                report_application_bug::<Infallible>(
                    &format!("{line} is not a market price"),
                    None,
                );
            }
            let (date, price_info) = last_part.split_once(' ').unwrap_or_else(|| {
                report_application_bug::<Infallible>(
                    &format!("Contains only one space: {line}"),
                    None,
                );
            });
            (date.to_string(), price_info.to_string())
        })
        .collect()
}

pub async fn get_history_for_stock(
    stock_symbol: String,
    stock_commodity_name: String,
    currency_commodity_name: String,
    journal_file: PathBuf,
    separator: char,
    decimal_digits: Option<usize>,
    currency_symbol_before: bool,
) {
    let provider = yahoo::YahooConnector::new().unwrap_or_else(|error| {
        report_application_bug("Could not create YahooConnector", Some(error))
    });

    let response = provider
        .get_quote_range(&stock_symbol, "1d", "max")
        .await
        .unwrap_or_else(|error| {
            report_application_bug(
                "yahoo_finance_api returned error during history fetch",
                Some(error),
            )
        });

    let quotes = response.quotes().unwrap_or_else(|error| {
        report_application_bug("Could not extract quotes from response", Some(error))
    });

    // The `api_data` hashmap uses the date (in format YYYY-MM-DD, as used by
    // the API as well as hledger) as key. As value, the string that should be
    // put behind the date in the journal file (commodity name and price) is
    // used. The idea behind this is that we need to merge this hashmap with the
    // current journal file contents, and we don't want to parse this file any
    // further than necessary to accomplish the merge.
    let api_data: HashMap<String, String> = quotes
        .iter()
        .map(|quote| {
            let date_str = time::OffsetDateTime::from_unix_timestamp(quote.timestamp)
                .unwrap_or_else(|_| {
                    report_application_bug::<Infallible>("Could not parse timestamp", None);
                })
                .date()
                .to_string();

            (date_str, {
                let price = quote.close;
                let mut price_string: String = decimal_digits.map_or_else(
                    || format!("{price}"),
                    |decimal_digits| format!("{price:.decimal_digits$}"),
                );

                if separator != '.' {
                    price_string = price_string.replace('.', &separator.to_string());
                }

                if currency_symbol_before {
                    format!("{stock_commodity_name} {currency_commodity_name}{price_string}")
                } else {
                    format!("{stock_commodity_name} {price_string} {currency_commodity_name}")
                }
            })
        })
        .collect();

    if quotes.len() != api_data.len() {
        report_application_bug::<Infallible>(
            &format!(
                "There are duplicate days in the API response: {} != {}",
                quotes.len(),
                api_data.len()
            ),
            None,
        );
    }

    let file_data = get_journal_file_data(&journal_file);

    let mut new_data = file_data;
    new_data.extend(api_data);

    let mut new_data: Vec<(String, String)> = new_data.into_iter().collect();
    new_data.sort_by(|(a, _), (b, _)| a.cmp(b).reverse());

    let mut file = File::create(&journal_file)
        .unwrap_or_else(|e| report_application_bug("Couldn't open journal file", Some(e)));

    writeln!(
        file,
        "; Generated by {}",
        concat!(env!("CARGO_PKG_NAME"), " V", env!("CARGO_PKG_VERSION"))
    )
    .unwrap_or_else(|e| report_application_bug("Failed writing to journal file", Some(e)));
    for (current_datetime, price_info) in &new_data {
        writeln!(file, "P {current_datetime} {price_info}")
            .unwrap_or_else(|e| report_application_bug("Failed writing to journal file", Some(e)));
    }
}

pub async fn print_commodities() {
    use hledger_parse::{Journal, parse_journal};
    use std::collections::BTreeSet;

    let journal_file_path = std::env::var("LEDGER_FILE").unwrap_or_else(|error| {
        match error {
            std::env::VarError::NotPresent => {
                eprintln!("Error: HLEDGER_JOURNAL_FILE environment variable is not set");
            }
            std::env::VarError::NotUnicode(_) => {
                eprintln!(
                    "Error: HLEDGER_JOURNAL_FILE environment variable contains invalid unicode"
                );
            }
        }
        std::process::exit(1);
    });

    let file_contents = std::fs::read_to_string(&journal_file_path).unwrap_or_else(|error| {
        report_application_bug("Couldn't read journal file", Some(error));
    });

    let base_path = std::path::PathBuf::from(&journal_file_path)
        .parent()
        .map(|v| v.to_owned());

    let mut input = file_contents.as_str();
    let journal: Journal = parse_journal(&mut input, base_path).unwrap_or_else(|error| {
        report_application_bug("Failed to parse journal file", Some(error));
    });

    let mut commodities: BTreeSet<String> = BTreeSet::new();

    // Iterate through all transactions and collect commodities
    for commodity in journal.commodities() {
        commodities.insert(commodity.name.to_string());
    }

    println!("Commodities found in journal file:");
    for commodity in commodities {
        println!("  {}", commodity);
    }
}
