//! Deribit public WS market data task.
//!
//! Subscribes to `book.<inst>.100ms` and `ticker.<inst>.100ms` and emits
//! `market://book` / `market://ticker` Tauri events.

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

const WS_URL: &str = "wss://www.deribit.com/ws/api/v2";
const MAX_LEVELS: usize = 50;

/// Ordered book: integer key = (price * 1e8).round() for stable float comparison.
type BookMap = BTreeMap<i64, f64>;

fn price_key(p: f64) -> i64 { (p * 1e8).round() as i64 }
fn key_price(k: i64) -> f64 { k as f64 / 1e8 }

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
                    eprintln!("[market/deribit][{}] {}", exchange_symbol, e);
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
    let (ws, _) = connect_async_tls_with_config(WS_URL, None, false, None)
        .await.map_err(|e| e.to_string())?;
    let (mut write, mut read) = ws.split();

    let sub = json!({
        "jsonrpc": "2.0", "id": 1, "method": "public/subscribe",
        "params": { "channels": [
            format!("book.{}.100ms", exch_sym),
            format!("ticker.{}.100ms", exch_sym),
        ]}
    });
    write.send(Message::Text(sub.to_string())).await.map_err(|e| e.to_string())?;

    let mut bids: BookMap = BTreeMap::new();
    let mut asks: BookMap = BTreeMap::new();
    let mut last_book_emit: i64 = 0;

    loop {
        tokio::select! {
            _ = async { cmd_rx.recv().await } => return Ok(()),
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => handle_msg(app, exch_sym, symbol, &t, &mut bids, &mut asks, &mut last_book_emit, emit_interval_ms),
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
    bids: &mut BookMap,
    asks: &mut BookMap,
    last_book_emit: &mut i64,
    emit_interval_ms: u32,
) {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v, Err(_) => return,
    };
    if v["method"].as_str() != Some("subscription") { return; }
    let channel = v["params"]["channel"].as_str().unwrap_or("");
    let data = &v["params"]["data"];
    let ts = now_ms();

    if channel.starts_with("book.") {
        let is_snapshot = data["type"].as_str().unwrap_or("") == "snapshot";

        let apply = |book: &mut BookMap, arr: &Value, snapshot: bool| {
            if snapshot { book.clear(); }
            if let Some(levels) = arr.as_array() {
                for lv in levels {
                    // Deribit format: ["new"|"change"|"delete", price, size]
                    let action = lv[0].as_str().unwrap_or("new");
                    let price  = match lv[1].as_f64() { Some(p) => p, None => continue };
                    let size   = lv[2].as_f64().unwrap_or(0.0);
                    let k = price_key(price);
                    if action == "delete" || size == 0.0 {
                        book.remove(&k);
                    } else {
                        book.insert(k, size);
                    }
                }
            }
        };

        apply(bids, &data["bids"], is_snapshot);
        apply(asks, &data["asks"], is_snapshot);

        // Throttle: configurable emit interval
        let now = now_ms();
        if now - *last_book_emit < emit_interval_ms as i64 { return; }
        *last_book_emit = now;

        let bid_levels: Vec<[f64; 2]> = bids.iter().rev().take(MAX_LEVELS)
            .map(|(&k, &s)| [key_price(k), s]).collect();
        let ask_levels: Vec<[f64; 2]> = asks.iter().take(MAX_LEVELS)
            .map(|(&k, &s)| [key_price(k), s]).collect();

        let _ = app.emit("market://book", MarketBookEvent {
            symbol: symbol.to_string(),
            exchange: "deribit".to_string(),
            exchange_symbol: exch_sym.to_string(),
            bids: bid_levels,
            asks: ask_levels,
            timestamp: ts,
        });
    } else if channel.starts_with("ticker.") {
        let _ = app.emit("market://ticker", MarketTickerEvent {
            symbol: symbol.to_string(),
            exchange: "deribit".to_string(),
            exchange_symbol: exch_sym.to_string(),
            last:            data["last_price"].as_f64(),
            mark:            data["mark_price"].as_f64(),
            index:           data["index_price"].as_f64(),
            bid:             data["best_bid_price"].as_f64(),
            ask:             data["best_ask_price"].as_f64(),
            bid_iv:          data["bid_iv"].as_f64(),
            ask_iv:          data["ask_iv"].as_f64(),
            mark_iv:         data["mark_iv"].as_f64(),
            delta:           data["greeks"]["delta"].as_f64(),
            gamma:           data["greeks"]["gamma"].as_f64(),
            vega:            data["greeks"]["vega"].as_f64(),
            theta:           data["greeks"]["theta"].as_f64(),
            open_interest:   data["open_interest"].as_f64(),
            price_change_24h: data["stats"]["price_change"].as_f64(),
            volume_24h:      data["stats"]["volume"].as_f64(),
            high_24h:        data["stats"]["high"].as_f64(),
            low_24h:         data["stats"]["low"].as_f64(),
            timestamp: ts,
        });
    }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}
