//! CoInCall public WS market data task.
//!
//! CoInCall requires an authenticated (signed) URL even for public market data.
//! The caller pre-computes the signed URL via `api::coincall::get_ws_url` and
//! passes it in.
//!
//! **Options WS** (`wss://ws.coincall.com/options`):
//!   dt:5  — orderbook update  (full symbol subscription)
//!   dt:3  — pricing info / greeks (bsInfo, full symbol subscription)
//!
//! **Futures WS** (`wss://ws.coincall.com/futures`):
//!   dt:32 — orderbook update  (base symbol e.g. "BTCUSD")
//!   dt:30 — index/spot price  (spotPrice, base symbol)
//!
//! Subscription formats:
//!   option  book:   {"action":"subscribe","dataType":"orderBook","payload":{"symbol":"BTCUSD-14JUN24-50000-C"}}
//!   option ticker:  {"action":"subscribe","dataType":"bsInfo","payload":{"symbol":"BTCUSD-14JUN24-50000-C"}}
//!   futures book:   {"action":"subscribe","dataType":"orderBook","payload":{"symbol":"BTCUSD"}}
//!   futures ticker: {"action":"subscribe","dataType":"spotPrice","payload":{"symbol":"BTCUSD"}}

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
    emit_interval_ms: u32,
    mut cmd_rx: mpsc::Receiver<MarketCmd>,
) {
    tokio::spawn(async move {
        let mut backoff = 1u64;
        loop {
            if cmd_rx.try_recv().is_ok() { break; }
            match run_once(&app, &exchange_symbol, &symbol, &kind, &ws_url, emit_interval_ms, &mut cmd_rx).await {
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
    emit_interval_ms: u32,
    cmd_rx: &mut mpsc::Receiver<MarketCmd>,
) -> Result<(), String> {
    eprintln!("[market/coincall][{}] connecting (kind={})", exch_sym, kind);
    let (ws, _) = connect_async_tls_with_config(ws_url, None, false, None)
        .await.map_err(|e| format!("WS connect failed: {}", e))?;
    eprintln!("[market/coincall][{}] connected", exch_sym);
    let (mut write, mut read) = ws.split();

    let is_option = kind == "option";

    // Options subscribe to the full instrument symbol (e.g. "BTCUSD-14JUN24-50000-C").
    // Futures subscribe to the base symbol (e.g. "BTCUSD").
    let (book_data_type, ticker_data_type) = if is_option {
        ("orderBook", "bsInfo")
    } else {
        ("orderBook", "spotPrice")
    };

    let sub_book = json!({
        "action": "subscribe",
        "dataType": book_data_type,
        "payload": { "symbol": exch_sym }
    });
    let sub_ticker = json!({
        "action": "subscribe",
        "dataType": ticker_data_type,
        "payload": { "symbol": exch_sym }
    });
    eprintln!("[market/coincall][{}] subscribing orderBook + {}", exch_sym, ticker_data_type);
    write.send(Message::Text(sub_book.to_string())).await.map_err(|e| e.to_string())?;
    write.send(Message::Text(sub_ticker.to_string())).await.map_err(|e| e.to_string())?;

    // dt codes differ between options and futures WS endpoints.
    let book_dt:   i64 = if is_option { 5 } else { 32 };
    let ticker_dt: i64 = if is_option { 3 } else { 30 };

    let mut bids: BookMap = BTreeMap::new();
    let mut asks: BookMap = BTreeMap::new();
    let mut last_book_emit: i64 = 0;

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
                        handle_msg(app, exch_sym, symbol, is_option, book_dt, ticker_dt,
                                   &t, &mut bids, &mut asks, &mut last_book_emit, emit_interval_ms);
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
    // CoInCall sometimes sends values as strings (with possible trailing spaces).
    v.as_str().and_then(|s| s.trim().parse().ok()).or_else(|| v.as_f64())
}

#[allow(clippy::too_many_arguments)]
fn handle_msg(
    app: &AppHandle,
    exch_sym: &str,
    symbol: &str,
    is_option: bool,
    book_dt: i64,
    ticker_dt: i64,
    text: &str,
    bids: &mut BookMap,
    asks: &mut BookMap,
    last_book_emit: &mut i64,
    emit_interval_ms: u32,
) {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v, Err(_) => return,
    };

    // Handle subscription confirmations and error responses from CoInCall.
    // Response code: "rc":1 = success, other values = error.
    if let Some(rc) = v["rc"].as_i64() {
        if rc != 1 {
            eprintln!("[market/coincall][{}] server error response: {}", exch_sym, text);
        } else {
            eprintln!("[market/coincall][{}] subscription confirmed: rc={}", exch_sym, rc);
        }
        return;
    }

    let dt = match v["dt"].as_i64() { Some(d) => d, None => return };
    let data = &v["d"];
    let ts = now_ms();

    if dt == book_dt {
        // Orderbook snapshot: {"d":{"s":"...","bids":[{"pr":"price","sz":"size"}...],"asks":[...]}}
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

        let now = now_ms();
        if now - *last_book_emit < emit_interval_ms as i64 { return; }
        *last_book_emit = now;

        let bid_levels: Vec<[f64; 2]> = bids.iter().rev().take(MAX_LEVELS)
            .map(|(&k, &s)| [key_price(k), s]).collect();
        let ask_levels: Vec<[f64; 2]> = asks.iter().take(MAX_LEVELS)
            .map(|(&k, &s)| [key_price(k), s]).collect();

        eprintln!("[market/coincall][{}] emitting book: {} bids, {} asks",
            exch_sym, bid_levels.len(), ask_levels.len());

        let _ = app.emit("market://book", MarketBookEvent {
            symbol: symbol.to_string(),
            exchange: "coincall".to_string(),
            exchange_symbol: exch_sym.to_string(),
            bids: bid_levels,
            asks: ask_levels,
            timestamp: ts,
        });
    } else if dt == ticker_dt {
        // Options: bsInfo (dt:3) — pricing / greeks
        // Futures: spotPrice (dt:30) — index price data
        //
        // Field mapping (both use same abbreviation table):
        //   lp/pr = last price (options use "lp", futures use "pr")
        //   mp = mark price,  ip = index price
        //   bid/ask = best bid/ask price (options bsInfo only)
        //   biv/aiv = bid/ask IV (options only)
        //   iv = mark IV (options only)
        //   delta/gamma/vega/theta = greeks (options only)
        //   h = price24hHigh,  l = price24hLow,  v24 = volume24h,  oi = open interest
        let last  = if is_option { parse_cc_price(&data["lp"]) } else { parse_cc_price(&data["pr"]) };
        let _ = app.emit("market://ticker", MarketTickerEvent {
            symbol: symbol.to_string(),
            exchange: "coincall".to_string(),
            exchange_symbol: exch_sym.to_string(),
            last,
            mark:    parse_cc_price(&data["mp"]),
            index:   parse_cc_price(&data["ip"]),
            bid:     parse_cc_price(&data["bid"]),
            ask:     parse_cc_price(&data["ask"]),
            bid_iv:  if is_option { parse_cc_price(&data["biv"]).map(|v| v * 100.0) } else { None },
            ask_iv:  if is_option { parse_cc_price(&data["aiv"]).map(|v| v * 100.0) } else { None },
            mark_iv: if is_option { parse_cc_price(&data["iv"]).map(|v| v * 100.0) } else { None },
            delta:   if is_option { parse_cc_price(&data["delta"]) } else { None },
            gamma:   if is_option { parse_cc_price(&data["gamma"]) } else { None },
            vega:    if is_option { parse_cc_price(&data["vega"]) } else { None },
            theta:   if is_option { parse_cc_price(&data["theta"]) } else { None },
            open_interest:    parse_cc_price(&data["oi"]),
            price_change_24h: parse_cc_price(&data["cr"]),
            volume_24h:       parse_cc_price(&data["v24"]),
            high_24h:         parse_cc_price(&data["h"]),
            low_24h:          parse_cc_price(&data["l"]),
            timestamp: ts,
        });
    }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

