/// Bybit Private WebSocket
///
/// Connects to `wss://stream.bybit.com/v5/private` (or testnet).
/// Authenticates via HMAC-SHA256 on "GET/realtime" + expires.
/// Subscribes to: `order`, `execution`, `position`
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

const WS_URL: &str      = "wss://stream.bybit.com/v5/private";
const WS_TEST_URL: &str = "wss://stream-testnet.bybit.com/v5/private";
const RECONNECT_MAX_DELAY_S: u64 = 60;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn sign(msg: &str, secret: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(msg.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub fn spawn(
    app: AppHandle,
    manager: Arc<WsManager>,
    account_id: String,
    api_key: String,
    api_secret: String,
    testnet: bool,
) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<WsCommand>(8);
    manager.register(WsHandle {
        account_id: account_id.clone(),
        exchange: "bybit".to_string(),
        status: WsStatus::Connecting,
        cmd_tx,
    });
    tokio::spawn(run_loop(app, manager, account_id, api_key, api_secret, testnet, cmd_rx));
}

async fn run_loop(
    app: AppHandle,
    manager: Arc<WsManager>,
    account_id: String,
    api_key: String,
    api_secret: String,
    testnet: bool,
    mut cmd_rx: mpsc::Receiver<WsCommand>,
) {
    let url = if testnet { WS_TEST_URL } else { WS_URL };
    let mut backoff_s: u64 = 1;

    loop {
        if let Ok(WsCommand::Disconnect) = cmd_rx.try_recv() { break; }

        emit_connection(&app, &account_id, "connecting", None);
        manager.set_status(&account_id, WsStatus::Connecting);

        match connect_async_tls_with_config(url, None, false, None).await {
            Err(e) => {
                let msg = format!("Connect failed: {}", e);
                eprintln!("[bybit_ws][{}] {}", account_id, msg);
                emit_connection(&app, &account_id, "error", Some(msg));
                manager.set_status(&account_id, WsStatus::Reconnecting { attempt: 1 });
                sleep(Duration::from_secs(backoff_s)).await;
                backoff_s = (backoff_s * 2).min(RECONNECT_MAX_DELAY_S);
                continue;
            }
            Ok((ws_stream, _)) => {
                backoff_s = 1;
                eprintln!("[bybit_ws][{}] connected", account_id);
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
    eprintln!("[bybit_ws][{}] task exited", account_id);
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
    let expires = now_ms() + 5000;
    let prehash = format!("GET/realtime{}", expires);
    let sig = sign(&prehash, api_secret);

    let auth_msg = json!({
        "op": "auth",
        "args": [api_key, expires, sig]
    });
    if sink.send(Message::Text(auth_msg.to_string().into())).await.is_err() {
        return true;
    }

    // Wait for auth response
    let mut authenticated = false;
    while let Some(Ok(msg)) = stream.next().await {
        if let Message::Text(text) = msg {
            let v: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if v["op"].as_str() == Some("auth") {
                if v["success"].as_bool().unwrap_or(false) {
                    authenticated = true;
                    eprintln!("[bybit_ws][{}] authenticated", account_id);
                } else {
                    let err = v["ret_msg"].as_str().unwrap_or("auth failed");
                    eprintln!("[bybit_ws][{}] auth error: {}", account_id, err);
                    emit_connection(app, account_id, "error", Some(format!("Auth: {}", err)));
                    return true;
                }
                break;
            }
        }
    }
    if !authenticated { return true; }

    // ── Subscribe ───────────────────────────────────────────────────────
    let sub_msg = json!({
        "op": "subscribe",
        "args": ["order", "execution", "position"]
    });
    if sink.send(Message::Text(sub_msg.to_string().into())).await.is_err() {
        return true;
    }
    eprintln!("[bybit_ws][{}] subscribed to order/execution/position", account_id);

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
                let ping = json!({"op": "ping"});
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
    let topic = v["topic"].as_str().unwrap_or("");

    if topic == "order" {
        if let Some(arr) = v["data"].as_array() {
            for o in arr {
                if let Some(order) = parse_order_update(account_id, o) {
                    let _ = app.emit("ws://order_update", &order);
                }
            }
        }
    } else if topic == "execution" {
        if let Some(arr) = v["data"].as_array() {
            for t in arr {
                // Only process actual fills (execType == "Trade")
                if t["execType"].as_str().unwrap_or("") == "Trade" {
                    if let Some(trade) = parse_trade_update(account_id, t) {
                        let _ = app.emit("ws://trade_update", &trade);
                    }
                }
            }
        }
    } else if topic == "position" {
        if let Some(arr) = v["data"].as_array() {
            for p in arr {
                if let Some(pos) = parse_position_update(account_id, p) {
                    let _ = app.emit("ws://position_update", &pos);
                }
            }
        }
    }
}

fn parse_order_update(account_id: &str, o: &Value) -> Option<WsOrderUpdate> {
    let order_id = o["orderId"].as_str()?.to_string();
    let state = match o["orderStatus"].as_str().unwrap_or("") {
        "New"           => "open",
        "PartiallyFilled" => "open",
        "Filled"        => "filled",
        "Cancelled"     => "cancelled",
        "Rejected"      => "rejected",
        other           => other,
    };
    Some(WsOrderUpdate {
        account_id:      account_id.to_string(),
        exchange:        "bybit".to_string(),
        order_id,
        instrument_name: o["symbol"].as_str().unwrap_or("").to_string(),
        direction:       o["side"].as_str().unwrap_or("").to_lowercase(),
        order_type:      o["orderType"].as_str().unwrap_or("").to_lowercase(),
        order_state:     state.to_string(),
        price:           o["price"].as_str().and_then(|s| s.parse().ok()),
        amount:          o["qty"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        filled_amount:   o["cumExecQty"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        time_in_force:   o["timeInForce"].as_str().unwrap_or("").to_string(),
        label:           o["orderLinkId"].as_str().map(|s| s.to_string()),
        client_order_id: o["orderLinkId"].as_str().map(|s| s.to_string()),
        timestamp:       o["updatedTime"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
    })
}

fn parse_trade_update(account_id: &str, t: &Value) -> Option<WsTradeUpdate> {
    let trade_id = t["execId"].as_str()?.to_string();
    Some(WsTradeUpdate {
        account_id:      account_id.to_string(),
        exchange:        "bybit".to_string(),
        trade_id,
        order_id:        t["orderId"].as_str().unwrap_or("").to_string(),
        instrument_name: t["symbol"].as_str().unwrap_or("").to_string(),
        direction:       t["side"].as_str().unwrap_or("").to_lowercase(),
        amount:          t["execQty"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        price:           t["execPrice"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        fee:             t["execFee"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        fee_currency:    t["feeCurrency"].as_str().unwrap_or("").to_string(),
        timestamp:       t["execTime"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
        client_order_id: t["orderLinkId"].as_str().map(|s| s.to_string()),
    })
}

fn parse_position_update(account_id: &str, p: &Value) -> Option<WsPositionUpdate> {
    let inst = p["symbol"].as_str()?;
    let side = p["side"].as_str().unwrap_or("").to_lowercase();
    let size: f64 = p["size"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    Some(WsPositionUpdate {
        account_id:      account_id.to_string(),
        exchange:        "bybit".to_string(),
        instrument_name: inst.to_string(),
        direction:       side,
        size,
        average_price:   p["avgPrice"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        mark_price:      p["markPrice"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        unrealized_pnl:  p["unrealisedPnl"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        // Bybit v5 position topic does not carry full Greeks; use what's available
        delta:           p["delta"].as_f64().or_else(|| p["delta"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0),
        gamma:           0.0,
        theta:           0.0,
        vega:            0.0,
    })
}

fn emit_connection(app: &AppHandle, account_id: &str, status: &str, message: Option<String>) {
    let _ = app.emit("ws://connection", &WsConnectionEvent {
        account_id: account_id.to_string(),
        exchange:   "bybit".to_string(),
        status:     status.to_string(),
        message,
    });
}
