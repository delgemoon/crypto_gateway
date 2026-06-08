//! OKX public WS market data task.
//!
//! Subscribes to `books` (full depth, 400-level) and `tickers` channels.
//! OKX uses a push-only subscription model; heartbeat is handled by ping/pong.

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

const WS_URL: &str = "wss://ws.okx.com:8443/ws/v5/public";
const MAX_LEVELS: usize = 50;

type BookMap = BTreeMap<i64, f64>;
fn price_key(p: f64) -> i64 { (p * 1e8).round() as i64 }
fn key_price(k: i64) -> f64 { k as f64 / 1e8 }

pub fn spawn(
    app: AppHandle,
    exchange_symbol: String,
    symbol: String,
    mut cmd_rx: mpsc::Receiver<MarketCmd>,
) {
    tokio::spawn(async move {
        let mut backoff = 1u64;
        loop {
            if cmd_rx.try_recv().is_ok() { break; }
            match run_once(&app, &exchange_symbol, &symbol, &mut cmd_rx).await {
                Ok(()) => break,
                Err(e) => {
                    eprintln!("[market/okx][{}] {}", exchange_symbol, e);
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
    cmd_rx: &mut mpsc::Receiver<MarketCmd>,
) -> Result<(), String> {
    let (ws, _) = connect_async_tls_with_config(WS_URL, None, false, None)
        .await.map_err(|e| e.to_string())?;
    let (mut write, mut read) = ws.split();

    let sub = json!({
        "op": "subscribe",
        "args": [
            { "channel": "books", "instId": exch_sym },
            { "channel": "tickers", "instId": exch_sym },
        ]
    });
    write.send(Message::Text(sub.to_string())).await.map_err(|e| e.to_string())?;

    let mut bids: BookMap = BTreeMap::new();
    let mut asks: BookMap = BTreeMap::new();

    // OKX requires a ping every 30s or the server closes the connection.
    let mut ping_interval = tokio::time::interval(Duration::from_secs(25));
    ping_interval.tick().await; // skip first immediate tick

    loop {
        tokio::select! {
            _ = async { cmd_rx.recv().await } => return Ok(()),
            _ = ping_interval.tick() => {
                let _ = write.send(Message::Text("ping".to_string())).await;
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        if t == "pong" { continue; }
                        handle_msg(app, exch_sym, symbol, &t, &mut bids, &mut asks);
                    }
                    Some(Ok(Message::Ping(d))) => { let _ = write.send(Message::Pong(d)).await; }
                    None | Some(Err(_)) => return Err("disconnected".into()),
                    _ => {}
                }
            }
        }
    }
}

fn apply_okx_levels(book: &mut BookMap, arr: &Value, snapshot: bool) {
    if snapshot { book.clear(); }
    if let Some(levels) = arr.as_array() {
        for lv in levels {
            // OKX format: ["price", "size", "0", "count"]
            let price = lv[0].as_str().and_then(|s| s.parse::<f64>().ok())
                .or_else(|| lv[0].as_f64());
            let size  = lv[1].as_str().and_then(|s| s.parse::<f64>().ok())
                .or_else(|| lv[1].as_f64());
            if let (Some(p), Some(s)) = (price, size) {
                let k = price_key(p);
                if s == 0.0 { book.remove(&k); } else { book.insert(k, s); }
            }
        }
    }
}

fn handle_msg(
    app: &AppHandle,
    exch_sym: &str,
    symbol: &str,
    text: &str,
    bids: &mut BookMap,
    asks: &mut BookMap,
) {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v, Err(_) => return,
    };
    let channel = v["arg"]["channel"].as_str().unwrap_or("");
    let action  = v["action"].as_str().unwrap_or("snapshot");
    let ts = now_ms();

    if channel == "books" {
        let is_snapshot = action == "snapshot";
        if let Some(arr) = v["data"].as_array() {
            for entry in arr {
                apply_okx_levels(bids, &entry["bids"], is_snapshot);
                apply_okx_levels(asks, &entry["asks"], is_snapshot);
            }
        }
        let bid_levels: Vec<[f64; 2]> = bids.iter().rev().take(MAX_LEVELS)
            .map(|(&k, &s)| [key_price(k), s]).collect();
        let ask_levels: Vec<[f64; 2]> = asks.iter().take(MAX_LEVELS)
            .map(|(&k, &s)| [key_price(k), s]).collect();

        let _ = app.emit("market://book", MarketBookEvent {
            symbol: symbol.to_string(),
            exchange: "okx".to_string(),
            exchange_symbol: exch_sym.to_string(),
            bids: bid_levels,
            asks: ask_levels,
            timestamp: ts,
        });
    } else if channel == "tickers" {
        if let Some(arr) = v["data"].as_array() {
            for d in arr {
                let pct = d["sodUtc0"].as_str().and_then(|s| s.parse::<f64>().ok());
                let _ = app.emit("market://ticker", MarketTickerEvent {
                    symbol: symbol.to_string(),
                    exchange: "okx".to_string(),
                    exchange_symbol: exch_sym.to_string(),
                    last:  d["last"].as_str().and_then(|s| s.parse().ok()),
                    mark:  d["markPx"].as_str().and_then(|s| s.parse().ok()),
                    index: d["idxPx"].as_str().and_then(|s| s.parse().ok()),
                    bid:   d["bidPx"].as_str().and_then(|s| s.parse().ok()),
                    ask:   d["askPx"].as_str().and_then(|s| s.parse().ok()),
                    bid_iv:  None,
                    ask_iv:  None,
                    mark_iv: None,
                    delta:   None,
                    gamma:   None,
                    vega:    None,
                    theta:   None,
                    open_interest: d["openInterest"].as_str().and_then(|s| s.parse().ok()),
                    price_change_24h: pct,
                    volume_24h: d["vol24h"].as_str().and_then(|s| s.parse().ok()),
                    high_24h:   d["high24h"].as_str().and_then(|s| s.parse().ok()),
                    low_24h:    d["low24h"].as_str().and_then(|s| s.parse().ok()),
                    timestamp: ts,
                });
            }
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}
