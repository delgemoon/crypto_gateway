/// Deribit Private WebSocket
///
/// Connects to `wss://www.deribit.com/ws/api/v2` (or testnet).
/// Authenticates via `public/auth` with client_credentials grant.
/// Subscribes to:
///   - `user.orders.any.any.raw`          → WsOrderUpdate events
///   - `user.trades.any.any.raw`          → WsTradeUpdate events
///   - `user.portfolio.{currency}`        → WsPositionUpdate events (portfolio Greeks)
///
/// Auto-reconnects with exponential backoff (1s → 2s → 4s → … capped at 60s).

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::{connect_async_tls_with_config, tungstenite::Message};

use crate::ws::{
    WsCommand, WsConnectionEvent, WsHandle, WsManager, WsOrderUpdate,
    WsPositionUpdate, WsStatus, WsTradeUpdate,
};

const WS_URL: &str = "wss://www.deribit.com/ws/api/v2";
const WS_TEST_URL: &str = "wss://test.deribit.com/ws/api/v2";

const RECONNECT_MAX_DELAY_S: u64 = 60;
// Currencies to subscribe portfolio updates for
const PORTFOLIO_CURRENCIES: &[&str] = &["BTC", "ETH", "SOL", "USDC"];

/// Spawn a background task managing the Deribit WS connection for one account.
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
        exchange: "deribit".to_string(),
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
        // Check for disconnect command before attempting connection
        if let Ok(WsCommand::Disconnect) = cmd_rx.try_recv() {
            break;
        }

        emit_connection(&app, &account_id, "connecting", None);
        manager.set_status(&account_id, WsStatus::Connecting);

        match connect_async_tls_with_config(url, None, false, None).await {
            Err(e) => {
                let msg = format!("Connect failed: {}", e);
                eprintln!("[deribit_ws][{}] {}", account_id, msg);
                emit_connection(&app, &account_id, "error", Some(msg));
                manager.set_status(&account_id, WsStatus::Reconnecting { attempt: 1 });
                sleep(Duration::from_secs(backoff_s)).await;
                backoff_s = (backoff_s * 2).min(RECONNECT_MAX_DELAY_S);
                continue;
            }
            Ok((ws_stream, _)) => {
                backoff_s = 1; // reset on successful connect
                eprintln!("[deribit_ws][{}] connected to {}", account_id, url);
                emit_connection(&app, &account_id, "connected", None);
                manager.set_status(&account_id, WsStatus::Connected);

                let disconnected = handle_session(
                    &app, &manager, &account_id, &api_key, &api_secret,
                    ws_stream, &mut cmd_rx,
                ).await;

                if !disconnected {
                    // Intentional disconnect
                    break;
                }
                // Connection dropped — reconnect
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
    eprintln!("[deribit_ws][{}] task exited", account_id);
}

/// Run a single WS session. Returns `true` if the connection dropped unexpectedly
/// (should reconnect), or `false` if we received an intentional Disconnect command.
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
    let auth_msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "public/auth",
        "params": {
            "grant_type": "client_credentials",
            "client_id": api_key,
            "client_secret": api_secret
        }
    });
    if sink.send(Message::Text(auth_msg.to_string().into())).await.is_err() {
        return true; // reconnect
    }

    // Wait for auth response
    let mut authenticated = false;
    while let Some(Ok(msg)) = stream.next().await {
        if let Message::Text(text) = msg {
            let v: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if v["id"] == 1 {
                if v.get("result").is_some() {
                    authenticated = true;
                    eprintln!("[deribit_ws][{}] authenticated", account_id);
                } else {
                    let err = v["error"]["message"].as_str().unwrap_or("auth failed");
                    eprintln!("[deribit_ws][{}] auth error: {}", account_id, err);
                    emit_connection(app, account_id, "error", Some(format!("Auth: {}", err)));
                    return true; // reconnect
                }
                break;
            }
        }
    }
    if !authenticated { return true; }

    // ── Subscribe ───────────────────────────────────────────────────────
    let all_channels: Vec<String> = vec![
        "user.orders.any.any.raw".to_string(),
        "user.trades.any.any.raw".to_string(),
    ].into_iter()
        .chain(PORTFOLIO_CURRENCIES.iter().map(|c| format!("user.portfolio.{}", c.to_lowercase())))
        .collect();
    let all_channels_ref: Vec<&str> = all_channels.iter().map(|s| s.as_str()).collect();

    let sub_msg = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "private/subscribe",
        "params": { "channels": all_channels_ref }
    });
    if sink.send(Message::Text(sub_msg.to_string().into())).await.is_err() {
        return true;
    }
    eprintln!("[deribit_ws][{}] subscribed to {} channels", account_id, all_channels_ref.len());

    // ── Heartbeat ping every 15s ────────────────────────────────────────
    let mut ping_interval = tokio::time::interval(Duration::from_secs(15));
    ping_interval.tick().await; // consume first immediate tick

    // ── Main event loop ─────────────────────────────────────────────────
    loop {
        tokio::select! {
            msg = stream.next() => {
                match msg {
                    None | Some(Err(_)) => return true, // disconnected
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
                let hb = json!({
                    "jsonrpc": "2.0", "id": 99,
                    "method": "public/test", "params": {}
                });
                if sink.send(Message::Text(hb.to_string().into())).await.is_err() {
                    return true;
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(WsCommand::Disconnect) | None => {
                        let _ = sink.send(Message::Close(None)).await;
                        return false; // intentional disconnect
                    }
                }
            }
        }
    }
}

// ── Message dispatcher ─────────────────────────────────────────────────────

fn dispatch_message(app: &AppHandle, account_id: &str, v: &Value) {
    // JSON-RPC notifications have "method" = "subscription"
    if v["method"].as_str() != Some("subscription") { return; }

    let channel = v["params"]["channel"].as_str().unwrap_or("");
    let data = &v["params"]["data"];

    if channel.starts_with("user.orders") {
        // data is a single order object
        if let Some(order) = parse_order_update(account_id, data) {
            let _ = app.emit("ws://order_update", &order);
        }
    } else if channel.starts_with("user.trades") {
        // data is an array of trades
        if let Some(arr) = data.as_array() {
            for t in arr {
                if let Some(trade) = parse_trade_update(account_id, t) {
                    let _ = app.emit("ws://trade_update", &trade);
                }
            }
        }
    } else if channel.starts_with("user.portfolio") {
        if let Some(pos) = parse_portfolio_update(account_id, data) {
            let _ = app.emit("ws://position_update", &pos);
        }
    }
}

fn parse_order_update(account_id: &str, o: &Value) -> Option<WsOrderUpdate> {
    let order_id = o["order_id"].as_str()?.to_string();
    Some(WsOrderUpdate {
        account_id:       account_id.to_string(),
        exchange:         "deribit".to_string(),
        order_id,
        instrument_name:  o["instrument_name"].as_str().unwrap_or("").to_string(),
        direction:        o["direction"].as_str().unwrap_or("").to_string(),
        order_type:       o["order_type"].as_str().unwrap_or("").to_string(),
        order_state:      o["order_state"].as_str().unwrap_or("").to_string(),
        price:            o["price"].as_f64(),
        amount:           o["amount"].as_f64().unwrap_or(0.0),
        filled_amount:    o["filled_amount"].as_f64().unwrap_or(0.0),
        time_in_force:    o["time_in_force"].as_str().unwrap_or("").to_string(),
        label:            o["label"].as_str().map(|s| s.to_string()),
        client_order_id:  o["label"].as_str().map(|s| s.to_string()),
        timestamp:        o["last_update_timestamp"].as_i64().unwrap_or(0),
    })
}

fn parse_trade_update(account_id: &str, t: &Value) -> Option<WsTradeUpdate> {
    let trade_id = t["trade_id"].as_str()?.to_string();
    Some(WsTradeUpdate {
        account_id:       account_id.to_string(),
        exchange:         "deribit".to_string(),
        trade_id,
        order_id:         t["order_id"].as_str().unwrap_or("").to_string(),
        instrument_name:  t["instrument_name"].as_str().unwrap_or("").to_string(),
        direction:        t["direction"].as_str().unwrap_or("").to_string(),
        amount:           t["amount"].as_f64().unwrap_or(0.0),
        price:            t["price"].as_f64().unwrap_or(0.0),
        fee:              t["fee"].as_f64().unwrap_or(0.0),
        fee_currency:     t["fee_currency"].as_str().unwrap_or("").to_string(),
        timestamp:        t["timestamp"].as_i64().unwrap_or(0),
        client_order_id:  t["label"].as_str().map(|s| s.to_string()),
    })
}

fn parse_portfolio_update(account_id: &str, d: &Value) -> Option<WsPositionUpdate> {
    // portfolio update has currency + aggregate greeks (delta_total, gamma_total, etc.)
    // We emit as a synthetic "portfolio" position per currency
    let currency = d["currency"].as_str()?;
    Some(WsPositionUpdate {
        account_id:       account_id.to_string(),
        exchange:         "deribit".to_string(),
        instrument_name:  format!("PORTFOLIO_{}", currency),
        direction:        "net".to_string(),
        size:             0.0,
        average_price:    0.0,
        mark_price:       0.0,
        unrealized_pnl:   d["session_upl"].as_f64().unwrap_or(0.0),
        delta:            d["delta_total"].as_f64().unwrap_or(0.0),
        gamma:            d["gamma_total"].as_f64().unwrap_or(0.0),
        theta:            d["theta_total"].as_f64().unwrap_or(0.0),
        vega:             d["vega_total"].as_f64().unwrap_or(0.0),
    })
}

fn emit_connection(app: &AppHandle, account_id: &str, status: &str, message: Option<String>) {
    let _ = app.emit("ws://connection", &WsConnectionEvent {
        account_id: account_id.to_string(),
        exchange:   "deribit".to_string(),
        status:     status.to_string(),
        message,
    });
}
