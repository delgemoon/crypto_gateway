//! MarketDataManager — backend public WebSocket subscriptions.
//!
//! Each (exchange, exchange_symbol) pair runs a dedicated tokio task that
//! connects to the exchange's public WS, subscribes to orderbook + ticker
//! channels, and emits Tauri events to the frontend:
//!
//!   `market://book`   — MarketBookEvent
//!   `market://ticker` — MarketTickerEvent

pub mod bybit;
pub mod coincall;
pub mod deribit;
pub mod okx;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

/// Stop signal sent to a running WS task.
pub enum MarketCmd {
    Stop,
}

type SubKey = (String, String); // (exchange, exchange_symbol)

struct SubHandle {
    cmd_tx: mpsc::Sender<MarketCmd>,
}

/// Manages public market-data WS subscriptions across exchanges.
pub struct MarketDataManager {
    subs: Mutex<HashMap<SubKey, SubHandle>>,
}

impl MarketDataManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            subs: Mutex::new(HashMap::new()),
        })
    }

    /// Subscribe to orderbook + ticker for the given instrument.
    ///
    /// - `exchange_symbol`: exchange's own identifier (used for WS subscription)
    /// - `symbol`: our canonical symbol (tagged in emitted events)
    /// - `kind`: "spot" | "perpetual" | "future" | "option" (needed by Bybit routing)
    /// - `ws_url`: only used for CoInCall (pre-signed URL); ignored by other exchanges
    pub async fn subscribe(
        &self,
        app: tauri::AppHandle,
        exchange: String,
        exchange_symbol: String,
        symbol: String,
        kind: String,
        ws_url: Option<String>,
    ) {
        let key: SubKey = (exchange.clone(), exchange_symbol.clone());
        let mut subs = self.subs.lock().await;
        if subs.contains_key(&key) {
            return; // already subscribed
        }

        let (cmd_tx, cmd_rx) = mpsc::channel::<MarketCmd>(4);

        match exchange.as_str() {
            "deribit"  => deribit::spawn(app, exchange_symbol.clone(), symbol, cmd_rx),
            "okx"      => okx::spawn(app, exchange_symbol.clone(), symbol, cmd_rx),
            "bybit"    => bybit::spawn(app, exchange_symbol.clone(), symbol, kind, cmd_rx),
            "coincall" => {
                let url = match ws_url {
                    Some(u) => u,
                    None => {
                        eprintln!("[market] CoInCall requires a signed WS URL");
                        return;
                    }
                };
                coincall::spawn(app, exchange_symbol.clone(), symbol, kind, url, cmd_rx);
            }
            _ => {
                eprintln!("[market] unsupported exchange for public WS: {}", exchange);
                return;
            }
        }

        subs.insert(key, SubHandle { cmd_tx });
    }

    /// Stop the WS task for the given instrument.
    pub async fn unsubscribe(&self, exchange: &str, exchange_symbol: &str) {
        let key: SubKey = (exchange.to_string(), exchange_symbol.to_string());
        if let Some(h) = self.subs.lock().await.remove(&key) {
            let _ = h.cmd_tx.try_send(MarketCmd::Stop);
        }
    }

    /// Stop all active subscriptions.
    pub async fn unsubscribe_all(&self) {
        let mut subs = self.subs.lock().await;
        for h in subs.values() {
            let _ = h.cmd_tx.try_send(MarketCmd::Stop);
        }
        subs.clear();
    }
}
