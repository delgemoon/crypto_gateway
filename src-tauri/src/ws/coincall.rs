/// CoInCall Private WebSocket
///
/// Auth is embedded in the WS URL query string (no separate auth message):
///   `wss://ws.coincall.com/{options|futures}?code=10&uuid=KEY&ts=TS&sign=SIGN&apiKey=KEY`
///
/// We spawn two connections — one for `options` and one for `futures` — so that
/// order/trade/position events from both instrument types are received.
///
/// After connecting we subscribe via:
///   `{"action": "subscribe", "args": ["order", "trade", "position"]}`
///
/// Heartbeat: send `{"action":"ping"}` every 20s.
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

const WS_BASE: &str      = "wss://ws.coincall.com";
const WS_TEST_BASE: &str = "wss://ws-test.coincall.com";
const RECONNECT_MAX_DELAY_S: u64 = 60;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn build_url(base: &str, channel: &str, api_key: &str, api_secret: &str) -> String {
    let ts = now_ms();
    let prehash = format!("GET/users/self/verify?apiKey={}&ts={}", api_key, ts);
    let mut mac = HmacSha256::new_from_slice(api_secret.as_bytes()).unwrap();
    mac.update(prehash.as_bytes());
    let sign = hex::encode(mac.finalize().into_bytes()).to_uppercase();
    format!("{}/{}?code=10&uuid={}&ts={}&sign={}&apiKey={}", base, channel, api_key, ts, sign, api_key)
}

/// Spawn two background tasks: one for options WS, one for futures WS.
/// Each gets its own WsHandle keyed by `{account_id}_options` / `{account_id}_futures`.
pub fn spawn(
    app: AppHandle,
    manager: Arc<WsManager>,
    account_id: String,
    api_key: String,
    api_secret: String,
    testnet: bool,
) {
    for channel in &["options", "futures"] {
        let handle_id = format!("{}_{}", account_id, channel);
        let (cmd_tx, cmd_rx) = mpsc::channel::<WsCommand>(8);
        manager.register(WsHandle {
            account_id: handle_id.clone(),
            exchange: "coincall".to_string(),
            status: WsStatus::Connecting,
            cmd_tx,
        });
        tokio::spawn(run_loop(
            app.clone(),
            manager.clone(),
            handle_id,
            account_id.clone(),
            api_key.clone(),
            api_secret.clone(),
            channel.to_string(),
            testnet,
            cmd_rx,
        ));
    }
}

async fn run_loop(
    app: AppHandle,
    manager: Arc<WsManager>,
    handle_id: String,
    account_id: String,
    api_key: String,
    api_secret: String,
    channel: String,
    testnet: bool,
    mut cmd_rx: mpsc::Receiver<WsCommand>,
) {
    let base = if testnet { WS_TEST_BASE } else { WS_BASE };
    let mut backoff_s: u64 = 1;

    loop {
        if let Ok(WsCommand::Disconnect) = cmd_rx.try_recv() { break; }

        emit_connection(&app, &handle_id, "connecting", None);
        manager.set_status(&handle_id, WsStatus::Connecting);

        let url = build_url(base, &channel, &api_key, &api_secret);

        match connect_async_tls_with_config(&url, None, false, None).await {
            Err(e) => {
                let msg = format!("Connect failed: {}", e);
                eprintln!("[coincall_ws][{}][{}] {}", account_id, channel, msg);
                emit_connection(&app, &handle_id, "error", Some(msg));
                manager.set_status(&handle_id, WsStatus::Reconnecting { attempt: 1 });
                sleep(Duration::from_secs(backoff_s)).await;
                backoff_s = (backoff_s * 2).min(RECONNECT_MAX_DELAY_S);
                continue;
            }
            Ok((ws_stream, _)) => {
                backoff_s = 1;
                eprintln!("[coincall_ws][{}][{}] connected", account_id, channel);
                emit_connection(&app, &handle_id, "connected", None);
                manager.set_status(&handle_id, WsStatus::Connected);

                let disconnected = handle_session(
                    &app, &manager, &handle_id, &account_id, &channel,
                    ws_stream, &mut cmd_rx,
                ).await;

                if !disconnected { break; }
                emit_connection(&app, &handle_id, "reconnecting", None);
                manager.set_status(&handle_id, WsStatus::Reconnecting { attempt: 1 });
                sleep(Duration::from_secs(backoff_s)).await;
                backoff_s = (backoff_s * 2).min(RECONNECT_MAX_DELAY_S);
            }
        }
    }

    emit_connection(&app, &handle_id, "disconnected", None);
    manager.set_status(&handle_id, WsStatus::Disconnected);
    manager.remove(&handle_id);
    eprintln!("[coincall_ws][{}][{}] task exited", account_id, channel);
}

async fn handle_session(
    app: &AppHandle,
    _manager: &Arc<WsManager>,
    handle_id: &str,
    account_id: &str,
    _channel: &str,
    ws_stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>
    >,
    cmd_rx: &mut mpsc::Receiver<WsCommand>,
) -> bool {
    let (mut sink, mut stream) = ws_stream.split();

    // Auth is in the URL — subscribe immediately
    let sub_msg = json!({
        "action": "subscribe",
        "args": ["order", "trade", "position"]
    });
    if sink.send(Message::Text(sub_msg.to_string().into())).await.is_err() {
        return true;
    }
    eprintln!("[coincall_ws][{}] subscribed to order/trade/position", handle_id);

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
                let ping = json!({"action": "ping"});
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
    let topic = v["topic"].as_str()
        .or_else(|| v["action"].as_str())
        .unwrap_or("");

    match topic {
        "order" => {
            if let Some(arr) = v["data"].as_array() {
                for o in arr {
                    if let Some(order) = parse_order_update(account_id, o) {
                        let _ = app.emit("ws://order_update", &order);
                    }
                }
            } else if v["data"].is_object() {
                if let Some(order) = parse_order_update(account_id, &v["data"]) {
                    let _ = app.emit("ws://order_update", &order);
                }
            }
        }
        "trade" => {
            if let Some(arr) = v["data"].as_array() {
                for t in arr {
                    if let Some(trade) = parse_trade_update(account_id, t) {
                        let _ = app.emit("ws://trade_update", &trade);
                    }
                }
            } else if v["data"].is_object() {
                if let Some(trade) = parse_trade_update(account_id, &v["data"]) {
                    let _ = app.emit("ws://trade_update", &trade);
                }
            }
        }
        "position" => {
            if let Some(arr) = v["data"].as_array() {
                for p in arr {
                    if let Some(pos) = parse_position_update(account_id, p) {
                        let _ = app.emit("ws://position_update", &pos);
                    }
                }
            } else if v["data"].is_object() {
                if let Some(pos) = parse_position_update(account_id, &v["data"]) {
                    let _ = app.emit("ws://position_update", &pos);
                }
            }
        }
        _ => {}
    }
}

fn parse_order_update(account_id: &str, o: &Value) -> Option<WsOrderUpdate> {
    let order_id = o["orderId"].as_str()
        .or_else(|| o["id"].as_str())
        .or_else(|| o["order_id"].as_str())?
        .to_string();

    let state_raw = o["orderStatus"].as_str()
        .or_else(|| o["status"].as_str())
        .unwrap_or("");
    let state = match state_raw {
        "0" | "open"     => "open",
        "1" | "partial"  => "open",
        "2" | "filled"   => "filled",
        "3" | "cancelled"| "canceled" => "cancelled",
        "4" | "rejected" => "rejected",
        other            => other,
    };

    let side_raw = o["side"].as_i64().unwrap_or(
        o["side"].as_str().map(|s| if s == "buy" || s == "1" { 1 } else { -1 }).unwrap_or(1)
    );
    let direction = if side_raw == 1 { "buy" } else { "sell" };

    Some(WsOrderUpdate {
        account_id:      account_id.to_string(),
        exchange:        "coincall".to_string(),
        order_id,
        instrument_name: o["symbol"].as_str().unwrap_or("").to_string(),
        direction:       direction.to_string(),
        order_type:      o["orderType"].as_str().unwrap_or("").to_lowercase(),
        order_state:     state.to_string(),
        price:           o["price"].as_f64().or_else(|| o["price"].as_str().and_then(|s| s.parse().ok())),
        amount:          o["qty"].as_f64().or_else(|| o["quantity"].as_f64()).unwrap_or(0.0),
        filled_amount:   o["filledQty"].as_f64().unwrap_or(0.0),
        time_in_force:   o["timeInForce"].as_str().unwrap_or("").to_string(),
        label:           o["clientOrderId"].as_str().map(|s| s.to_string()),
        client_order_id: o["clientOrderId"].as_str().map(|s| s.to_string()),
        timestamp:       o["updateTime"].as_i64().or_else(|| o["ts"].as_i64()).unwrap_or(0),
    })
}

fn parse_trade_update(account_id: &str, t: &Value) -> Option<WsTradeUpdate> {
    let trade_id = t["tradeId"].as_str()
        .or_else(|| t["id"].as_str())
        .or_else(|| t["trade_id"].as_str())?
        .to_string();

    let side_raw = t["side"].as_i64().unwrap_or(
        t["side"].as_str().map(|s| if s == "buy" || s == "1" { 1 } else { -1 }).unwrap_or(1)
    );
    let direction = if side_raw == 1 { "buy" } else { "sell" };

    Some(WsTradeUpdate {
        account_id:      account_id.to_string(),
        exchange:        "coincall".to_string(),
        trade_id,
        order_id:        t["orderId"].as_str().unwrap_or("").to_string(),
        instrument_name: t["symbol"].as_str().unwrap_or("").to_string(),
        direction:       direction.to_string(),
        amount:          t["qty"].as_f64().or_else(|| t["quantity"].as_f64()).unwrap_or(0.0),
        price:           t["price"].as_f64().or_else(|| t["price"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0),
        fee:             t["fee"].as_f64().unwrap_or(0.0),
        fee_currency:    t["feeCurrency"].as_str().unwrap_or("USDT").to_string(),
        timestamp:       t["createTime"].as_i64().or_else(|| t["ts"].as_i64()).unwrap_or(0),
        client_order_id: t["clientOrderId"].as_str().map(|s| s.to_string()),
    })
}

fn parse_position_update(account_id: &str, p: &Value) -> Option<WsPositionUpdate> {
    let inst = p["symbol"].as_str()?;
    let side_raw = p["side"].as_i64().unwrap_or(
        p["side"].as_str().map(|s| if s == "1" { 1 } else { -1 }).unwrap_or(1)
    );
    let direction = if side_raw == 1 { "buy" } else { "sell" };
    let size = p["qty"].as_f64().or_else(|| p["quantity"].as_f64()).unwrap_or(0.0);

    Some(WsPositionUpdate {
        account_id:      account_id.to_string(),
        exchange:        "coincall".to_string(),
        instrument_name: inst.to_string(),
        direction:       direction.to_string(),
        size,
        average_price:   p["avgPrice"].as_f64().unwrap_or(0.0),
        mark_price:      p["markPrice"].as_f64().unwrap_or(0.0),
        unrealized_pnl:  p["unrealisedPnl"].as_f64().or_else(|| p["unrealizedPnl"].as_f64()).unwrap_or(0.0),
        delta:           p["delta"].as_f64().unwrap_or(0.0),
        gamma:           p["gamma"].as_f64().unwrap_or(0.0),
        theta:           p["theta"].as_f64().unwrap_or(0.0),
        vega:            p["vega"].as_f64().unwrap_or(0.0),
    })
}

fn emit_connection(app: &AppHandle, account_id: &str, status: &str, message: Option<String>) {
    let _ = app.emit("ws://connection", &WsConnectionEvent {
        account_id: account_id.to_string(),
        exchange:   "coincall".to_string(),
        status:     status.to_string(),
        message,
    });
}
