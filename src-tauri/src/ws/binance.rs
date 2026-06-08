/// Binance USDT-M Futures Private WebSocket
///
/// Binance private streams require a **listenKey** obtained via REST.
/// URL: `wss://fstream.binance.com/ws/<listenKey>` (or testnet)
///
/// The listenKey must be kept alive via a REST PUT every 30 minutes.
/// We refresh it every 25 minutes to be safe.
///
/// Subscribes to all user data events automatically (no sub message needed):
///   - ORDER_TRADE_UPDATE → WsOrderUpdate + WsTradeUpdate
///   - ACCOUNT_UPDATE     → WsPositionUpdate
///
/// Auto-reconnects with exponential backoff (1s → 2s → 4s … capped at 60s).

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::{connect_async_tls_with_config, tungstenite::Message};
use reqwest::Client;

use crate::ws::{
    WsCommand, WsConnectionEvent, WsHandle, WsManager, WsOrderUpdate,
    WsPositionUpdate, WsStatus, WsTradeUpdate,
};

const WS_BASE: &str       = "wss://fstream.binance.com/ws";
const WS_TEST_BASE: &str  = "wss://stream.binancefuture.com/ws";
const REST_BASE: &str     = "https://fapi.binance.com";
const REST_TEST: &str     = "https://testnet.binancefuture.com";
const RECONNECT_MAX_DELAY_S: u64 = 60;

async fn get_listen_key(api_key: &str, testnet: bool) -> Result<String, String> {
    let base = if testnet { REST_TEST } else { REST_BASE };
    let client = Client::new();
    let resp = client
        .post(&format!("{}/fapi/v1/listenKey", base))
        .header("X-MBX-APIKEY", api_key)
        .send().await.map_err(|e| e.to_string())?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    v["listenKey"].as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| format!("Failed to get listenKey: {}", v))
}

async fn keepalive_listen_key(api_key: &str, testnet: bool) {
    let base = if testnet { REST_TEST } else { REST_BASE };
    let client = Client::new();
    let _ = client
        .put(&format!("{}/fapi/v1/listenKey", base))
        .header("X-MBX-APIKEY", api_key)
        .send().await;
}

pub fn spawn(
    app: AppHandle,
    manager: Arc<WsManager>,
    account_id: String,
    api_key: String,
    _api_secret: String,
    testnet: bool,
) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<WsCommand>(8);
    manager.register(WsHandle {
        account_id: account_id.clone(),
        exchange: "binance".to_string(),
        status: WsStatus::Connecting,
        cmd_tx,
    });
    tokio::spawn(run_loop(app, manager, account_id, api_key, testnet, cmd_rx));
}

async fn run_loop(
    app: AppHandle,
    manager: Arc<WsManager>,
    account_id: String,
    api_key: String,
    testnet: bool,
    mut cmd_rx: mpsc::Receiver<WsCommand>,
) {
    let mut backoff_s: u64 = 1;

    loop {
        if let Ok(WsCommand::Disconnect) = cmd_rx.try_recv() { break; }

        emit_connection(&app, &account_id, "connecting", None);
        manager.set_status(&account_id, WsStatus::Connecting);

        // Get a fresh listenKey
        let listen_key = match get_listen_key(&api_key, testnet).await {
            Ok(k) => k,
            Err(e) => {
                let msg = format!("listenKey error: {}", e);
                eprintln!("[binance_ws][{}] {}", account_id, msg);
                emit_connection(&app, &account_id, "error", Some(msg));
                manager.set_status(&account_id, WsStatus::Reconnecting { attempt: 1 });
                sleep(Duration::from_secs(backoff_s)).await;
                backoff_s = (backoff_s * 2).min(RECONNECT_MAX_DELAY_S);
                continue;
            }
        };

        let ws_base = if testnet { WS_TEST_BASE } else { WS_BASE };
        let url = format!("{}/{}", ws_base, listen_key);

        match connect_async_tls_with_config(&url, None, false, None).await {
            Err(e) => {
                let msg = format!("Connect failed: {}", e);
                eprintln!("[binance_ws][{}] {}", account_id, msg);
                emit_connection(&app, &account_id, "error", Some(msg));
                manager.set_status(&account_id, WsStatus::Reconnecting { attempt: 1 });
                sleep(Duration::from_secs(backoff_s)).await;
                backoff_s = (backoff_s * 2).min(RECONNECT_MAX_DELAY_S);
                continue;
            }
            Ok((ws_stream, _)) => {
                backoff_s = 1;
                eprintln!("[binance_ws][{}] connected, listenKey={}...", account_id, &listen_key[..8]);
                emit_connection(&app, &account_id, "connected", None);
                manager.set_status(&account_id, WsStatus::Connected);

                let disconnected = handle_session(
                    &app, &manager, &account_id, &api_key, testnet,
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
    eprintln!("[binance_ws][{}] task exited", account_id);
}

async fn handle_session(
    app: &AppHandle,
    _manager: &Arc<WsManager>,
    account_id: &str,
    api_key: &str,
    testnet: bool,
    ws_stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>
    >,
    cmd_rx: &mut mpsc::Receiver<WsCommand>,
) -> bool {
    let (mut sink, mut stream) = ws_stream.split();

    // Keep listenKey alive every 25 minutes
    let mut keepalive_interval = tokio::time::interval(Duration::from_secs(25 * 60));
    keepalive_interval.tick().await; // skip immediate tick

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
            _ = keepalive_interval.tick() => {
                eprintln!("[binance_ws][{}] refreshing listenKey", account_id);
                keepalive_listen_key(api_key, testnet).await;
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
    let event_type = v["e"].as_str().unwrap_or("");

    match event_type {
        "ORDER_TRADE_UPDATE" => {
            let o = &v["o"];
            if let Some(order) = parse_order_update(account_id, o) {
                let _ = app.emit("ws://order_update", &order);
            }
            // If order is filled, also emit a trade update
            if o["X"].as_str() == Some("FILLED") || o["X"].as_str() == Some("PARTIALLY_FILLED") {
                if let Some(trade) = parse_trade_update(account_id, o) {
                    let _ = app.emit("ws://trade_update", &trade);
                }
            }
        }
        "ACCOUNT_UPDATE" => {
            if let Some(positions) = v["a"]["P"].as_array() {
                for p in positions {
                    if let Some(pos) = parse_position_update(account_id, p) {
                        let _ = app.emit("ws://position_update", &pos);
                    }
                }
            }
        }
        _ => {}
    }
}

fn parse_order_update(account_id: &str, o: &Value) -> Option<WsOrderUpdate> {
    let order_id = o["i"].as_i64()?.to_string();
    let state = match o["X"].as_str().unwrap_or("") {
        "NEW"              => "open",
        "PARTIALLY_FILLED" => "open",
        "FILLED"           => "filled",
        "CANCELED"         => "cancelled",
        "EXPIRED"          => "cancelled",
        "REJECTED"         => "rejected",
        other              => other,
    };
    let tif = match o["f"].as_str().unwrap_or("GTC") {
        "GTC" => "good_til_cancelled",
        "IOC" => "immediate_or_cancel",
        "FOK" => "fill_or_kill",
        other => other,
    };
    Some(WsOrderUpdate {
        account_id:      account_id.to_string(),
        exchange:        "binance".to_string(),
        order_id,
        instrument_name: o["s"].as_str().unwrap_or("").to_string(),
        direction:       o["S"].as_str().unwrap_or("").to_lowercase(),
        order_type:      o["o"].as_str().unwrap_or("").to_lowercase(),
        order_state:     state.to_string(),
        price:           o["p"].as_str().and_then(|s| s.parse().ok()),
        amount:          o["q"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        filled_amount:   o["z"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        time_in_force:   tif.to_string(),
        label:           o["c"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_string()),
        client_order_id: o["c"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_string()),
        timestamp:       o["T"].as_i64().unwrap_or(0),
    })
}

fn parse_trade_update(account_id: &str, o: &Value) -> Option<WsTradeUpdate> {
    let trade_id = o["t"].as_i64()?.to_string();
    Some(WsTradeUpdate {
        account_id:      account_id.to_string(),
        exchange:        "binance".to_string(),
        trade_id,
        order_id:        o["i"].as_i64().map(|i| i.to_string()).unwrap_or_default(),
        instrument_name: o["s"].as_str().unwrap_or("").to_string(),
        direction:       o["S"].as_str().unwrap_or("").to_lowercase(),
        amount:          o["l"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        price:           o["L"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        fee:             o["n"].as_str().and_then(|s| s.parse::<f64>().ok()).map(|f| f.abs()).unwrap_or(0.0),
        fee_currency:    o["N"].as_str().unwrap_or("USDT").to_string(),
        timestamp:       o["T"].as_i64().unwrap_or(0),
        client_order_id: o["c"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_string()),
    })
}

fn parse_position_update(account_id: &str, p: &Value) -> Option<WsPositionUpdate> {
    let inst = p["s"].as_str()?;
    let size: f64 = p["pa"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let direction = if size >= 0.0 { "buy" } else { "sell" };
    Some(WsPositionUpdate {
        account_id:      account_id.to_string(),
        exchange:        "binance".to_string(),
        instrument_name: inst.to_string(),
        direction:       direction.to_string(),
        size:            size.abs(),
        average_price:   p["ep"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        mark_price:      0.0,
        unrealized_pnl:  p["up"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        delta:           size.signum(),
        gamma:           0.0,
        theta:           0.0,
        vega:            0.0,
    })
}

fn emit_connection(app: &AppHandle, account_id: &str, status: &str, message: Option<String>) {
    let _ = app.emit("ws://connection", &WsConnectionEvent {
        account_id: account_id.to_string(),
        exchange:   "binance".to_string(),
        status:     status.to_string(),
        message,
    });
}
