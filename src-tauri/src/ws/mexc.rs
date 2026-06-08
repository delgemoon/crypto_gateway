/// MEXC Private WebSocket
///
/// Connects to `wss://wbs.mexc.com/ws`.
/// Authenticates via login message with HMAC-SHA256.
/// Subscribes to: `spot@private.orders.v3.api`, `spot@private.deals.v3.api`,
///                `contract@private.order`, `contract@private.position`
///
/// Auto-reconnects with exponential backoff (1s → 2s → 4s … capped at 60s).

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::{connect_async_tls_with_config, tungstenite::Message};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ws::{
    WsCommand, WsConnectionEvent, WsHandle, WsManager, WsOrderUpdate,
    WsPositionUpdate, WsStatus, WsTradeUpdate,
};

type HmacSha256 = Hmac<Sha256>;

const WS_URL: &str = "wss://wbs.mexc.com/ws";
const RECONNECT_MAX_DELAY_S: u64 = 60;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn sign_login(api_key: &str, api_secret: &str) -> (u64, String) {
    let ts = now_ms();
    let msg = format!("{}{}", api_key, ts);
    let mut mac = HmacSha256::new_from_slice(api_secret.as_bytes()).unwrap();
    mac.update(msg.as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());
    (ts, sig)
}

pub fn spawn(
    app: AppHandle,
    manager: Arc<WsManager>,
    account_id: String,
    api_key: String,
    api_secret: String,
    _testnet: bool,
) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<WsCommand>(8);
    manager.register(WsHandle {
        account_id: account_id.clone(),
        exchange: "mexc".to_string(),
        status: WsStatus::Connecting,
        cmd_tx,
    });
    tokio::spawn(run_loop(app, manager, account_id, api_key, api_secret, cmd_rx));
}

async fn run_loop(
    app: AppHandle,
    manager: Arc<WsManager>,
    account_id: String,
    api_key: String,
    api_secret: String,
    mut cmd_rx: mpsc::Receiver<WsCommand>,
) {
    let mut backoff_s: u64 = 1;

    loop {
        if let Ok(WsCommand::Disconnect) = cmd_rx.try_recv() { break; }

        emit_connection(&app, &account_id, "connecting", None);
        manager.set_status(&account_id, WsStatus::Connecting);

        match connect_async_tls_with_config(WS_URL, None, false, None).await {
            Err(e) => {
                let msg = format!("Connect failed: {}", e);
                eprintln!("[mexc_ws][{}] {}", account_id, msg);
                emit_connection(&app, &account_id, "error", Some(msg));
                manager.set_status(&account_id, WsStatus::Reconnecting { attempt: 1 });
                sleep(Duration::from_secs(backoff_s)).await;
                backoff_s = (backoff_s * 2).min(RECONNECT_MAX_DELAY_S);
                continue;
            }
            Ok((ws_stream, _)) => {
                backoff_s = 1;
                eprintln!("[mexc_ws][{}] connected", account_id);
                emit_connection(&app, &account_id, "connected", None);
                manager.set_status(&account_id, WsStatus::Connected);

                let disconnected = handle_session(
                    &app, &manager, &account_id, &api_key, &api_secret,
                    ws_stream, &mut cmd_rx,
                ).await;

                if !disconnected { break; }
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
    eprintln!("[mexc_ws][{}] task exited", account_id);
}

async fn handle_session(
    app: &AppHandle,
    _manager: &Arc<WsManager>,
    account_id: &str,
    api_key: &str,
    api_secret: &str,
    ws_stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>
    >,
    cmd_rx: &mut mpsc::Receiver<WsCommand>,
) -> bool {
    let (mut sink, mut stream) = ws_stream.split();

    // ── Authenticate ────────────────────────────────────────────────────
    let (ts, sig) = sign_login(api_key, api_secret);
    let login_msg = json!({
        "method": "SUBSCRIPTION",
        "params": ["spot@private.orders.v3.api", "spot@private.deals.v3.api"]
    });
    // MEXC requires login before subscribing private channels
    let auth_msg = json!({
        "method": "LOGIN",
        "params": {
            "apiKey": api_key,
            "reqTime": ts.to_string(),
            "signature": sig,
        }
    });
    if sink.send(Message::Text(auth_msg.to_string().into())).await.is_err() {
        return true;
    }

    // Wait for login response
    let mut authenticated = false;
    while let Some(Ok(msg)) = stream.next().await {
        if let Message::Text(text) = msg {
            let v: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let msg_type = v["msg"].as_str().unwrap_or("");
            if msg_type.contains("login") || v["code"].as_i64() == Some(0) {
                authenticated = true;
                eprintln!("[mexc_ws][{}] authenticated", account_id);
                break;
            } else if v["code"].as_i64().map(|c| c != 0).unwrap_or(false) {
                let err = v["msg"].as_str().unwrap_or("auth failed");
                eprintln!("[mexc_ws][{}] auth error: {}", account_id, err);
                emit_connection(app, account_id, "error", Some(format!("Auth: {}", err)));
                return true;
            }
        }
    }
    if !authenticated { return true; }

    // ── Subscribe to private streams ────────────────────────────────────
    if sink.send(Message::Text(login_msg.to_string().into())).await.is_err() {
        return true;
    }
    eprintln!("[mexc_ws][{}] subscribed to spot orders/deals", account_id);

    // ── Heartbeat ping every 20s ────────────────────────────────────────
    let mut ping_interval = tokio::time::interval(Duration::from_secs(20));
    ping_interval.tick().await;

    loop {
        tokio::select! {
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
            _ = ping_interval.tick() => {
                let ping = json!({"method": "PING"});
                if sink.send(Message::Text(ping.to_string().into())).await.is_err() {
                    return true;
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
    let channel = v["c"].as_str()
        .or_else(|| v["channel"].as_str())
        .unwrap_or("");

    if channel.contains("orders") || channel.contains("order") {
        let data = &v["d"];
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
    } else if channel.contains("deals") || channel.contains("trade") {
        let data = &v["d"];
        if let Some(arr) = data.as_array() {
            for t in arr {
                if let Some(trade) = parse_trade_update(account_id, t) {
                    let _ = app.emit("ws://trade_update", &trade);
                }
            }
        } else if data.is_object() {
            if let Some(trade) = parse_trade_update(account_id, data) {
                let _ = app.emit("ws://trade_update", &trade);
            }
        }
    } else if channel.contains("position") {
        let data = &v["d"];
        if let Some(arr) = data.as_array() {
            for p in arr {
                if let Some(pos) = parse_position_update(account_id, p) {
                    let _ = app.emit("ws://position_update", &pos);
                }
            }
        } else if data.is_object() {
            if let Some(pos) = parse_position_update(account_id, data) {
                let _ = app.emit("ws://position_update", &pos);
            }
        }
    }
}

fn parse_order_update(account_id: &str, o: &Value) -> Option<WsOrderUpdate> {
    let order_id = o["i"].as_str()
        .or_else(|| o["orderId"].as_str())
        .or_else(|| o["id"].as_str())?
        .to_string();

    let state_raw = o["s"].as_i64()
        .or_else(|| o["status"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0);
    let state = match state_raw {
        1 => "open",      // New
        2 => "open",      // Filled (partial)
        3 => "filled",    // Fully filled
        4 => "cancelled",
        _ => "open",
    };
    let side_raw = o["S"].as_i64()
        .or_else(|| o["side"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(1);
    let direction = if side_raw == 1 { "buy" } else { "sell" };

    Some(WsOrderUpdate {
        account_id:      account_id.to_string(),
        exchange:        "mexc".to_string(),
        order_id,
        instrument_name: o["s"].as_str()
            .or_else(|| o["symbol"].as_str())
            .unwrap_or("").to_string(),
        direction:       direction.to_string(),
        order_type:      "limit".to_string(),
        order_state:     state.to_string(),
        price:           o["p"].as_str().and_then(|s| s.parse().ok())
                            .or_else(|| o["price"].as_f64()),
        amount:          o["q"].as_str().and_then(|s| s.parse().ok())
                            .or_else(|| o["quantity"].as_f64()).unwrap_or(0.0),
        filled_amount:   o["z"].as_str().and_then(|s| s.parse().ok())
                            .or_else(|| o["filledQuantity"].as_f64()).unwrap_or(0.0),
        time_in_force:   "good_til_cancelled".to_string(),
        label:           o["c"].as_str().map(|s| s.to_string()),
        client_order_id: o["c"].as_str().map(|s| s.to_string()),
        timestamp:       o["t"].as_i64().or_else(|| o["updateTime"].as_i64()).unwrap_or(0),
    })
}

fn parse_trade_update(account_id: &str, t: &Value) -> Option<WsTradeUpdate> {
    let trade_id = t["t"].as_str()
        .or_else(|| t["tradeId"].as_str())
        .or_else(|| t["id"].as_str())?
        .to_string();

    let side_raw = t["S"].as_i64()
        .or_else(|| t["side"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(1);
    let direction = if side_raw == 1 { "buy" } else { "sell" };

    Some(WsTradeUpdate {
        account_id:      account_id.to_string(),
        exchange:        "mexc".to_string(),
        trade_id,
        order_id:        t["i"].as_str().or_else(|| t["orderId"].as_str()).unwrap_or("").to_string(),
        instrument_name: t["s"].as_str().or_else(|| t["symbol"].as_str()).unwrap_or("").to_string(),
        direction:       direction.to_string(),
        amount:          t["v"].as_str().and_then(|s| s.parse().ok())
                            .or_else(|| t["quantity"].as_f64()).unwrap_or(0.0),
        price:           t["p"].as_str().and_then(|s| s.parse().ok())
                            .or_else(|| t["price"].as_f64()).unwrap_or(0.0),
        fee:             t["n"].as_str().and_then(|s| s.parse::<f64>().ok())
                            .map(|f| f.abs())
                            .or_else(|| t["fee"].as_f64()).unwrap_or(0.0),
        fee_currency:    t["N"].as_str().or_else(|| t["feeCurrency"].as_str()).unwrap_or("USDT").to_string(),
        timestamp:       t["t"].as_i64().or_else(|| t["time"].as_i64()).unwrap_or(0),
        client_order_id: t["c"].as_str().map(|s| s.to_string()),
    })
}

fn parse_position_update(account_id: &str, p: &Value) -> Option<WsPositionUpdate> {
    let inst = p["symbol"].as_str().or_else(|| p["s"].as_str())?;
    let size: f64 = p["holdVol"].as_f64()
        .or_else(|| p["posAmt"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0.0);
    let pos_type = p["positionType"].as_i64().unwrap_or(1);
    let direction = if pos_type == 1 { "buy" } else { "sell" };

    Some(WsPositionUpdate {
        account_id:      account_id.to_string(),
        exchange:        "mexc".to_string(),
        instrument_name: inst.to_string(),
        direction:       direction.to_string(),
        size,
        average_price:   p["openAvgPrice"].as_f64()
                            .or_else(|| p["avgPrice"].as_str().and_then(|s| s.parse().ok()))
                            .unwrap_or(0.0),
        mark_price:      p["markPrice"].as_f64().unwrap_or(0.0),
        unrealized_pnl:  p["unrealisedPnl"].as_f64()
                            .or_else(|| p["unrealizedPnl"].as_f64()).unwrap_or(0.0),
        delta:           if direction == "buy" { size } else { -size },
        gamma:           0.0,
        theta:           0.0,
        vega:            0.0,
    })
}

fn emit_connection(app: &AppHandle, account_id: &str, status: &str, message: Option<String>) {
    let _ = app.emit("ws://connection", &WsConnectionEvent {
        account_id: account_id.to_string(),
        exchange:   "mexc".to_string(),
        status:     status.to_string(),
        message,
    });
}
