//! Bybit public WS market data task.
//!
//! Routes to the correct WS endpoint based on instrument kind:
//!   option  → wss://stream.bybit.com/v5/public/option
//!   spot    → wss://stream.bybit.com/v5/public/spot
//!   inverse → wss://stream.bybit.com/v5/public/inverse  (USD-settled futures)
//!   linear  → wss://stream.bybit.com/v5/public/linear   (USDT-settled futures)
//!
//! Subscribes to `orderbook.50.<sym>` and `tickers.<sym>`.

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

const MAX_LEVELS: usize = 50;

fn ws_url(exchange_symbol: &str, kind: &str) -> &'static str {
    if kind == "option" {
        return "wss://stream.bybit.com/v5/public/option";
    }
    if kind == "spot" {
        return "wss://stream.bybit.com/v5/public/spot";
    }
    // Inverse: BTC-USD or ETH-USD (perpetual/future without USDT)
    if exchange_symbol.ends_with("USD") || exchange_symbol.contains("-USD-") {
        return "wss://stream.bybit.com/v5/public/inverse";
    }
    "wss://stream.bybit.com/v5/public/linear"
}

type BookMap = BTreeMap<i64, f64>;
fn price_key(p: f64) -> i64 { (p * 1e8).round() as i64 }
fn key_price(k: i64) -> f64 { k as f64 / 1e8 }

pub fn spawn(
    app: AppHandle,
    exchange_symbol: String,
    symbol: String,
    kind: String,
    emit_interval_ms: u32,
    mut cmd_rx: mpsc::Receiver<MarketCmd>,
) {
    tokio::spawn(async move {
        let mut backoff = 1u64;
        loop {
            if cmd_rx.try_recv().is_ok() { break; }
            match run_once(&app, &exchange_symbol, &symbol, &kind, emit_interval_ms, &mut cmd_rx).await {
                Ok(()) => break,
                Err(e) => {
                    eprintln!("[market/bybit][{}] {}", exchange_symbol, e);
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
    kind: &str,
    emit_interval_ms: u32,
    cmd_rx: &mut mpsc::Receiver<MarketCmd>,
) -> Result<(), String> {
    let url = ws_url(exch_sym, kind);
    let (ws, _) = connect_async_tls_with_config(url, None, false, None)
        .await.map_err(|e| e.to_string())?;
    let (mut write, mut read) = ws.split();

    let sub = json!({
        "op": "subscribe",
        "args": [
            format!("orderbook.50.{}", exch_sym),
            format!("tickers.{}", exch_sym),
        ]
    });
    write.send(Message::Text(sub.to_string())).await.map_err(|e| e.to_string())?;

    let mut bids: BookMap = BTreeMap::new();
    let mut asks: BookMap = BTreeMap::new();
    let mut last_book_emit: i64 = 0;

    // Bybit requires a ping every 20s.
    let mut ping_interval = tokio::time::interval(Duration::from_secs(20));
    ping_interval.tick().await;

    loop {
        tokio::select! {
            _ = async { cmd_rx.recv().await } => return Ok(()),
            _ = ping_interval.tick() => {
                let _ = write.send(Message::Text(r#"{"op":"ping"}"#.to_string())).await;
            }
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

fn parse_level(lv: &Value) -> Option<(f64, f64)> {
    // Bybit format: ["price_str", "size_str"]
    let p = lv[0].as_str().and_then(|s| s.parse::<f64>().ok())
        .or_else(|| lv[0].as_f64())?;
    let s = lv[1].as_str().and_then(|s| s.parse::<f64>().ok())
        .or_else(|| lv[1].as_f64())?;
    Some((p, s))
}

fn apply_bybit_levels(book: &mut BookMap, arr: &Value, snapshot: bool) {
    if snapshot { book.clear(); }
    if let Some(levels) = arr.as_array() {
        for lv in levels {
            if let Some((p, s)) = parse_level(lv) {
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
    last_book_emit: &mut i64,
    emit_interval_ms: u32,
) {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v, Err(_) => return,
    };
    let topic = v["topic"].as_str().unwrap_or("");
    let msg_type = v["type"].as_str().unwrap_or("delta");
    let ts = v["ts"].as_i64().unwrap_or_else(now_ms);

    if topic.starts_with("orderbook.") {
        let data = &v["data"];
        let is_snapshot = msg_type == "snapshot";
        apply_bybit_levels(bids, &data["b"], is_snapshot);
        apply_bybit_levels(asks, &data["a"], is_snapshot);

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
            exchange: "bybit".to_string(),
            exchange_symbol: exch_sym.to_string(),
            bids: bid_levels,
            asks: ask_levels,
            timestamp: ts,
        });
    } else if topic.starts_with("tickers.") {
        let d = &v["data"];
        let parse = |key: &str| -> Option<f64> {
            d[key].as_str().and_then(|s| s.parse().ok()).or_else(|| d[key].as_f64())
        };
        let pct = parse("price24hPcnt").map(|v| v * 100.0);
        let _ = app.emit("market://ticker", MarketTickerEvent {
            symbol: symbol.to_string(),
            exchange: "bybit".to_string(),
            exchange_symbol: exch_sym.to_string(),
            last:  parse("lastPrice"),
            mark:  parse("markPrice"),
            index: parse("indexPrice"),
            bid:   parse("bid1Price"),
            ask:   parse("ask1Price"),
            bid_iv:  None,
            ask_iv:  None,
            mark_iv: parse("markIv").or_else(|| parse("iv")),
            delta:   None,
            gamma:   None,
            vega:    None,
            theta:   None,
            open_interest: parse("openInterest"),
            price_change_24h: pct,
            volume_24h: parse("volume24h"),
            high_24h:   parse("highPrice24h"),
            low_24h:    parse("lowPrice24h"),
            timestamp: ts,
        });
    }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}
