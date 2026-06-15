/// CoInCall Private WebSocket
///
/// Auth is embedded in the WS URL query string (no separate auth message):
///   `wss://ws.coincall.com/{options|futures}?code=10&uuid=KEY&ts=TS&sign=SIGN&apiKey=KEY`
///
/// We spawn two connections — one for `options` and one for `futures` — so that
/// order/trade/position events from both instrument types are received.
///
/// Subscription (each channel separately, using `dataType`):
///   Options:  order(dt:11)  trade(dt:15)  position(dt:12)  positionEvent(dt:27)
///   Futures:  order(dt:35)  trade(dt:38)
///
/// Heartbeat: send `{"action":"heartbeat"}` every 15s; expect `{"c":11,"rc":1}`.
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
    WsPositionUpdate, WsRfqUpdate, WsStatus, WsTradeUpdate,
};

type HmacSha256 = Hmac<Sha256>;

const WS_BASE: &str      = "wss://ws.coincall.com";
const WS_TEST_BASE: &str = "wss://betaws.seizeyouralpha.com";
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
    // Spot endpoint has an extra /ws path segment; options and futures do not.
    if channel == "spot" {
        format!("{}/{}/ws?code=10&uuid={}&ts={}&sign={}&apiKey={}", base, channel, api_key, ts, sign, api_key)
    } else {
        format!("{}/{}?code=10&uuid={}&ts={}&sign={}&apiKey={}", base, channel, api_key, ts, sign, api_key)
    }
}

/// Spawn three background tasks: options, futures, and spot private WS.
/// Each gets its own WsHandle keyed by `{account_id}_options` / `{account_id}_futures` / `{account_id}_spot`.
pub fn spawn(
    app: AppHandle,
    manager: Arc<WsManager>,
    account_id: String,
    api_key: String,
    api_secret: String,
    testnet: bool,
) {
    for channel in &["options", "futures", "spot"] {
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
    channel: &str,
    ws_stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>
    >,
    cmd_rx: &mut mpsc::Receiver<WsCommand>,
) -> bool {
    let (mut sink, mut stream) = ws_stream.split();

    // Subscribe to each private channel separately using CoInCall's dataType format.
    // Options WS: order(dt:11), trade(dt:15), position(dt:12), positionEvent(dt:27),
    //             blockTrade(dt:20) — RFQ / block-trade notifications
    // Futures WS: order(dt:35), trade(dt:38)
    // Spot WS:    order(dt:35), trade(dt:38)  (same dt codes as futures on spot endpoint)
    let sub_channels: &[&str] = match channel {
        "options" => &["order", "trade", "position", "positionEvent", "blockTrade"],
        _         => &["order", "trade"],  // futures + spot
    };

    for ch in sub_channels {
        let msg = json!({ "action": "subscribe", "dataType": ch });
        if sink.send(Message::Text(msg.to_string().into())).await.is_err() {
            return true;
        }
    }
    eprintln!("[coincall_ws][{}] subscribed {:?}", handle_id, sub_channels);

    // ── Heartbeat every 15s ─────────────────────────────────────────────────
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    heartbeat.tick().await;

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
                        // Ignore subscription ack / heartbeat responses (contain "rc" field)
                        if v["rc"].is_number() { continue; }
                        dispatch_message(app, account_id, channel == "options", &v);
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = sink.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Close(_))) => return true,
                    _ => {}
                }
            }
            _ = heartbeat.tick() => {
                let ping = json!({"action": "heartbeat"});
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

// ── CoInCall dt code constants ─────────────────────────────────────────────
// Options WS
const DT_OPT_ORDER:    i64 = 11;
const DT_OPT_TRADE:    i64 = 15;
const DT_OPT_POSITION: i64 = 12;
const DT_OPT_POS_EVT:  i64 = 27;
const DT_BLOCK_TRADE:  i64 = 20;   // RFQ / block-trade channel
// Futures WS
const DT_FUT_ORDER:    i64 = 35;
const DT_FUT_TRADE:    i64 = 38;

fn dispatch_message(app: &AppHandle, account_id: &str, is_options: bool, v: &Value) {
    let dt = match v["dt"].as_i64() { Some(d) => d, None => return };
    let data = &v["d"];

    if is_options {
        match dt {
            DT_OPT_ORDER => {
                if let Some(order) = parse_order_update(account_id, data) {
                    let _ = app.emit("ws://order_update", &order);
                }
            }
            DT_OPT_TRADE => {
                if let Some(trade) = parse_options_trade_update(account_id, data) {
                    let _ = app.emit("ws://trade_update", &trade);
                }
            }
            DT_OPT_POSITION | DT_OPT_POS_EVT => {
                if let Some(pos) = parse_options_position_update(account_id, data) {
                    let _ = app.emit("ws://position_update", &pos);
                }
            }
            DT_BLOCK_TRADE => {
                if let Some(update) = parse_block_trade_update(account_id, data) {
                    let _ = app.emit("ws://coincall_rfq_update", &update);
                }
            }
            _ => {}
        }
    } else {
        match dt {
            DT_FUT_ORDER => {
                if let Some(order) = parse_order_update(account_id, data) {
                    let _ = app.emit("ws://order_update", &order);
                }
            }
            DT_FUT_TRADE => {
                if let Some(trade) = parse_futures_trade_update(account_id, data) {
                    let _ = app.emit("ws://trade_update", &trade);
                }
            }
            _ => {}
        }
    }
}

// ── Parse helpers ──────────────────────────────────────────────────────────

/// Parse a value that CoInCall may send as either number or numeric string.
fn parse_f64(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn parse_str(v: &Value) -> Option<String> {
    v.as_str().map(|s| s.to_string())
        .or_else(|| v.as_i64().map(|n| n.to_string()))
        .or_else(|| v.as_u64().map(|n| n.to_string()))
}

/// Map CoInCall numeric `os` order status to canonical string.
fn map_order_state(os: i64) -> &'static str {
    match os {
        0  => "open",       // NEW
        1  => "filled",     // FILLED
        2  => "open",       // PARTIALLY_FILLED (still open)
        3  => "cancelled",  // CANCELED
        4  => "cancelled",  // PRE_CANCEL
        5  => "cancelled",  // CANCELING
        6  => "rejected",   // INVALID
        10 => "filled",     // CANCEL_BY_EXERCISE
        _  => "unknown",
    }
}

/// Map CoInCall numeric `ty` trade type to string.
fn map_order_type(ty: i64) -> &'static str {
    match ty {
        1  => "limit",
        2  => "market",
        3  => "limit",     // POST_ONLY
        4  => "stop_limit",
        5  => "stop_market",
        14 => "block_trade",
        _  => "limit",
    }
}

/// Parse an order update from either options(dt:11) or futures(dt:35).
/// Both use abbreviated field names in the `d` object.
fn parse_order_update(account_id: &str, o: &Value) -> Option<WsOrderUpdate> {
    let order_id = parse_str(&o["oid"])?;
    let symbol = parse_str(&o["s"]).unwrap_or_default();
    let si = o["si"].as_i64().unwrap_or(1);
    let direction = if si == 1 { "buy" } else { "sell" };
    let os = o["os"].as_i64()
        .or_else(|| o["os"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0);
    let ty = o["ty"].as_i64().unwrap_or(1);
    let tif = match o["tif"].as_i64().unwrap_or(0) {
        0 => "good_til_cancelled",
        1 => "good_til_cancelled", // POST_ONLY
        2 => "immediate_or_cancel",
        3 => "fill_or_kill",
        _ => "good_til_cancelled",
    };
    let amount = parse_f64(&o["q"]).unwrap_or(0.0);
    let filled = parse_f64(&o["fq"]).unwrap_or(0.0);
    let price  = parse_f64(&o["pr"]);
    let ts = o["ts"].as_i64().or_else(|| o["ct"].as_i64()).unwrap_or(0);
    let client_order_id = parse_str(&o["coid"]);

    Some(WsOrderUpdate {
        account_id:      account_id.to_string(),
        exchange:        "coincall".to_string(),
        order_id,
        instrument_name: symbol,
        direction:       direction.to_string(),
        order_type:      map_order_type(ty).to_string(),
        order_state:     map_order_state(os).to_string(),
        price,
        amount,
        filled_amount:   filled,
        time_in_force:   tif.to_string(),
        label:           client_order_id.clone(),
        client_order_id,
        timestamp: ts,
    })
}

/// Parse a trade update from options WS (dt:15).
/// Options trades use full camelCase field names.
fn parse_options_trade_update(account_id: &str, t: &Value) -> Option<WsTradeUpdate> {
    let trade_id = parse_str(&t["tradeId"])?;
    let order_id = parse_str(&t["orderId"]).unwrap_or_default();
    let symbol   = parse_str(&t["symbol"]).unwrap_or_default();
    let side_raw = t["orderSide"].as_str().unwrap_or("1");
    let direction = if side_raw == "1" { "buy" } else { "sell" };
    let price  = parse_f64(&t["matchPrice"]).unwrap_or(0.0);
    let amount = parse_f64(&t["matchQty"]).unwrap_or(0.0);
    let fee_rate = parse_f64(&t["feeRate"]).unwrap_or(0.0);
    let fee = price * amount * fee_rate.abs();
    let ts = parse_f64(&t["tradeTime"]).unwrap_or(0.0) as i64;
    let client_order_id = parse_str(&t["clientOrderId"]);

    Some(WsTradeUpdate {
        account_id:      account_id.to_string(),
        exchange:        "coincall".to_string(),
        trade_id,
        order_id,
        instrument_name: symbol,
        direction:       direction.to_string(),
        amount,
        price,
        fee,
        fee_currency:    "USDT".to_string(),
        timestamp: ts,
        client_order_id,
    })
}

/// Parse a trade update from futures WS (dt:38).
/// Futures trades use abbreviated field names.
fn parse_futures_trade_update(account_id: &str, t: &Value) -> Option<WsTradeUpdate> {
    let trade_id = parse_str(&t["tid"])?;
    let order_id = parse_str(&t["oid"]).unwrap_or_default();
    let symbol   = parse_str(&t["s"]).unwrap_or_default();
    let si = t["si"].as_i64().or_else(|| t["si"].as_str().and_then(|s| s.parse().ok())).unwrap_or(1);
    let direction = if si == 1 { "buy" } else { "sell" };
    let price  = parse_f64(&t["mpr"]).unwrap_or(0.0);
    let amount = parse_f64(&t["mq"]).unwrap_or(0.0);
    let fee_rate = parse_f64(&t["fr"]).unwrap_or(0.0);
    let fee = price * amount * fee_rate.abs();
    let ts = t["ct"].as_i64().or_else(|| t["ts"].as_i64()).unwrap_or(0);
    let client_order_id = parse_str(&t["coid"]);

    Some(WsTradeUpdate {
        account_id:      account_id.to_string(),
        exchange:        "coincall".to_string(),
        trade_id,
        order_id,
        instrument_name: symbol,
        direction:       direction.to_string(),
        amount,
        price,
        fee,
        fee_currency:    "USDT".to_string(),
        timestamp: ts,
        client_order_id,
    })
}

/// Parse a position update from options WS (dt:12 snapshot / dt:27 event).
/// Both use abbreviated field names.
fn parse_options_position_update(account_id: &str, p: &Value) -> Option<WsPositionUpdate> {
    let symbol = parse_str(&p["s"])?;
    let si = p["si"].as_i64()
        .or_else(|| p["si"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(1);
    let direction = if si == 1 { "buy" } else { "sell" };
    let size = parse_f64(&p["q"]).unwrap_or(0.0);

    Some(WsPositionUpdate {
        account_id:      account_id.to_string(),
        exchange:        "coincall".to_string(),
        instrument_name: symbol,
        direction:       direction.to_string(),
        size,
        average_price:   parse_f64(&p["ap"]).unwrap_or(0.0),
        mark_price:      parse_f64(&p["mp"]).unwrap_or(0.0),
        unrealized_pnl:  parse_f64(&p["upnl"]).unwrap_or(0.0),
        delta:           parse_f64(&p["delta"]).unwrap_or(0.0),
        gamma:           parse_f64(&p["gamma"]).unwrap_or(0.0),
        theta:           parse_f64(&p["theta"]).unwrap_or(0.0),
        vega:            parse_f64(&p["vega"]).unwrap_or(0.0),
    })
}

/// Parse a block-trade/RFQ notification from options WS (dt:20).
///
/// The `d` field contains `{ "msg": { "msgType": N, "content": ... } }`.
/// msgType meanings:
///   1 ADD_SEEK  2 CANCEL_SEEK  3 EXPIRE_SEEK  4 ADD_QUOTE  5 CANCEL_QUOTE
///   6 EXPIRE_QUOTE  7 DEAL_MM  8 MSG_MM  9 MSG_USER
fn parse_block_trade_update(account_id: &str, d: &Value) -> Option<WsRfqUpdate> {
    // CoInCall may wrap the payload in a "msg" field or send it flat in "d".
    // Try both layouts:  d["msgType"]  or  d["msg"]["msgType"]
    let msg_type = d["msgType"].as_i64()
        .or_else(|| d["msg"]["msgType"].as_i64())
        .unwrap_or(-1);

    // Determine which node contains "content"
    let content_node = if !d["content"].is_null() { &d["content"] }
                       else { &d["msg"]["content"] };

    let request_id = content_node["seekId"]
        .as_i64().map(|n| n.to_string())
        .or_else(|| content_node["seekId"].as_str().map(|s| s.to_string()))
        .or_else(|| {
            // Some msgTypes encode content as a JSON string containing an array
            let s = content_node.as_str()?;
            let arr: Value = serde_json::from_str(s).ok()?;
            arr.as_array()?.first()?.as_i64().map(|n| n.to_string())
        });

    eprintln!(
        "[coincall_ws][{}] block_trade dt:20 raw={} msgType:{} seekId:{:?}",
        account_id,
        serde_json::to_string(d).unwrap_or_default(),
        msg_type,
        request_id
    );

    Some(WsRfqUpdate {
        account_id: account_id.to_string(),
        msg_type,
        request_id,
        raw: d.clone(),
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
