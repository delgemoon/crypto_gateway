//! Bullish public market data via WebSocket.
//!
//! Public WS endpoint: `wss://api.exchange.bullish.com/trading-api/v1/market-data/orderbook`
//! No auth needed for public market data.
//!
//! Subscribe with:
//!   `{"jsonrpc":"2.0","type":"command","method":"subscribe","params":{"topic":"l2Orderbook","symbol":"BTCUSDC"},"id":"..."}`
//!
//! Keepalive every 5 min:
//!   `{"jsonrpc":"2.0","type":"command","method":"keepalivePing","id":"..."}`
//!
//! Message format: `{"topic":"l2Orderbook","symbol":"BTCUSDC","data":{...},"type":"data","jsonrpc":"2.0"}`
//!
//! For ticker we also subscribe `l1Orderbook` (best bid/ask + last).

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use tokio_tungstenite::{connect_async_tls_with_config, tungstenite::Message};

use crate::api::models::{MarketBookEvent, MarketTickerEvent};
use super::MarketCmd;

const WS_URL: &str = "wss://api.exchange.bullish.com/trading-api/v1/market-data/orderbook";
const MAX_LEVELS: usize = 50;

/// Integer key for price levels: round to 8 decimal places for stable comparison.
fn price_key(p: f64) -> i64 { (p * 1e8).round() as i64 }
fn key_price(k: i64) -> f64 { k as f64 / 1e8 }

type BookSide = BTreeMap<i64, f64>;

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

fn get_id() -> String {
    now_ms().to_string()
}

fn pf(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

pub fn spawn(
    app: AppHandle,
    exchange_symbol: String,
    symbol: String,
    emit_interval_ms: u32,
    mut cmd_rx: mpsc::Receiver<MarketCmd>,
) {
    tokio::spawn(async move {
        let mut backoff = 1u64;
        loop {
            if cmd_rx.try_recv().is_ok() { break; }
            match run_once(&app, &exchange_symbol, &symbol, emit_interval_ms, &mut cmd_rx).await {
                Ok(()) => break,
                Err(e) => {
                    eprintln!("[market/bullish][{}] {}", exchange_symbol, e);
                    sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(60);
                }
            }
        }
    });
}

async fn run_once(
    app: &AppHandle,
    exch_sym: &str,
    symbol: &str,
    emit_interval_ms: u32,
    cmd_rx: &mut mpsc::Receiver<MarketCmd>,
) -> Result<(), String> {
    eprintln!("[market/bullish][{}] connecting to {}", exch_sym, WS_URL);
    let (ws, _) = connect_async_tls_with_config(WS_URL, None, false, None)
        .await.map_err(|e| e.to_string())?;
    let (mut write, mut read) = ws.split();
    eprintln!("[market/bullish][{}] connected", exch_sym);

    // Subscribe to l2Orderbook (full depth) and l1Orderbook (best bid/ask + ticker)
    for topic in &["l2Orderbook", "l1Orderbook"] {
        let sub = json!({
            "jsonrpc": "2.0",
            "type": "command",
            "method": "subscribe",
            "params": { "topic": topic, "symbol": exch_sym },
            "id": get_id()
        });
        write.send(Message::Text(sub.to_string().into())).await.map_err(|e| e.to_string())?;
    }

    let mut bids: BookSide = BTreeMap::new();
    let mut asks: BookSide = BTreeMap::new();
    let mut last_book_emit: i64 = 0;

    // Keepalive every 4 minutes (server requires at least every 5)
    let mut keepalive = tokio::time::interval(Duration::from_secs(240));
    keepalive.tick().await;

    loop {
        tokio::select! {
            _ = keepalive.tick() => {
                let ping = json!({
                    "jsonrpc": "2.0",
                    "type": "command",
                    "method": "keepalivePing",
                    "id": get_id()
                });
                write.send(Message::Text(ping.to_string().into())).await
                    .map_err(|e| e.to_string())?;
            }
            _ = async { cmd_rx.recv().await } => return Ok(()),
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        handle_msg(app, exch_sym, symbol, &t, &mut bids, &mut asks, &mut last_book_emit, emit_interval_ms);
                    }
                    Some(Ok(Message::Ping(d))) => { let _ = write.send(Message::Pong(d)).await; }
                    None | Some(Err(_)) => return Err("disconnected".into()),
                    _ => {}
                }
            }
        }
    }
}

fn handle_msg(
    app: &AppHandle,
    exch_sym: &str,
    symbol: &str,
    text: &str,
    bids: &mut BookSide,
    asks: &mut BookSide,
    last_book_emit: &mut i64,
    emit_interval_ms: u32,
) {
    let v: Value = match serde_json::from_str(text) { Ok(v) => v, Err(_) => return };
    let topic = v["topic"].as_str().unwrap_or("");

    match topic {
        "l2Orderbook" => {
            handle_l2(&v, bids, asks);
            let now = now_ms();
            if now - *last_book_emit >= emit_interval_ms as i64 {
                *last_book_emit = now;
                emit_book(app, exch_sym, symbol, bids, asks, now);
            }
        }
        "l1Orderbook" => {
            emit_ticker(app, exch_sym, symbol, &v);
        }
        _ => {}
    }
}

fn handle_l2(v: &Value, bids: &mut BookSide, asks: &mut BookSide) {
    let data = &v["data"];

    // l2Orderbook sends a snapshot on first message, then incremental updates.
    // Snapshot: `data.bids` and `data.asks` arrays of {price, quantity}.
    // Update:   `data.changes` array of {side:"BID"|"ASK", price, quantity} (quantity 0 = delete).
    if data["bids"].is_array() || data["asks"].is_array() {
        // Snapshot
        bids.clear();
        asks.clear();
        for l in data["bids"].as_array().unwrap_or(&vec![]) {
            if let (Some(p), Some(q)) = (pf(&l["price"]), pf(&l["quantity"])) {
                if q > 0.0 { bids.insert(price_key(p), q); }
            }
        }
        for l in data["asks"].as_array().unwrap_or(&vec![]) {
            if let (Some(p), Some(q)) = (pf(&l["price"]), pf(&l["quantity"])) {
                if q > 0.0 { asks.insert(price_key(p), q); }
            }
        }
    } else if data["changes"].is_array() {
        // Incremental update
        for ch in data["changes"].as_array().unwrap_or(&vec![]) {
            let side = ch["side"].as_str().unwrap_or("");
            let p = match pf(&ch["price"]) { Some(p) => p, None => continue };
            let q = pf(&ch["quantity"]).unwrap_or(0.0);
            let k = price_key(p);
            let book = if side == "BID" { bids as &mut BookSide } else { asks as &mut BookSide };
            if q == 0.0 { book.remove(&k); } else { book.insert(k, q); }
        }
    }
}

fn emit_book(app: &AppHandle, exch_sym: &str, symbol: &str, bids: &BookSide, asks: &BookSide, ts: i64) {
    let top_bids: Vec<[f64; 2]> = bids.iter().rev().take(MAX_LEVELS)
        .map(|(k, q)| [key_price(*k), *q]).collect();
    let top_asks: Vec<[f64; 2]> = asks.iter().take(MAX_LEVELS)
        .map(|(k, q)| [key_price(*k), *q]).collect();

    let _ = app.emit("market://book", &MarketBookEvent {
        symbol:          symbol.to_string(),
        exchange:        "bullish".to_string(),
        exchange_symbol: exch_sym.to_string(),
        bids:            top_bids,
        asks:            top_asks,
        timestamp:       ts,
    });
}

fn emit_ticker(app: &AppHandle, exch_sym: &str, symbol: &str, v: &Value) {
    let data = &v["data"];
    // l1Orderbook fields: bestBid, bestAsk, bestBidQuantity, bestAskQuantity, lastTradedPrice
    let _ = app.emit("market://ticker", &MarketTickerEvent {
        symbol:           symbol.to_string(),
        exchange:         "bullish".to_string(),
        exchange_symbol:  exch_sym.to_string(),
        last:             pf(&data["lastTradedPrice"]).or_else(|| pf(&data["lastPrice"])),
        mark:             pf(&data["bestBid"]),
        index:            None,
        bid:              pf(&data["bestBid"]),
        ask:              pf(&data["bestAsk"]),
        bid_iv:           None,
        ask_iv:           None,
        mark_iv:          None,
        delta:            None,
        gamma:            None,
        vega:             None,
        theta:            None,
        open_interest:    None,
        price_change_24h: None,
        volume_24h:       None,
        high_24h:         None,
        low_24h:          None,
        timestamp:        now_ms(),
    });
}

