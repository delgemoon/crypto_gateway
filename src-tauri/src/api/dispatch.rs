use crate::api::{binance, bullish, bybit, coincall, deribit, hyperliquid, mexc, okx, uniswap};
use crate::api::models::{Account, AccountSummary, Instrument, Order, OrderResult, OrderbookSnapshot, PlaceOrderRequest, Position, ReferenceData, Ticker, Trade, TransactionLog, instrument_to_ref};

pub async fn fetch_instruments(exchange: &str, currency: &str, kind: &str, testnet: bool) -> Result<Vec<Instrument>, String> {
    match exchange {
        "okx"         => okx::fetch_instruments(currency, kind).await,
        "bybit"       => bybit::fetch_instruments(currency, kind).await,
        "coincall"    => coincall::fetch_instruments(currency, kind, testnet).await,
        "binance"     => binance::fetch_instruments(currency, kind).await,
        "mexc"        => mexc::fetch_instruments(currency, kind).await,
        "hyperliquid" => hyperliquid::fetch_instruments(currency, kind).await,
        "uniswap"     => uniswap::fetch_instruments(currency, kind).await,
        "bullish"     => bullish::fetch_instruments(currency, kind).await,
        _             => deribit::fetch_instruments(currency, kind).await,
    }
}

pub async fn fetch_ticker(exchange: &str, instrument_name: &str, testnet: bool) -> Result<Ticker, String> {
    match exchange {
        "okx"         => okx::fetch_ticker(instrument_name).await,
        "bybit"       => bybit::fetch_ticker(instrument_name).await,
        "coincall"    => coincall::fetch_ticker(instrument_name, testnet).await,
        "binance"     => binance::fetch_ticker(instrument_name).await,
        "mexc"        => mexc::fetch_ticker(instrument_name).await,
        "hyperliquid" => hyperliquid::fetch_ticker(instrument_name).await,
        "uniswap"     => uniswap::fetch_ticker(instrument_name).await,
        "bullish"     => bullish::fetch_ticker(instrument_name).await,
        _             => deribit::fetch_ticker(instrument_name).await,
    }
}

pub async fn place_order(account: &Account, req: &PlaceOrderRequest) -> Result<OrderResult, String> {
    match account.exchange.as_str() {
        "okx"         => okx::place_order(req, account).await,
        "bybit"       => bybit::place_order(req, account).await,
        "coincall"    => coincall::place_order(req, account).await,
        "binance"     => binance::place_order(req, account).await,
        "mexc"        => mexc::place_order(req, account).await,
        "hyperliquid" => hyperliquid::place_order(req, account).await,
        "uniswap"     => uniswap::place_order(req, account).await,
        "bullish"     => bullish::place_order(req, account).await,
        _             => deribit::place_order(req, &account.api_key, &account.api_secret, account.testnet).await,
    }
}

pub async fn cancel_order(account: &Account, order_id: &str, instrument_name: Option<&str>) -> Result<bool, String> {
    match account.exchange.as_str() {
        "okx"         => okx::cancel_order(order_id, instrument_name, account).await,
        "bybit"       => bybit::cancel_order(order_id, instrument_name, account).await,
        "coincall"    => coincall::cancel_order(order_id, instrument_name, account).await,
        "binance"     => binance::cancel_order(order_id, instrument_name, account).await,
        "mexc"        => mexc::cancel_order(order_id, instrument_name, account).await,
        "hyperliquid" => hyperliquid::cancel_order(order_id, instrument_name, account).await,
        "uniswap"     => uniswap::cancel_order(order_id, instrument_name, account).await,
        "bullish"     => bullish::cancel_order(order_id, instrument_name, account).await,
        _             => deribit::cancel_order(order_id, &account.api_key, &account.api_secret, account.testnet).await,
    }
}

pub async fn get_open_orders(account: &Account, instrument_name: &str) -> Result<Vec<Order>, String> {
    match account.exchange.as_str() {
        "okx"         => okx::get_open_orders(instrument_name, account).await,
        "bybit"       => bybit::get_open_orders(instrument_name, account).await,
        "coincall"    => coincall::get_open_orders(instrument_name, account).await,
        "binance"     => binance::get_open_orders(instrument_name, account).await,
        "mexc"        => mexc::get_open_orders(instrument_name, account).await,
        "hyperliquid" => hyperliquid::get_open_orders(instrument_name, account).await,
        "uniswap"     => uniswap::get_open_orders(instrument_name, account).await,
        "bullish"     => bullish::get_open_orders(instrument_name, account).await,
        _             => deribit::get_open_orders(instrument_name, &account.api_key, &account.api_secret, account.testnet).await,
    }
}

pub async fn get_account_summary(account: &Account, currency: &str) -> Result<AccountSummary, String> {
    match account.exchange.as_str() {
        "okx"         => okx::get_account_summary(currency, account).await,
        "bybit"       => bybit::get_account_summary(currency, account).await,
        "coincall"    => coincall::get_account_summary(currency, account).await,
        "binance"     => binance::get_account_summary(currency, account).await,
        "mexc"        => mexc::get_account_summary(currency, account).await,
        "hyperliquid" => hyperliquid::get_account_summary(currency, account).await,
        "uniswap"     => uniswap::get_account_summary(currency, account).await,
        "bullish"     => bullish::get_account_summary(currency, account).await,
        _             => deribit::get_account_summary(currency, &account.api_key, &account.api_secret, account.testnet).await,
    }
}

pub async fn get_all_open_orders(account: &Account) -> Result<Vec<Order>, String> {
    match account.exchange.as_str() {
        "okx"         => okx::get_all_open_orders(account).await,
        "bybit"       => bybit::get_all_open_orders(account).await,
        "coincall"    => coincall::get_all_open_orders(account).await,
        "binance"     => binance::get_all_open_orders(account).await,
        "mexc"        => mexc::get_all_open_orders(account).await,
        "hyperliquid" => hyperliquid::get_all_open_orders(account).await,
        "uniswap"     => uniswap::get_all_open_orders(account).await,
        "bullish"     => bullish::get_all_open_orders(account).await,
        _             => deribit::get_all_open_orders(&account.api_key, &account.api_secret, account.testnet).await,
    }
}

pub async fn get_trade_history(account: &Account, start_ms: i64, end_ms: i64) -> Result<Vec<Trade>, String> {
    match account.exchange.as_str() {
        "okx"         => okx::get_trade_history(account, start_ms, end_ms).await,
        "bybit"       => bybit::get_trade_history(account, start_ms, end_ms).await,
        "coincall"    => coincall::get_trade_history(account, start_ms, end_ms).await,
        "binance"     => binance::get_trade_history(account, start_ms, end_ms).await,
        "mexc"        => mexc::get_trade_history(account, start_ms, end_ms).await,
        "hyperliquid" => hyperliquid::get_trade_history(account, start_ms, end_ms).await,
        "uniswap"     => uniswap::get_trade_history(account, start_ms, end_ms).await,
        "bullish"     => bullish::get_trade_history(account, start_ms, end_ms).await,
        _             => deribit::get_trade_history(&account.api_key, &account.api_secret, account.testnet, start_ms, end_ms).await,
    }
}

pub async fn get_positions(account: &Account, currency: &str) -> Result<Vec<Position>, String> {
    match account.exchange.as_str() {
        "okx"         => okx::get_positions(currency, account).await,
        "bybit"       => bybit::get_positions(currency, account).await,
        "coincall"    => coincall::get_positions(currency, account).await,
        "binance"     => binance::get_positions(currency, account).await,
        "mexc"        => mexc::get_positions(currency, account).await,
        "hyperliquid" => hyperliquid::get_positions(currency, account).await,
        "uniswap"     => uniswap::get_positions(currency, account).await,
        "bullish"     => bullish::get_positions(currency, account).await,
        _             => deribit::get_positions(currency, &account.api_key, &account.api_secret, account.testnet).await,
    }
}

pub async fn fetch_orderbook(exchange: &str, instrument_name: &str, depth: u32, testnet: bool) -> Result<OrderbookSnapshot, String> {
    match exchange {
        "okx"         => okx::fetch_orderbook(instrument_name, depth).await,
        "bybit"       => bybit::fetch_orderbook(instrument_name, depth).await,
        "coincall"    => coincall::fetch_orderbook(instrument_name, depth, testnet).await,
        "binance"     => binance::fetch_orderbook(instrument_name, depth).await,
        "mexc"        => mexc::fetch_orderbook(instrument_name, depth).await,
        "hyperliquid" => hyperliquid::fetch_orderbook(instrument_name, depth).await,
        "uniswap"     => uniswap::fetch_orderbook(instrument_name, depth).await,
        "bullish"     => bullish::fetch_orderbook(instrument_name, depth).await,
        _             => deribit::fetch_orderbook(instrument_name, depth).await,
    }
}

/// Fetch instruments and return them as canonical `ReferenceData` structs.
pub async fn fetch_reference_data(exchange: &str, currency: &str, kind: &str, testnet: bool) -> Result<Vec<ReferenceData>, String> {
    let instruments = fetch_instruments(exchange, currency, kind, testnet).await?;
    Ok(instruments.iter().map(|i| instrument_to_ref(exchange, i)).collect())
}

/// Fetch transaction log for an account over a date range (milliseconds).
/// Supported exchanges: deribit, bybit, coincall, bullish.
pub async fn get_transaction_log(
    account: &Account,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<TransactionLog>, String> {
    match account.exchange.as_str() {
        "deribit" => {
            // Deribit is per-currency; fetch BTC, ETH, SOL, USDC and merge
            let currencies = ["BTC", "ETH", "SOL", "USDC", "USDT", "USED", "STETH", "BUIDL", "BNB", "PAXG", "USYC"];
            let mut all: Vec<TransactionLog> = Vec::new();
            for cur in &currencies {
                match deribit::get_transaction_log(
                    &account.api_key, &account.api_secret, account.testnet,
                    cur, start_ms, end_ms,
                ).await {
                    Ok(mut logs) => all.append(&mut logs),
                    Err(e) => eprintln!("[dispatch] deribit tx_log {}: {}", cur, e),
                }
            }
            all.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
            Ok(all)
        }
        "bybit"    => bybit::get_transaction_log(account, start_ms, end_ms).await,
        "coincall" => coincall::get_transaction_log(account, start_ms, end_ms).await,
        "bullish"  => bullish::get_transaction_log(account, start_ms, end_ms).await,
        ex => Err(format!("get_transaction_log not supported for {}", ex)),
    }
}
