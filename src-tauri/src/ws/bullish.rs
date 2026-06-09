//! Bullish Private WebSocket
//!
//! Private WS endpoint: `wss://api.exchange.bullish.com/trading-api/v1/private-data?tradingAccountId={id}`
//! Auth: JWT token passed as a cookie `JWT_COOKIE={jwt}` in the HTTP upgrade request.
//!
//! Subscribe:
//!   `{"jsonrpc":"2.0","type":"command","method":"subscribe","params":{"topic":"spotAccounts"},"id":"..."}`
//!   `{"jsonrpc":"2.0","type":"command","method":"subscribe","params":{"topic":"orders"},"id":"..."}`
//!
//! Keepalive every ~5s:
//!   `{"jsonrpc":"2.0","type":"command","method":"keepalivePing","params":{},"id":"..."}`
//!
//! Messages arrive as:
//!   `{"topic":"orders","type":"data","data":{...},"jsonrpc":"2.0"}`
//!   `{"topic":"spotAccounts","type":"data","data":{...},"jsonrpc":"2.0"}`

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::{
    connect_async_tls_with_config,
    tungstenite::Message,
    tungstenite::http::Request,
};

use crate::api::bullish::{BASE, get_jwt_pub};
use crate::ws::{
    WsCommand, WsConnectionEvent, WsHandle, WsManager, WsOrderUpdate,
    WsTradeUpdate, WsStatus,
};

const WS_BASE: &str = "wss://api.exchange.bullish.com";
const RECONNECT_MAX_DELAY_S: u64 = 60;

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

fn get_id() -> String { now_ms().to_string() }

fn pf(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn pf0(v: &Value) -> f64 { pf(v).unwrap_or(0.0) }

fn emit_connection(app: &AppHandle, account_id: &str, status: &str, msg: Option<String>) {
    let _ = app.emit("ws://connection", WsConnectionEvent {
        account_id: account_id.to_string(),
        exchange:   "bullish".to_string(),
        status:     status.to_string(),
        message:    msg,
    });
}

/// Get trading account ID from passphrase field, or fetch from REST.
async fn resolve_trading_account_id(
    api_key: &str,
    api_secret: &str,
    passphrase: &str,
) -> Result<String, String> {
    let trimmed = passphrase.trim();
    if !trimmed.is_empty() { return Ok(trimmed.to_string()); }

    let jwt = get_jwt_pub(api_key, api_secret).await?;
    let resp: Value = reqwest::Client::new()
        .get(format!("{}/trading-api/v1/accounts/trading-accounts", BASE))
        .header("Authorization", format!("Bearer {}", jwt))
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    resp.as_array()
        .and_then(|a| a.first())
        .and_then(|a| a["tradingAccountId"].as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No Bullish trading accounts found".to_string())
}

pub fn spawn(
    app: AppHandle,
    manager: Arc<WsManager>,
    account_id: String,
    api_key: String,
    api_secret: String,
    trading_account_id: String, // from passphrase field, empty = auto-detect
) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<WsCommand>(8);
    manager.register(WsHandle {
        account_id: account_id.clone(),
        exchange:   "bullish".to_string(),
        status:     WsStatus::Connecting,
        cmd_tx,
    });
    tokio::spawn(run_loop(
        app,
        manager,
        account_id,
        api_key,
        api_secret,
        trading_account_id,
        cmd_rx,
    ));
}

async fn run_loop(
    app: AppHandle,
    manager: Arc<WsManager>,
    account_id: String,
    api_key: String,
    api_secret: String,
    passphrase: String,
    mut cmd_rx: mpsc::Receiver<WsCommand>,
) {
    let mut backoff_s: u64 = 1;

    loop {
        if let Ok(WsCommand::Disconnect) = cmd_rx.try_recv() { break; }

        emit_connection(&app, &account_id, "connecting", None);
        manager.set_status(&account_id, WsStatus::Connecting);

        // Resolve trading account ID and get JWT
        let (jwt, taid) = match async {
            let j = get_jwt_pub(&api_key, &api_secret).await?;
            let t = resolve_trading_account_id(&api_key, &api_secret, &passphrase).await?;
            Ok::<_, String>((j, t))
        }.await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[bullish_ws][{}] auth failed: {}", account_id, e);
                emit_connection(&app, &account_id, "error", Some(e));
                manager.set_status(&account_id, WsStatus::Reconnecting { attempt: 1 });
                sleep(Duration::from_secs(backoff_s)).await;
                backoff_s = (backoff_s * 2).min(RECONNECT_MAX_DELAY_S);
                continue;
            }
        };

        let ws_url = format!(
            "{}/trading-api/v1/private-data?tradingAccountId={}",
            WS_BASE, taid
        );

        // Build request with JWT cookie
        let request = match Request::builder()
            .uri(&ws_url)
            .header("Cookie", format!("JWT_COOKIE={}", jwt))
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_ws_key())
            .body(())
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[bullish_ws][{}] request build error: {}", account_id, e);
                sleep(Duration::from_secs(backoff_s)).await;
                backoff_s = (backoff_s * 2).min(RECONNECT_MAX_DELAY_S);
                continue;
            }
        };

        match connect_async_tls_with_config(request, None, false, None).await {
            Err(e) => {
                let msg = format!("Connect failed: {}", e);
                eprintln!("[bullish_ws][{}] {}", account_id, msg);
                emit_connection(&app, &account_id, "error", Some(msg));
                manager.set_status(&account_id, WsStatus::Reconnecting { attempt: 1 });
                sleep(Duration::from_secs(backoff_s)).await;
                backoff_s = (backoff_s * 2).min(RECONNECT_MAX_DELAY_S);
            }
            Ok((ws_stream, _)) => {
                backoff_s = 1;
                eprintln!("[bullish_ws][{}] connected, tradingAccountId={}", account_id, taid);
                emit_connection(&app, &account_id, "connected", None);
                manager.set_status(&account_id, WsStatus::Connected);

                let should_reconnect = handle_session(
                    &app, &account_id, ws_stream, &mut cmd_rx,
                ).await;

                if !should_reconnect { break; }
                emit_connection(&app, &account_id, "reconnecting", None);
                manager.set_status(&account_id, WsStatus::Reconnecting { attempt: 1 });
                sleep(Duration::from_secs(backoff_s)).await;
                backoff_s = (backoff_s * 2).min(RECONNECT_MAX_DELAY_S);
            }
        }
    }

    emit_connection(&app, &account_id, "disconnected", None);
    manager.set_status(&account_id, WsStatus::Disconnected);
    manager.remove(&account_id);
    eprintln!("[bullish_ws][{}] task exited", account_id);
}

async fn handle_session(
    app: &AppHandle,
    account_id: &str,
    ws_stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>
    >,
    cmd_rx: &mut mpsc::Receiver<WsCommand>,
) -> bool {
    let (mut sink, mut stream) = ws_stream.split();

    // Subscribe to orders and account (spot balance) topics
    for topic in &["orders", "spotAccounts"] {
        let sub = json!({
            "jsonrpc": "2.0",
            "type": "command",
            "method": "subscribe",
            "params": { "topic": topic },
            "id": get_id()
        });
        if sink.send(Message::Text(sub.to_string().into())).await.is_err() {
            return true;
        }
    }
    eprintln!("[bullish_ws][{}] subscribed to orders + spotAccounts", account_id);

    // Keepalive every 5 seconds (as shown in official examples)
    let mut keepalive = tokio::time::interval(Duration::from_secs(5));
    keepalive.tick().await;

    loop {
        tokio::select! {
            _ = keepalive.tick() => {
                let ping = json!({
                    "jsonrpc": "2.0",
                    "type": "command",
                    "method": "keepalivePing",
                    "params": {},
                    "id": get_id()
                });
                if sink.send(Message::Text(ping.to_string().into())).await.is_err() {
                    return true;
                }
            }
            msg = stream.next() => {
                match msg {
                    None | Some(Err(_)) => return true,
                    Some(Ok(Message::Text(text))) => {
                        let v: Value = match serde_json::from_str(&text) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        dispatch_message(app, account_id, &v);
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = sink.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Close(_))) => return true,
                    _ => {}
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(WsCommand::Disconnect) | None => {
                        let _ = sink.send(Message::Close(None)).await;
                        return false;
                    }
                }
            }
        }
    }
}

fn dispatch_message(app: &AppHandle, account_id: &str, v: &Value) {
    let topic = v["topic"].as_str().unwrap_or("");
    let data  = &v["data"];

    match topic {
        "orders" => {
            // Can be a single order object or array
            if let Some(arr) = data.as_array() {
                for o in arr {
                    if let Some(order) = parse_order_update(account_id, o) {
                        let _ = app.emit("ws://order_update", &order);
                    }
                }
            } else if data.is_object() {
                if let Some(order) = parse_order_update(account_id, data) {
                    let _ = app.emit("ws://order_update", &order);
                }
            }
        }
        // spotAccounts = balance updates — no standard ws event type for this, skip for now
        _ => {}
    }
}

fn parse_order_update(account_id: &str, o: &Value) -> Option<WsOrderUpdate> {
    let order_id = o["orderId"].as_str().unwrap_or("").to_string();
    if order_id.is_empty() { return None; }

    let status_raw = o["status"].as_str().unwrap_or("");
    let order_state = match status_raw {
        "OPEN" | "PENDING_NEW" | "PARTIALLY_FILLED" => "open",
        "FILLED"                                     => "filled",
        "CANCELED" | "CANCELLED" | "EXPIRED" | "REJECTED" => "cancelled",
        other => other,
    };

    let post_only = o["timeInForce"].as_str().unwrap_or("") == "POST_ONLY";
    let tif = match o["timeInForce"].as_str().unwrap_or("GTC") {
        "IOC"       => "immediate_or_cancel",
        "FOK"       => "fill_or_kill",
        "POST_ONLY" => "post_only",
        _           => "good_til_cancelled",
    };
    let order_type = match o["type"].as_str().unwrap_or("LMT") {
        "MKT" => "market", "STOP_LIMIT" => "stop_limit", _ => "limit",
    };
    let ts: i64 = o["updatedAtTimestamp"].as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| o["createdAtTimestamp"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or_else(|| now_ms());

    let client_order_id = o["clientOrderId"].as_str().map(|s| s.to_string());

    Some(WsOrderUpdate {
        account_id:      account_id.to_string(),
        exchange:        "bullish".to_string(),
        order_id,
        instrument_name: o["symbol"].as_str().unwrap_or("").to_string(),
        direction:       match o["side"].as_str().unwrap_or("BUY") { "SELL" => "sell", _ => "buy" }.to_string(),
        order_type:      order_type.to_string(),
        order_state:     order_state.to_string(),
        price:           pf(&o["price"]),
        amount:          pf0(&o["quantity"]),
        filled_amount:   pf0(&o["cumulativeQuantity"]),
        time_in_force:   tif.to_string(),
        label:           client_order_id.clone(),
        client_order_id,
        timestamp:       ts,
    })
}

/// Generate a random base64 WebSocket key for the upgrade handshake.
fn generate_ws_key() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let bytes = ts.to_le_bytes();
    // Simple base64-like encoding — tokio-tungstenite accepts any valid base64
    base64_encode(&bytes[..16.min(bytes.len())])
}

fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < input.len() {
        let b0 = input[i] as u32;
        let b1 = if i + 1 < input.len() { input[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < input.len() { input[i + 2] as u32 } else { 0 };
        out.push(CHARS[((b0 >> 2) & 0x3f) as usize] as char);
        out.push(CHARS[(((b0 << 4) | (b1 >> 4)) & 0x3f) as usize] as char);
        out.push(if i + 1 < input.len() { CHARS[(((b1 << 2) | (b2 >> 6)) & 0x3f) as usize] as char } else { '=' });
        out.push(if i + 2 < input.len() { CHARS[(b2 & 0x3f) as usize] as char } else { '=' });
        i += 3;
    }
    out
}
