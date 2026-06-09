/// WebSocket Manager
///
/// Manages per-account private WebSocket connections.
/// Each connected account gets a background tokio task that:
///   1. Authenticates using API key/secret
///   2. Subscribes to order, trade, and position channels
///   3. Parses incoming messages and emits Tauri events to the frontend
///   4. Auto-reconnects with exponential backoff on disconnect
///
/// Tauri events emitted:
///   - `ws://order_update`    — payload: WsOrderUpdate
///   - `ws://trade_update`    — payload: WsTradeUpdate
///   - `ws://position_update` — payload: WsPositionUpdate
///   - `ws://connection`      — payload: WsConnectionEvent

pub mod binance;
pub mod bullish;
pub mod bybit;
pub mod coincall;
pub mod deribit;
pub mod hyperliquid;
pub mod mexc;
pub mod okx;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use serde::{Deserialize, Serialize};

// ── Public event payloads (sent to frontend) ───────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WsOrderUpdate {
    pub account_id: String,
    pub exchange: String,
    pub order_id: String,
    pub instrument_name: String,
    pub direction: String,
    pub order_type: String,
    pub order_state: String,
    pub price: Option<f64>,
    pub amount: f64,
    pub filled_amount: f64,
    pub time_in_force: String,
    pub label: Option<String>,
    pub client_order_id: Option<String>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WsTradeUpdate {
    pub account_id: String,
    pub exchange: String,
    pub trade_id: String,
    pub order_id: String,
    pub instrument_name: String,
    pub direction: String,
    pub amount: f64,
    pub price: f64,
    pub fee: f64,
    pub fee_currency: String,
    pub timestamp: i64,
    pub client_order_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WsPositionUpdate {
    pub account_id: String,
    pub exchange: String,
    pub instrument_name: String,
    pub direction: String,
    pub size: f64,
    pub average_price: f64,
    pub mark_price: f64,
    pub unrealized_pnl: f64,
    pub delta: f64,
    pub gamma: f64,
    pub theta: f64,
    pub vega: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WsConnectionEvent {
    pub account_id: String,
    pub exchange: String,
    pub status: String, // "connected" | "disconnected" | "reconnecting" | "error"
    pub message: Option<String>,
}

// ── Connection handle ──────────────────────────────────────────────────────

/// Sent to a running WS task to control it.
#[derive(Debug)]
pub enum WsCommand {
    Disconnect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WsStatus {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting { attempt: u32 },
    Error(String),
}

#[derive(Debug)]
pub struct WsHandle {
    pub account_id: String,
    pub exchange: String,
    pub status: WsStatus,
    pub cmd_tx: mpsc::Sender<WsCommand>,
}

// ── Manager ────────────────────────────────────────────────────────────────

/// Singleton WS manager — stored in Tauri app state.
pub struct WsManager {
    handles: Mutex<HashMap<String, WsHandle>>,
}

impl WsManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { handles: Mutex::new(HashMap::new()) })
    }

    /// Register a new connection handle (called from the spawned task).
    pub fn register(&self, handle: WsHandle) {
        self.handles.lock().unwrap().insert(handle.account_id.clone(), handle);
    }

    /// Update status of an existing connection.
    pub fn set_status(&self, account_id: &str, status: WsStatus) {
        if let Some(h) = self.handles.lock().unwrap().get_mut(account_id) {
            h.status = status;
        }
    }

    /// Disconnect a specific account.
    pub fn disconnect(&self, account_id: &str) {
        if let Some(h) = self.handles.lock().unwrap().get(account_id) {
            let _ = h.cmd_tx.try_send(WsCommand::Disconnect);
        }
    }

    /// Disconnect all accounts.
    pub fn disconnect_all(&self) {
        let handles = self.handles.lock().unwrap();
        for h in handles.values() {
            let _ = h.cmd_tx.try_send(WsCommand::Disconnect);
        }
    }

    /// Remove a handle (called when task exits).
    pub fn remove(&self, account_id: &str) {
        self.handles.lock().unwrap().remove(account_id);
    }

    /// Get status snapshot for all accounts.
    pub fn status_all(&self) -> Vec<WsStatusSnapshot> {
        self.handles.lock().unwrap().values().map(|h| WsStatusSnapshot {
            account_id: h.account_id.clone(),
            exchange:   h.exchange.clone(),
            status:     format!("{:?}", h.status),
        }).collect()
    }

    pub fn is_connected(&self, account_id: &str) -> bool {
        self.handles.lock().unwrap()
            .get(account_id)
            .map(|h| matches!(h.status, WsStatus::Connected))
            .unwrap_or(false)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WsStatusSnapshot {
    pub account_id: String,
    pub exchange: String,
    pub status: String,
}
