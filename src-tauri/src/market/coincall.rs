//! CoInCall public WS market data task.
//!
//! CoInCall requires an authenticated (signed) URL even for public market data.
//! The caller pre-computes the signed URL via `api::coincall::get_ws_url` and
//! passes it in.  Channel routing uses `dt` (data type) codes:
//!   dt:32 — orderbook update
//!   dt:3  — option ticker
//!   dt:30 — future ticker
//!
//! Subscription format:
//!   option   book: {"action":"subscribe","dataType":"orderBook","payload":{"symbol":"BTCUSD"}}
//!   futures  book: {"action":"subscribe","dataType":"futureOrderBook","payload":{"symbol":"BTCUSD"}}
//!   ticker:  {"action":"subscribe","dataType":"quotation","payload":{"symbol":"BTCUSD-14JUN24-50000-C"}}

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

type BookMap = BTreeMap<i64, f64>;
fn price_key(p: f64) -> i64 { (p * 1e8).round() as i64 }
fn key_price(k: i64) -> f64 { k as f64 / 1e8 }

pub fn spawn(
    app: AppHandle,
    exchange_symbol: String,
    symbol: String,
    kind: String,
    ws_url: String,
    mut cmd_rx: mpsc::Receiver<MarketCmd>,
) {
    tokio::spawn(async move {
        let mut backoff = 1u64;
        loop {
            if cmd_rx.try_recv().is_ok() { break; }
            match run_once(&app, &exchange_symbol, &symbol, &kind, &ws_url, &mut cmd_rx).await {
                Ok(()) => break,
                Err(e) => {
                    eprintln!("[market/coincall][{}] {}", exchange_symbol, e);
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
    ws_url: &str,
    cmd_rx: &mut mpsc::Receiver<MarketCmd>,
) -> Result<(), String> {
    let (ws, _) = connect_async_tls_with_config(ws_url, None, false, None)
        .await.map_err(|e| e.to_string())?;
    let (mut write, mut read) = ws.split();

    // CoInCall uses a "base" symbol (strip strike/expiry for perpetuals).
    // For options, the base is e.g. "BTCUSD"; the full symbol is in exchange_symbol.
    let base_sym = {
        let parts: Vec<&str> = exch_sym.split('-').collect();
        parts.first().copied().unwrap_or(exch_sym).to_string()
    };

    let book_data_type = if kind == "option" { "orderBook" } else { "futureOrderBook" };

    let sub_book = json!({
        "action": "subscribe",
        "dataType": book_data_type,
        "payload": { "symbol": base_sym }
    });
    let sub_ticker = json!({
        "action": "subscribe",
        "dataType": "quotation",
        "payload": { "symbol": exch_sym }
    });
    write.send(Message::Text(sub_book.to_string())).await.map_err(|e| e.to_string())?;
    write.send(Message::Text(sub_ticker.to_string())).await.map_err(|e| e.to_string())?;

    let mut bids: BookMap = BTreeMap::new();
    let mut asks: BookMap = BTreeMap::new();

    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    heartbeat.tick().await;

    loop {
        tokio::select! {
            _ = async { cmd_rx.recv().await } => return Ok(()),
            _ = heartbeat.tick() => {
                let _ = write.send(Message::Text(r#"{"action":"heartbeat"}"#.to_string())).await;
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => {
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

fn parse_cc_price(v: &Value) -> Option<f64> {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_f64())
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
    let dt = match v["dt"].as_i64() { Some(d) => d, None => return };
    let data = &v["d"];
    let ts = now_ms();

    if dt == 32 {
        // Orderbook snapshot: {"dt":32,"d":{"bids":[{"pr":"price","sz":"size"}...],"asks":[...]}}
        bids.clear();
        asks.clear();
        let apply = |book: &mut BookMap, arr: &Value| {
            if let Some(levels) = arr.as_array() {
                for lv in levels {
                    let p = parse_cc_price(&lv["pr"]);
                    let s = parse_cc_price(&lv["sz"]);
                    if let (Some(p), Some(s)) = (p, s) {
                        let k = price_key(p);
                        if s == 0.0 { book.remove(&k); } else { book.insert(k, s); }
                    }
                }
            }
        };
        apply(bids, &data["bids"]);
        apply(asks, &data["asks"]);

        let bid_levels: Vec<[f64; 2]> = bids.iter().rev().take(MAX_LEVELS)
            .map(|(&k, &s)| [key_price(k), s]).collect();
        let ask_levels: Vec<[f64; 2]> = asks.iter().take(MAX_LEVELS)
            .map(|(&k, &s)| [key_price(k), s]).collect();

        let _ = app.emit("market://book", MarketBookEvent {
            symbol: symbol.to_string(),
            exchange: "coincall".to_string(),
            exchange_symbol: exch_sym.to_string(),
            bids: bid_levels,
            asks: ask_levels,
            timestamp: ts,
        });
    } else if dt == 3 {
        // Option ticker
        let _ = app.emit("market://ticker", MarketTickerEvent {
            symbol: symbol.to_string(),
            exchange: "coincall".to_string(),
            exchange_symbol: exch_sym.to_string(),
            last:    parse_cc_price(&data["lp"]),
            mark:    parse_cc_price(&data["mp"]),
            index:   parse_cc_price(&data["ip"]),
            bid:     parse_cc_price(&data["bp"]),
            ask:     parse_cc_price(&data["ap"]),
            bid_iv:  parse_cc_price(&data["biv"]).map(|v| v * 100.0),
            ask_iv:  parse_cc_price(&data["aiv"]).map(|v| v * 100.0),
            mark_iv: parse_cc_price(&data["miv"]).map(|v| v * 100.0),
            delta:   parse_cc_price(&data["dt"]),
            gamma:   parse_cc_price(&data["ga"]),
            vega:    parse_cc_price(&data["ve"]),
            theta:   parse_cc_price(&data["th"]),
            open_interest: parse_cc_price(&data["oi"]),
            price_change_24h: None,
            volume_24h: parse_cc_price(&data["v24"]),
            high_24h:   parse_cc_price(&data["h24"]),
            low_24h:    parse_cc_price(&data["l24"]),
            timestamp: ts,
        });
    } else if dt == 30 {
        // Future ticker
        let _ = app.emit("market://ticker", MarketTickerEvent {
            symbol: symbol.to_string(),
            exchange: "coincall".to_string(),
            exchange_symbol: exch_sym.to_string(),
            last:    parse_cc_price(&data["lp"]),
            mark:    parse_cc_price(&data["mp"]),
            index:   parse_cc_price(&data["ip"]),
            bid:     parse_cc_price(&data["bp"]),
            ask:     parse_cc_price(&data["ap"]),
            bid_iv:  None,
            ask_iv:  None,
            mark_iv: None,
            delta:   None,
            gamma:   None,
            vega:    None,
            theta:   None,
            open_interest: parse_cc_price(&data["oi"]),
            price_change_24h: None,
            volume_24h: parse_cc_price(&data["v24"]),
            high_24h:   parse_cc_price(&data["h24"]),
            low_24h:    parse_cc_price(&data["l24"]),
            timestamp: ts,
        });
    }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}
