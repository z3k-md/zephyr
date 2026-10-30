//! Stock and crypto quotes for the ticker card ("TSLA", "$aapl", "eth"), from Yahoo Finance's
//! chart endpoint, which needs no key. One request gives the price, the day's change and the
//! points for its chart. Replies are kept for half a minute, so retyping a symbol is instant.

use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::util::now_ms;

const FRESH_MS: u64 = 30 * 1000;
const TIMEOUT: Duration = Duration::from_secs(8);
const DAY_SECS: i64 = 24 * 60 * 60;

/// A quote and its recent history.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub symbol: String,
    pub name: String,
    pub currency: String,
    /// Where it trades, as Yahoo names it ("NasdaqGS", "NYSE").
    pub exchange: String,
    pub crypto: bool,
    pub price: f64,
    /// What the change is measured from: the close before the chart starts, or for crypto's
    /// day the price 24 hours ago.
    pub reference: f64,
    /// The chart's span: `1d`, `5d`, `1mo` or `1y`.
    pub range: String,
    /// The last trade, in milliseconds since the epoch.
    pub traded_at: u64,
    /// The chart: (seconds since the epoch, price), oldest first.
    pub points: Vec<(i64, f64)>,
    pub fetched_at: u64,
}

/// Yahoo's symbol for what was typed: crypto trades against the dollar (`ETH-USD`), and only
/// the characters symbols use get through to the URL.
fn yahoo_symbol(symbol: &str, crypto: bool) -> Option<String> {
    let symbol = symbol.trim().to_uppercase();
    let valid = !symbol.is_empty()
        && symbol.len() <= 12
        && symbol
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '^' | '='));
    if !valid {
        return None;
    }
    Some(if crypto && !symbol.contains('-') {
        format!("{symbol}-USD")
    } else {
        symbol
    })
}

/// Yahoo's range and interval for a chart span, with about a hundred points each. Crypto
/// never closes, so its day is the last 24 hours, cut from two days; a stock's is its
/// trading day.
fn span(range: &str, crypto: bool) -> Option<&'static str> {
    Some(match range {
        "1d" if crypto => "range=2d&interval=15m",
        "1d" => "range=1d&interval=5m",
        "5d" => "range=5d&interval=1h",
        "1mo" => "range=1mo&interval=1d",
        "1y" => "range=1y&interval=1wk",
        _ => return None,
    })
}

/// The quote for `symbol` over `range`, or None when Yahoo has no such symbol.
pub async fn quote(symbol: &str, crypto: bool, range: &str) -> Result<Option<Quote>, String> {
    static RECENT: Mutex<Vec<Quote>> = Mutex::new(Vec::new());
    let (Some(symbol), Some(span)) = (yahoo_symbol(symbol, crypto), span(range, crypto)) else {
        return Ok(None);
    };
    if let Ok(recent) = RECENT.lock()
        && let Some(quote) = recent.iter().find(|quote| {
            quote.symbol == symbol
                && quote.range == range
                && now_ms().saturating_sub(quote.fetched_at) < FRESH_MS
        })
    {
        return Ok(Some(quote.clone()));
    }

    let url = format!("https://query1.finance.yahoo.com/v8/finance/chart/{symbol}?{span}");
    let response = crate::util::http()
        .get(url)
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let raw: Raw = response
        .error_for_status()
        .map_err(|err| err.to_string())?
        .json()
        .await
        .map_err(|err| err.to_string())?;
    let Some(found) = raw.into_quote(crypto, range, now_ms()) else {
        return Ok(None);
    };
    if let Ok(mut recent) = RECENT.lock() {
        recent.retain(|quote| quote.symbol != found.symbol || quote.range != found.range);
        recent.push(found.clone());
        if recent.len() > 16 {
            recent.remove(0);
        }
    }
    Ok(Some(found))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Raw {
    chart: RawChart,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawChart {
    result: Option<Vec<RawResult>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawResult {
    meta: RawMeta,
    timestamp: Vec<i64>,
    indicators: RawIndicators,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawMeta {
    symbol: String,
    currency: String,
    full_exchange_name: String,
    instrument_type: String,
    regular_market_price: Option<f64>,
    regular_market_time: i64,
    chart_previous_close: Option<f64>,
    previous_close: Option<f64>,
    long_name: Option<String>,
    short_name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawIndicators {
    quote: Vec<RawSeries>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawSeries {
    close: Vec<Option<f64>>,
}

impl Raw {
    fn into_quote(self, crypto: bool, range: &str, now: u64) -> Option<Quote> {
        let result = self.chart.result?.into_iter().next()?;
        let meta = result.meta;
        let price = meta.regular_market_price?;
        let closes = result
            .indicators
            .quote
            .into_iter()
            .next()
            .unwrap_or_default()
            .close;
        let mut points: Vec<(i64, f64)> = result
            .timestamp
            .into_iter()
            .zip(closes)
            .filter_map(|(time, close)| close.map(|close| (time, close)))
            .collect();
        let crypto = crypto || meta.instrument_type == "CRYPTOCURRENCY";
        let reference = if crypto && range == "1d" {
            let since = points.last().map_or(0, |(time, _)| time - DAY_SECS);
            points.retain(|(time, _)| *time >= since);
            points.first().map(|(_, price)| *price)
        } else {
            meta.chart_previous_close
                .or(meta.previous_close)
                .or(points.first().map(|(_, price)| *price))
        }
        .unwrap_or(price);
        let name = meta
            .long_name
            .or(meta.short_name)
            .unwrap_or_else(|| meta.symbol.clone());
        Some(Quote {
            // "Ethereum USD" reads as just Ethereum on a card already priced in dollars.
            name: name.strip_suffix(" USD").unwrap_or(&name).to_string(),
            symbol: meta.symbol,
            currency: meta.currency,
            exchange: meta.full_exchange_name,
            crypto,
            price,
            reference,
            range: range.to_string(),
            traded_at: (meta.regular_market_time.max(0) as u64) * 1000,
            points,
            fetched_at: now,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_are_cleaned_and_crypto_trades_against_the_dollar() {
        assert_eq!(yahoo_symbol("tsla", false).as_deref(), Some("TSLA"));
        assert_eq!(yahoo_symbol(" brk.b ", false).as_deref(), Some("BRK.B"));
        assert_eq!(yahoo_symbol("eth", true).as_deref(), Some("ETH-USD"));
        assert_eq!(yahoo_symbol("btc-eur", true).as_deref(), Some("BTC-EUR"));
        assert_eq!(yahoo_symbol("", false), None);
        assert_eq!(yahoo_symbol("a/b", false), None);
        assert_eq!(yahoo_symbol("tsla?x=1", false), None);
        assert_eq!(yahoo_symbol("abcdefghijklm", false), None);
    }

    fn raw(json: &str) -> Raw {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_stock_measures_from_the_last_close_and_skips_gaps() {
        let quote = raw(r#"{"chart":{"result":[{
            "meta":{"symbol":"TSLA","currency":"USD","fullExchangeName":"NasdaqGS",
              "instrumentType":"EQUITY","regularMarketPrice":403.3,"regularMarketTime":1000,
              "chartPreviousClose":404.22,"longName":"Tesla, Inc."},
            "timestamp":[1,2,3],
            "indicators":{"quote":[{"close":[400.0,null,403.3]}]}}]}}"#)
        .into_quote(false, "1d", 7)
        .unwrap();
        assert_eq!(quote.name, "Tesla, Inc.");
        assert_eq!(quote.reference, 404.22);
        assert_eq!(quote.points, vec![(1, 400.0), (3, 403.3)]);
        assert_eq!(quote.traded_at, 1_000_000);
        assert!(!quote.crypto);
    }

    #[test]
    fn crypto_keeps_the_last_day_and_measures_from_its_start() {
        let late = 3 * DAY_SECS;
        let json = format!(
            r#"{{"chart":{{"result":[{{
            "meta":{{"symbol":"ETH-USD","currency":"USD","instrumentType":"CRYPTOCURRENCY",
              "regularMarketPrice":2200.0,"chartPreviousClose":1.0,"longName":"Ethereum USD"}},
            "timestamp":[{},{},{}],
            "indicators":{{"quote":[{{"close":[1000.0,2100.0,2200.0]}}]}}}}]}}}}"#,
            late - DAY_SECS - 60,
            late - DAY_SECS,
            late
        );
        let quote = raw(&json).into_quote(true, "1d", 0).unwrap();
        assert_eq!(quote.name, "Ethereum");
        assert_eq!(quote.reference, 2100.0);
        assert_eq!(quote.points.len(), 2);
    }

    #[test]
    fn an_unknown_symbol_has_no_quote() {
        let missing = raw(r#"{"chart":{"result":null,"error":{"code":"Not Found"}}}"#);
        assert!(missing.into_quote(false, "1d", 0).is_none());
    }
}
