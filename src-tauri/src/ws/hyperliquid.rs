/// Hyperliquid WebSocket client
///
/// Connects to wss://api.hyperliquid.xyz/ws (or testnet)
/// Subscribes to order updates, fills, and positions for a specific wallet.

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use std::time::Duration;

use crate::api::models::{Order, Position, Trade};
use crate::ws::{WsOrderUpdate, WsPositionUpdate, WsStatus, WsTradeUpdate};

pub async fn run(
    app: AppHandle,
    account_id: String,
    wallet_address: String,
    testnet: bool,
    status_tx: mpsc::Sender<WsStatus>,
) {
    let mut attempt = 0u32;
    loop {
        let _ = status_tx.send(WsStatus::Connecting).await;
        let ws_url = if testnet {
            "wss://api.hyperliquid-testnet.xyz/ws"
        } else {
            "wss://api.hyperliquid.xyz/ws"
        };

        match connect_async(ws_url).await {
            Err(e) => {
                attempt += 1;
                let _ = status_tx.send(WsStatus::Reconnecting { attempt }).await;
                let delay = Duration::from_secs(2u64.pow(attempt.min(6)));
                tokio::time::sleep(delay).await;
                eprintln!("[hyperliquid ws] connect error: {}", e);
                continue;
            }
            Ok((ws_stream, _)) => {
                attempt = 0;
                let _ = status_tx.send(WsStatus::Connected).await;
                let (mut write, mut read) = ws_stream.split();

                // Subscribe to order updates, fills, and webData2 (positions)
                let subscriptions = vec![
                    json!({"method": "subscribe", "subscription": {"type": "orderUpdates", "user": wallet_address}}),
                    json!({"method": "subscribe", "subscription": {"type": "userFills", "user": wallet_address}}),
                    json!({"method": "subscribe", "subscription": {"type": "webData2", "user": wallet_address}}),
                ];

                for sub in subscriptions {
                    if let Err(e) = write.send(Message::Text(sub.to_string())).await {
                        eprintln!("[hyperliquid ws] subscribe error: {}", e);
                        break;
                    }
                }

                // Heartbeat ping every 20s
                let (ping_tx, mut ping_rx) = mpsc::channel::<()>(1);
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(20)).await;
                        if ping_tx.send(()).await.is_err() { break; }
                    }
                });

                let mut disconnected = false;
                loop {
                    tokio::select! {
                        Some(msg) = read.next() => {
                            match msg {
                                Ok(Message::Text(text)) => {
                                    handle_message(&app, &account_id, &text);
                                }
                                Ok(Message::Ping(p)) => {
                                    let _ = write.send(Message::Pong(p)).await;
                                }
                                Ok(Message::Close(_)) | Err(_) => {
                                    disconnected = true;
                                    break;
                                }
                                _ => {}
                            }
                        }
                        Some(_) = ping_rx.recv() => {
                            let ping = json!({"method": "ping"});
                            if write.send(Message::Text(ping.to_string())).await.is_err() {
                                disconnected = true;
                                break;
                            }
                        }
                    }
                }

                if disconnected {
                    attempt += 1;
                    let _ = status_tx.send(WsStatus::Reconnecting { attempt }).await;
                    let delay = Duration::from_secs(2u64.pow(attempt.min(6)));
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
}

fn handle_message(app: &AppHandle, account_id: &str, text: &str) {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return };
    let channel = v["channel"].as_str().unwrap_or_default();

    match channel {
        "orderUpdates" => {
            if let Some(updates) = v["data"].as_array() {
                for upd in updates {
                    if let Some(order) = parse_order(upd, account_id) {
                        let evt = WsOrderUpdate {
                            account_id: account_id.to_string(),
                            exchange: "hyperliquid".to_string(),
                            order_id: order.order_id.clone(),
                            instrument_name: order.instrument_name.clone(),
                            direction: order.direction.clone(),
                            order_type: order.order_type.clone(),
                            order_state: order.order_state.clone(),
                            price: order.price,
                            amount: order.amount,
                            filled_amount: order.filled_amount,
                            time_in_force: order.time_in_force.clone(),
                            label: None,
                            client_order_id: None,
                            timestamp: order.last_update_timestamp,
                        };
                        let _ = app.emit("ws://order_update", &evt);
                    }
                }
            }
        }
        "userFills" => {
            if let Some(fills) = v["data"]["fills"].as_array() {
                for fill in fills {
                    if let Some(trade) = parse_trade(fill, account_id) {
                        let evt = WsTradeUpdate {
                            account_id: account_id.to_string(),
                            exchange: "hyperliquid".to_string(),
                            trade_id: trade.trade_id.clone(),
                            order_id: trade.order_id.clone(),
                            instrument_name: trade.instrument_name.clone(),
                            direction: trade.direction.clone(),
                            amount: trade.amount,
                            price: trade.price,
                            fee: trade.fee,
                            fee_currency: trade.fee_currency.clone(),
                            timestamp: trade.timestamp,
                            client_order_id: None,
                        };
                        let _ = app.emit("ws://trade_update", &evt);
                    }
                }
            }
        }
        "webData2" => {
            if let Some(positions) = v["data"]["clearinghouseState"]["assetPositions"].as_array() {
                for ap in positions {
                    if let Some(pos) = parse_position(&ap["position"], account_id) {
                        let evt = WsPositionUpdate {
                            account_id: account_id.to_string(),
                            exchange: "hyperliquid".to_string(),
                            instrument_name: pos.instrument_name.clone(),
                            direction: pos.direction.clone(),
                            size: pos.size,
                            average_price: pos.average_price,
                            mark_price: pos.mark_price,
                            unrealized_pnl: pos.unrealized_pnl,
                            delta: pos.delta,
                            gamma: pos.gamma,
                            theta: pos.theta,
                            vega: pos.vega,
                        };
                        let _ = app.emit("ws://position_update", &evt);
                    }
                }
            }
        }
        _ => {}
    }
}

fn parse_order(data: &Value, account_id: &str) -> Option<Order> {
    let o = &data["order"];
    let coin = o["coin"].as_str()?;
    let oid = o["oid"].as_i64()?.to_string();
    let side = o["side"].as_str()?;
    let status_str = data["status"].as_str().unwrap_or("open");
    let state = match status_str {
        "filled" | "totallyFilled" => "filled",
        "cancelled" | "canceled" => "cancelled",
        "rejected" => "rejected",
        _ => "open",
    };
    Some(Order {
        order_id:            oid,
        instrument_name:     format!("{}-PERP", coin),
        direction:           if side == "B" { "buy" } else { "sell" }.to_string(),
        order_type:          "limit".to_string(),
        order_state:         state.to_string(),
        price:               o["limitPx"].as_str().and_then(|s| s.parse().ok()),
        amount:              o["sz"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        filled_amount:       o["origSz"].as_str().zip(o["sz"].as_str()).and_then(|(orig, cur)| {
            let orig_f: f64 = orig.parse().ok()?;
            let cur_f: f64 = cur.parse().ok()?;
            Some(orig_f - cur_f)
        }).unwrap_or(0.0),
        average_price:       None,
        post_only:           o["orderType"].as_str().map_or(false, |t| t.to_lowercase().contains("post")),
        time_in_force:       o["tif"].as_str().unwrap_or("Gtc").to_string(),
        creation_timestamp:  o["timestamp"].as_i64().unwrap_or(0),
        last_update_timestamp: data["statusTimestamp"].as_i64().unwrap_or(0),
    })
}

fn parse_trade(fill: &Value, account_id: &str) -> Option<Trade> {
    let coin = fill["coin"].as_str()?;
    let side = fill["side"].as_str()?;
    let tid = fill["tid"].as_i64()?.to_string();
    Some(Trade {
        trade_id:        tid,
        account_id:      account_id.to_string(),
        account_name:    String::new(),
        exchange:        "hyperliquid".to_string(),
        instrument_name: format!("{}-PERP", coin),
        direction:       if side == "B" { "buy" } else { "sell" }.to_string(),
        amount:          fill["sz"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        price:           fill["px"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        fee:             fill["fee"].as_str().and_then(|s| s.parse::<f64>().ok()).map(|v| v.abs()).unwrap_or(0.0),
        fee_currency:    "USDC".to_string(),
        timestamp:       fill["time"].as_i64().unwrap_or(0),
        order_id:        fill["oid"].as_i64().map(|v| v.to_string()).unwrap_or_default(),
    })
}

fn parse_position(pos: &Value, account_id: &str) -> Option<Position> {
    let coin = pos["coin"].as_str()?;
    let szi: f64 = pos["szi"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    if szi == 0.0 { return None; }
    let entry: f64 = pos["entryPx"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let unrealized: f64 = pos["unrealizedPnl"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    Some(Position {
        instrument_name: format!("{}-PERP", coin),
        direction:       if szi > 0.0 { "long" } else { "short" }.to_string(),
        size:            szi.abs(),
        average_price:   entry,
        mark_price:      entry,
        mark_iv:         0.0,
        unrealized_pnl:  unrealized,
        delta:           szi,
        gamma:           0.0,
        theta:           0.0,
        vega:            0.0,
    })
}
