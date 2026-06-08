/// Public orderbook WS feeds for the aggregated book.
///
/// Each `spawn_*` function:
///   - Takes an instrument name and a `watch::Sender<Option<OrderbookSnapshot>>`
///   - Spawns a background task with reconnect loop
///   - Parses snapshots + incremental deltas into `LocalBook`
///   - Sends the updated snapshot to the watch channel after every change
///
/// Supported exchanges (WS):
///   - deribit  : `book.{instrument}.raw` (incremental)
///   - okx      : `books` channel (snapshot + delta)
///   - bybit    : `orderbook.50.{symbol}` (snapshot + delta)
///   - binance  : `{symbol}@depth20@100ms` (full snapshots every 100ms, no seq tracking)
///   - hyperliquid : `l2Book` (full snapshots, no incremental)
///
/// Exchanges with REST fallback only: mexc, coincall, uniswap

use std::time::Duration;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::watch;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async_tls_with_config, tungstenite::Message};

use crate::api::models::OrderbookSnapshot;
use super::local_book::LocalBook;

const RECONNECT_MAX_S: u64 = 60;
const BOOK_DEPTH: usize = 50;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Parse [[price_str, size_str], ...] or [[price_f64, size_f64], ...]
fn parse_levels(arr: &Value) -> Vec<(f64, f64)> {
    arr.as_array().unwrap_or(&vec![])
        .iter()
        .filter_map(|entry| {
            let a = entry.as_array()?;
            let price = a.get(0)?.as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .or_else(|| a.get(0)?.as_f64())?;
            let size = a.get(1)?.as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .or_else(|| a.get(1)?.as_f64())?;
            Some((price, size))
        })
        .collect()
}

/// Parse Deribit raw book levels: [["new"|"change"|"delete", price, size], ...]
/// For "delete" entries, returns size=0.
fn parse_deribit_levels(arr: &Value) -> Vec<(f64, f64)> {
    arr.as_array().unwrap_or(&vec![])
        .iter()
        .filter_map(|entry| {
            let a = entry.as_array()?;
            let action = a.get(0)?.as_str().unwrap_or("");
            let price = a.get(1)?.as_f64()?;
            let size = if action == "delete" { 0.0 } else { a.get(2)?.as_f64()? };
            Some((price, size))
        })
        .collect()
}

// ── Deribit ────────────────────────────────────────────────────────────────

pub fn spawn_deribit(
    instrument: String,
    testnet: bool,
    tx: watch::Sender<Option<OrderbookSnapshot>>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let url = if testnet {
            "wss://test.deribit.com/ws/api/v2"
        } else {
            "wss://www.deribit.com/ws/api/v2"
        };
        let channel = format!("book.{}.raw", instrument);
        let mut backoff = 1u64;

        loop {
            match connect_async_tls_with_config(url, None, false, None).await {
                Err(e) => {
                    eprintln!("[agg_book/ws] deribit connect error: {}", e);
                    sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(RECONNECT_MAX_S);
                    continue;
                }
                Ok((ws, _)) => {
                    backoff = 1;
                    let (mut sink, mut stream) = ws.split();
                    let sub = json!({
                        "jsonrpc": "2.0", "id": 1,
                        "method": "public/subscribe",
                        "params": { "channels": [&channel] }
                    });
                    if sink.send(Message::Text(sub.to_string().into())).await.is_err() { continue; }

                    let mut book = LocalBook::new(&instrument);
                    let mut ping_interval = tokio::time::interval(Duration::from_secs(20));
                    ping_interval.tick().await;

                    loop {
                        tokio::select! {
                            _ = ping_interval.tick() => {
                                let ping = json!({"jsonrpc":"2.0","id":99,"method":"public/test","params":{}});
                                if sink.send(Message::Text(ping.to_string().into())).await.is_err() { break; }
                            }
                            msg = stream.next() => {
                                match msg {
                                    None | Some(Err(_)) => break,
                                    Some(Ok(Message::Text(text))) => {
                                        let v: Value = match serde_json::from_str(&text) { Ok(v) => v, Err(_) => continue };
                                        // Subscription notification
                                        if v["method"] == "subscription" {
                                            let data = &v["params"]["data"];
                                            let ts = data["timestamp"].as_i64().unwrap_or_else(now_ms);
                                            let bids = parse_deribit_levels(&data["bids"]);
                                            let asks = parse_deribit_levels(&data["asks"]);
                                            if data["type"] == "snapshot" {
                                                book.apply_snapshot(&bids, &asks, ts);
                                            } else {
                                                book.apply_delta(&bids, &asks, ts);
                                            }
                                            if !book.is_empty() {
                                                let _ = tx.send(Some(book.to_snapshot(BOOK_DEPTH)));
                                            }
                                        }
                                    }
                                    Some(Ok(Message::Close(_))) => break,
                                    _ => {}
                                }
                            }
                        }
                    }
                    eprintln!("[agg_book/ws] deribit {} disconnected, reconnecting…", instrument);
                    sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(RECONNECT_MAX_S);
                }
            }
        }
    })
}

// ── OKX ───────────────────────────────────────────────────────────────────

pub fn spawn_okx(
    instrument: String,
    _testnet: bool,
    tx: watch::Sender<Option<OrderbookSnapshot>>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let url = "wss://ws.okx.com:8443/ws/v5/public";
        let mut backoff = 1u64;

        loop {
            match connect_async_tls_with_config(url, None, false, None).await {
                Err(e) => {
                    eprintln!("[agg_book/ws] okx connect error: {}", e);
                    sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(RECONNECT_MAX_S);
                    continue;
                }
                Ok((ws, _)) => {
                    backoff = 1;
                    let (mut sink, mut stream) = ws.split();
                    let sub = json!({
                        "op": "subscribe",
                        "args": [{ "channel": "books", "instId": &instrument }]
                    });
                    if sink.send(Message::Text(sub.to_string().into())).await.is_err() { continue; }

                    let mut book = LocalBook::new(&instrument);
                    let mut ping_interval = tokio::time::interval(Duration::from_secs(25));
                    ping_interval.tick().await;

                    loop {
                        tokio::select! {
                            _ = ping_interval.tick() => {
                                if sink.send(Message::Text("ping".into())).await.is_err() { break; }
                            }
                            msg = stream.next() => {
                                match msg {
                                    None | Some(Err(_)) => break,
                                    Some(Ok(Message::Text(text))) => {
                                        if text == "pong" { continue; }
                                        let v: Value = match serde_json::from_str(&text) { Ok(v) => v, Err(_) => continue };
                                        let action = v["action"].as_str().unwrap_or("");
                                        if action != "snapshot" && action != "update" { continue; }
                                        let data = &v["data"][0];
                                        let ts = data["ts"].as_str()
                                            .and_then(|s| s.parse::<i64>().ok())
                                            .unwrap_or_else(now_ms);
                                        let bids = parse_levels(&data["bids"]);
                                        let asks = parse_levels(&data["asks"]);
                                        if action == "snapshot" {
                                            book.apply_snapshot(&bids, &asks, ts);
                                        } else {
                                            book.apply_delta(&bids, &asks, ts);
                                        }
                                        if !book.is_empty() {
                                            let _ = tx.send(Some(book.to_snapshot(BOOK_DEPTH)));
                                        }
                                    }
                                    Some(Ok(Message::Close(_))) => break,
                                    _ => {}
                                }
                            }
                        }
                    }
                    eprintln!("[agg_book/ws] okx {} disconnected, reconnecting…", instrument);
                    sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(RECONNECT_MAX_S);
                }
            }
        }
    })
}

// ── Bybit ─────────────────────────────────────────────────────────────────

/// Determine Bybit WS URL from instrument name.
fn bybit_ws_url(instrument: &str) -> &'static str {
    // Options contain a date component: "BTC-28JUN24-60000-C"
    if instrument.contains('-') { return "wss://stream.bybit.com/v5/public/option"; }
    // Inverse perps end in USD (BTCUSD) without a second currency like USDT
    if instrument.ends_with("USD") && !instrument.ends_with("USDT") && !instrument.ends_with("USDC") {
        return "wss://stream.bybit.com/v5/public/inverse";
    }
    // Spot instruments are typically like "BTCUSDT" but appear in spot category;
    // we treat all remaining as linear (perp/futures)
    "wss://stream.bybit.com/v5/public/linear"
}

pub fn spawn_bybit(
    instrument: String,
    tx: watch::Sender<Option<OrderbookSnapshot>>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let url = bybit_ws_url(&instrument);
        let channel = format!("orderbook.50.{}", instrument);
        let mut backoff = 1u64;

        loop {
            match connect_async_tls_with_config(url, None, false, None).await {
                Err(e) => {
                    eprintln!("[agg_book/ws] bybit connect error: {}", e);
                    sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(RECONNECT_MAX_S);
                    continue;
                }
                Ok((ws, _)) => {
                    backoff = 1;
                    let (mut sink, mut stream) = ws.split();
                    let sub = json!({ "op": "subscribe", "args": [&channel] });
                    if sink.send(Message::Text(sub.to_string().into())).await.is_err() { continue; }

                    let mut book = LocalBook::new(&instrument);
                    let mut ping_interval = tokio::time::interval(Duration::from_secs(20));
                    ping_interval.tick().await;

                    loop {
                        tokio::select! {
                            _ = ping_interval.tick() => {
                                let ping = json!({"op":"ping"});
                                if sink.send(Message::Text(ping.to_string().into())).await.is_err() { break; }
                            }
                            msg = stream.next() => {
                                match msg {
                                    None | Some(Err(_)) => break,
                                    Some(Ok(Message::Text(text))) => {
                                        let v: Value = match serde_json::from_str(&text) { Ok(v) => v, Err(_) => continue };
                                        let msg_type = v["type"].as_str().unwrap_or("");
                                        if msg_type != "snapshot" && msg_type != "delta" { continue; }
                                        let data = &v["data"];
                                        let ts = v["ts"].as_i64().unwrap_or_else(now_ms);
                                        let bids = parse_levels(&data["b"]);
                                        let asks = parse_levels(&data["a"]);
                                        if msg_type == "snapshot" {
                                            book.apply_snapshot(&bids, &asks, ts);
                                        } else {
                                            book.apply_delta(&bids, &asks, ts);
                                        }
                                        if !book.is_empty() {
                                            let _ = tx.send(Some(book.to_snapshot(BOOK_DEPTH)));
                                        }
                                    }
                                    Some(Ok(Message::Close(_))) => break,
                                    _ => {}
                                }
                            }
                        }
                    }
                    eprintln!("[agg_book/ws] bybit {} disconnected, reconnecting…", instrument);
                    sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(RECONNECT_MAX_S);
                }
            }
        }
    })
}

// ── Binance ───────────────────────────────────────────────────────────────

/// Binance: use `@depth20@100ms` which sends full 20-level snapshots — avoids
/// sequence-tracking complexity of incremental depth streams.
fn binance_ws_url(instrument: &str) -> String {
    let sym = instrument.to_lowercase();
    // Coin-margined futures: symbol like "BTCUSD_PERP" or "BTCUSD_241227"
    if sym.contains("usd_") {
        return format!("wss://dstream.binance.com/ws/{}@depth20@100ms", sym);
    }
    // USDT-margined futures: symbol like "BTCUSDT" (without spot indicators)
    // We treat all agg-book instruments as futures; spot is rarely used here.
    format!("wss://fstream.binance.com/ws/{}@depth20@100ms", sym)
}

pub fn spawn_binance(
    instrument: String,
    tx: watch::Sender<Option<OrderbookSnapshot>>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let url = binance_ws_url(&instrument);
        let mut backoff = 1u64;

        loop {
            match connect_async_tls_with_config(url.as_str(), None, false, None).await {
                Err(e) => {
                    eprintln!("[agg_book/ws] binance connect error: {}", e);
                    sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(RECONNECT_MAX_S);
                    continue;
                }
                Ok((ws, _)) => {
                    backoff = 1;
                    // Binance depth20 stream sends frames automatically — no subscribe needed
                    let (mut sink, mut stream) = ws.split();
                    let mut book = LocalBook::new(&instrument);
                    let mut ping_interval = tokio::time::interval(Duration::from_secs(30));
                    ping_interval.tick().await;

                    loop {
                        tokio::select! {
                            _ = ping_interval.tick() => {
                                // Binance expects pong in response to ping frames
                                if sink.send(Message::Ping(vec![].into())).await.is_err() { break; }
                            }
                            msg = stream.next() => {
                                match msg {
                                    None | Some(Err(_)) => break,
                                    Some(Ok(Message::Ping(payload))) => {
                                        let _ = sink.send(Message::Pong(payload)).await;
                                    }
                                    Some(Ok(Message::Text(text))) => {
                                        let v: Value = match serde_json::from_str(&text) { Ok(v) => v, Err(_) => continue };
                                        // depth20 full snapshot: {"lastUpdateId":..., "bids":[[p,q],...], "asks":[[p,q],...]}
                                        let bids = parse_levels(&v["bids"]);
                                        let asks = parse_levels(&v["asks"]);
                                        if !bids.is_empty() || !asks.is_empty() {
                                            book.apply_snapshot(&bids, &asks, now_ms());
                                            let _ = tx.send(Some(book.to_snapshot(BOOK_DEPTH)));
                                        }
                                    }
                                    Some(Ok(Message::Close(_))) => break,
                                    _ => {}
                                }
                            }
                        }
                    }
                    eprintln!("[agg_book/ws] binance {} disconnected, reconnecting…", instrument);
                    sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(RECONNECT_MAX_S);
                }
            }
        }
    })
}

// ── Hyperliquid ───────────────────────────────────────────────────────────

/// Hyperliquid sends full l2Book snapshots (no incremental).
pub fn spawn_hyperliquid(
    instrument: String,
    tx: watch::Sender<Option<OrderbookSnapshot>>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let url = "wss://api.hyperliquid.xyz/ws";
        // Hyperliquid coin = base currency only (e.g. "BTC" not "BTC-PERP")
        let coin = instrument.split('-').next().unwrap_or(&instrument).to_uppercase();
        let coin_owned = coin.clone();
        let mut backoff = 1u64;

        loop {
            match connect_async_tls_with_config(url, None, false, None).await {
                Err(e) => {
                    eprintln!("[agg_book/ws] hyperliquid connect error: {}", e);
                    sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(RECONNECT_MAX_S);
                    continue;
                }
                Ok((ws, _)) => {
                    backoff = 1;
                    let (mut sink, mut stream) = ws.split();
                    let sub = json!({
                        "method": "subscribe",
                        "subscription": { "type": "l2Book", "coin": &coin_owned }
                    });
                    if sink.send(Message::Text(sub.to_string().into())).await.is_err() { continue; }

                    let mut book = LocalBook::new(&instrument);

                    loop {
                        match stream.next().await {
                            None | Some(Err(_)) => break,
                            Some(Ok(Message::Text(text))) => {
                                let v: Value = match serde_json::from_str(&text) { Ok(v) => v, Err(_) => continue };
                                if v["channel"] == "l2Book" {
                                    let data = &v["data"];
                                    let ts = data["time"].as_i64().unwrap_or_else(now_ms);
                                    // levels: {"coin":"BTC","levels":[[{px,sz,n},...],[{px,sz,n},...]]}}
                                    // levels[0] = bids (desc), levels[1] = asks (asc)
                                    let parse_hl = |arr: &Value| -> Vec<(f64, f64)> {
                                        arr.as_array().unwrap_or(&vec![])
                                            .iter()
                                            .filter_map(|e| {
                                                let price = e["px"].as_str()?.parse::<f64>().ok()?;
                                                let size  = e["sz"].as_str()?.parse::<f64>().ok()?;
                                                Some((price, size))
                                            })
                                            .collect()
                                    };
                                    let bids = parse_hl(&data["levels"][0]);
                                    let asks = parse_hl(&data["levels"][1]);
                                    book.apply_snapshot(&bids, &asks, ts);
                                    if !book.is_empty() {
                                        let _ = tx.send(Some(book.to_snapshot(BOOK_DEPTH)));
                                    }
                                }
                            }
                            Some(Ok(Message::Close(_))) => break,
                            _ => {}
                        }
                    }
                    eprintln!("[agg_book/ws] hyperliquid {} disconnected, reconnecting…", instrument);
                    sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(RECONNECT_MAX_S);
                }
            }
        }
    })
}
