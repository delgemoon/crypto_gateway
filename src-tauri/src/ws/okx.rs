/// OKX Private WebSocket
///
/// Connects to `wss://ws.okx.com:8443/ws/v5/private` (or simulated trading).
/// Authenticates via `login` op with HMAC-SHA256(ts + "GET" + "/users/self/verify").
/// Subscribes to: `orders` (ANY), `fills` (ANY), `positions` (ANY)
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
use base64::{Engine as _, engine::general_purpose};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ws::{
    WsCommand, WsConnectionEvent, WsHandle, WsManager, WsOrderUpdate,
    WsPositionUpdate, WsStatus, WsTradeUpdate,
};

type HmacSha256 = Hmac<Sha256>;

const WS_URL: &str      = "wss://ws.okx.com:8443/ws/v5/private";
const WS_SIM_URL: &str  = "wss://wspap.okx.com:8443/ws/v5/private?brokerId=9999";
const RECONNECT_MAX_DELAY_S: u64 = 60;

fn now_secs_str() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn sign(timestamp: &str, secret: &str) -> String {
    let msg = format!("{}GET/users/self/verify", timestamp);
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(msg.as_bytes());
    general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

pub fn spawn(
    app: AppHandle,
    manager: Arc<WsManager>,
    account_id: String,
    api_key: String,
    api_secret: String,
    passphrase: String,
    testnet: bool,
) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<WsCommand>(8);
    manager.register(WsHandle {
        account_id: account_id.clone(),
        exchange: "okx".to_string(),
        status: WsStatus::Connecting,
        cmd_tx,
    });
    tokio::spawn(run_loop(app, manager, account_id, api_key, api_secret, passphrase, testnet, cmd_rx));
}

async fn run_loop(
    app: AppHandle,
    manager: Arc<WsManager>,
    account_id: String,
    api_key: String,
    api_secret: String,
    passphrase: String,
    testnet: bool,
    mut cmd_rx: mpsc::Receiver<WsCommand>,
) {
    let url = if testnet { WS_SIM_URL } else { WS_URL };
    let mut backoff_s: u64 = 1;

    loop {
        if let Ok(WsCommand::Disconnect) = cmd_rx.try_recv() { break; }

        emit_connection(&app, &account_id, "connecting", None);
        manager.set_status(&account_id, WsStatus::Connecting);

        match connect_async_tls_with_config(url, None, false, None).await {
            Err(e) => {
                let msg = format!("Connect failed: {}", e);
                eprintln!("[okx_ws][{}] {}", account_id, msg);
                emit_connection(&app, &account_id, "error", Some(msg));
                manager.set_status(&account_id, WsStatus::Reconnecting { attempt: 1 });
                sleep(Duration::from_secs(backoff_s)).await;
                backoff_s = (backoff_s * 2).min(RECONNECT_MAX_DELAY_S);
                continue;
            }
            Ok((ws_stream, _)) => {
                backoff_s = 1;
                eprintln!("[okx_ws][{}] connected", account_id);
                emit_connection(&app, &account_id, "connected", None);
                manager.set_status(&account_id, WsStatus::Connected);

                let disconnected = handle_session(
                    &app, &manager, &account_id, &api_key, &api_secret, &passphrase,
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
    eprintln!("[okx_ws][{}] task exited", account_id);
}

async fn handle_session(
    app: &AppHandle,
    _manager: &Arc<WsManager>,
    account_id: &str,
    api_key: &str,
    api_secret: &str,
    passphrase: &str,
    ws_stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>
    >,
    cmd_rx: &mut mpsc::Receiver<WsCommand>,
) -> bool {
    let (mut sink, mut stream) = ws_stream.split();

    // ── Authenticate ────────────────────────────────────────────────────
    let ts = now_secs_str();
    let sig = sign(&ts, api_secret);

    let login_msg = json!({
        "op": "login",
        "args": [{
            "apiKey": api_key,
            "passphrase": passphrase,
            "timestamp": ts,
            "sign": sig
        }]
    });
    if sink.send(Message::Text(login_msg.to_string().into())).await.is_err() {
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
            if v["event"].as_str() == Some("login") {
                if v["code"].as_str() == Some("0") {
                    authenticated = true;
                    eprintln!("[okx_ws][{}] authenticated", account_id);
                } else {
                    let err = v["msg"].as_str().unwrap_or("auth failed");
                    eprintln!("[okx_ws][{}] auth error: {}", account_id, err);
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
        "args": [
            { "channel": "orders",    "instType": "ANY" },
            { "channel": "fills",     "instType": "ANY" },
            { "channel": "positions", "instType": "ANY" }
        ]
    });
    if sink.send(Message::Text(sub_msg.to_string().into())).await.is_err() {
        return true;
    }
    eprintln!("[okx_ws][{}] subscribed to orders/fills/positions", account_id);

    // ── Heartbeat ping every 25s ────────────────────────────────────────
    let mut ping_interval = tokio::time::interval(Duration::from_secs(25));
    ping_interval.tick().await;

    loop {
        tokio::select! {
            msg = stream.next() => {
                match msg {
                    None | Some(Err(_)) => return true,
                    Some(Ok(Message::Text(text))) => {
                        // OKX heartbeat reply is plain "pong"
                        if text.trim() == "pong" { continue; }
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
                // OKX requires plain text "ping"
                if sink.send(Message::Text("ping".into())).await.is_err() {
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
    // Only handle push data (no "event" field, or event is empty)
    let event = v["event"].as_str().unwrap_or("");
    if !event.is_empty() { return; } // subscription confirmations, etc.

    let channel = v["arg"]["channel"].as_str().unwrap_or("");

    if channel == "orders" {
        if let Some(arr) = v["data"].as_array() {
            for o in arr {
                if let Some(order) = parse_order_update(account_id, o) {
                    let _ = app.emit("ws://order_update", &order);
                }
            }
        }
    } else if channel == "fills" {
        if let Some(arr) = v["data"].as_array() {
            for t in arr {
                if let Some(trade) = parse_trade_update(account_id, t) {
                    let _ = app.emit("ws://trade_update", &trade);
                }
            }
        }
    } else if channel == "positions" {
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
    let order_id = o["ordId"].as_str()?.to_string();
    let state = match o["state"].as_str().unwrap_or("") {
        "live"           => "open",
        "partially_filled" => "open",
        "filled"         => "filled",
        "canceled"       => "cancelled",
        other            => other,
    };
    Some(WsOrderUpdate {
        account_id:      account_id.to_string(),
        exchange:        "okx".to_string(),
        order_id,
        instrument_name: o["instId"].as_str().unwrap_or("").to_string(),
        direction:       o["side"].as_str().unwrap_or("").to_string(),
        order_type:      o["ordType"].as_str().unwrap_or("").to_string(),
        order_state:     state.to_string(),
        price:           o["px"].as_str().and_then(|s| s.parse().ok()),
        amount:          o["sz"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        filled_amount:   o["fillSz"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        time_in_force:   o["tif"].as_str().unwrap_or("").to_string(),
        label:           o["clOrdId"].as_str().map(|s| s.to_string()),
        client_order_id: o["clOrdId"].as_str().map(|s| s.to_string()),
        timestamp:       o["uTime"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
    })
}

fn parse_trade_update(account_id: &str, t: &Value) -> Option<WsTradeUpdate> {
    let trade_id = t["tradeId"].as_str()?.to_string();
    Some(WsTradeUpdate {
        account_id:      account_id.to_string(),
        exchange:        "okx".to_string(),
        trade_id,
        order_id:        t["ordId"].as_str().unwrap_or("").to_string(),
        instrument_name: t["instId"].as_str().unwrap_or("").to_string(),
        direction:       t["side"].as_str().unwrap_or("").to_string(),
        amount:          t["fillSz"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        price:           t["fillPx"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        fee:             t["fee"].as_str().and_then(|s| s.parse::<f64>().ok()).map(|f| f.abs()).unwrap_or(0.0),
        fee_currency:    t["feeCcy"].as_str().unwrap_or("").to_string(),
        timestamp:       t["ts"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
        client_order_id: t["clOrdId"].as_str().map(|s| s.to_string()),
    })
}

fn parse_position_update(account_id: &str, p: &Value) -> Option<WsPositionUpdate> {
    let inst = p["instId"].as_str()?;
    let size: f64 = p["pos"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let direction = match p["posSide"].as_str().unwrap_or("") {
        "long"  => "buy",
        "short" => "sell",
        "net"   => if size >= 0.0 { "buy" } else { "sell" },
        other   => other,
    };
    Some(WsPositionUpdate {
        account_id:      account_id.to_string(),
        exchange:        "okx".to_string(),
        instrument_name: inst.to_string(),
        direction:       direction.to_string(),
        size:            size.abs(),
        average_price:   p["avgPx"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        mark_price:      p["markPx"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        unrealized_pnl:  p["upl"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        delta:           p["deltaBS"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        gamma:           p["gammaBS"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        theta:           p["thetaBS"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        vega:            p["vegaBS"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
    })
}

fn emit_connection(app: &AppHandle, account_id: &str, status: &str, message: Option<String>) {
    let _ = app.emit("ws://connection", &WsConnectionEvent {
        account_id: account_id.to_string(),
        exchange:   "okx".to_string(),
        status:     status.to_string(),
        message,
    });
}
