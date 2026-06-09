use std::collections::HashMap;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, watch};

use crate::api::dispatch;
use crate::api::models::{Account, Instrument, OrderbookSnapshot};

pub mod local_book;
pub mod ws_book;

// ── Config ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggBookConfig {
    pub id: String,
    pub name: String,
    pub base_symbol: String,
    /// "perpetual_linear" | "perpetual_inverse" | "future" | "spot" | "option"
    pub instrument_kind: String,
    pub account_ids: Vec<String>,
    pub unify_quote: bool,
    pub max_levels: u32,
    pub tick_size: Option<f64>,
    pub poll_interval_ms: u64,
    pub active: bool,
}

// ── Aggregated book types ─────────────────────────────────────────────────

/// Full contribution kept internally for merge logic.
#[derive(Debug, Clone)]
pub struct AggContribution {
    pub exchange: String,
    pub account_id: String,
    pub instrument_name: String,
    pub size: f64,
}

/// Compact contribution sent over IPC — only exchange + size to minimise payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggContributionEmit {
    pub exchange: String,
    pub size: f64,
}

#[derive(Debug, Clone)]
pub struct AggLevel {
    pub price: f64,
    pub total_size: f64,
    pub contributions: Vec<AggContribution>,
}

/// Compact level sent over IPC — caps contributions at 8 entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggLevelEmit {
    pub price: f64,
    pub total_size: f64,
    pub contributions: Vec<AggContributionEmit>,
}

impl AggLevel {
    fn to_emit(&self) -> AggLevelEmit {
        AggLevelEmit {
            price: self.price,
            total_size: self.total_size,
            contributions: self.contributions.iter().take(8).map(|c| AggContributionEmit {
                exchange: c.exchange.clone(),
                size: c.size,
            }).collect(),
        }
    }
}

/// Full snapshot stored in Redux — uses compact emit types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggBookSnapshot {
    pub config_id: String,
    pub name: String,
    pub base_symbol: String,
    pub instrument_kind: String,
    /// Capped at EMIT_LEVELS levels (20).
    pub bids: Vec<AggLevelEmit>,
    /// Capped at EMIT_LEVELS levels (20).
    pub asks: Vec<AggLevelEmit>,
    pub exchange_status: HashMap<String, String>,
    pub timestamp: i64,
}

/// Max levels included in each IPC emit. UI rarely shows more than 20 at once.
const EMIT_LEVELS: usize = 20;

// ── Instrument matching ───────────────────────────────────────────────────

fn is_stablecoin(currency: &str) -> bool {
    matches!(
        currency.to_uppercase().as_str(),
        "USDT" | "USDC" | "USD" | "BUSD" | "USDS" | "DAI"
    )
}

pub fn is_matching_instrument(inst: &Instrument, config: &AggBookConfig) -> bool {
    if inst.base_currency.to_uppercase() != config.base_symbol.to_uppercase() {
        return false;
    }
    if !inst.is_active {
        return false;
    }

    // Detect perpetuals:
    // - explicit kind="perpetual" (some exchanges like Hyperliquid)
    // - kind="future" + no expiry (Bybit deliveryTime=0 → expiration_timestamp=None)
    // - instrument name contains "PERPETUAL" (Deribit: BTC-PERPETUAL has far-future expiry, not null)
    let is_perp = inst.kind == "perpetual"
        || (inst.kind == "future" && inst.expiration_timestamp.is_none())
        || inst.instrument_name.to_uppercase().contains("PERPETUAL");

    match config.instrument_kind.as_str() {
        "perpetual_linear" => is_perp && is_stablecoin(&inst.settlement_currency),
        "perpetual_inverse" => is_perp && !is_stablecoin(&inst.settlement_currency),
        "future" => inst.kind == "future" && inst.expiration_timestamp.is_some(),
        "spot" => inst.kind == "spot",
        "option" => inst.kind == "option",
        _ => false,
    }
}

// ── Merge logic ───────────────────────────────────────────────────────────

fn price_key(price: f64, tick_size: Option<f64>) -> i64 {
    let snapped = if let Some(ts) = tick_size {
        (price / ts).round() * ts
    } else {
        price
    };
    (snapped * 1e8) as i64
}

fn snapped_price(price: f64, tick_size: Option<f64>) -> f64 {
    if let Some(ts) = tick_size {
        (price / ts).round() * ts
    } else {
        price
    }
}

fn merge_books(
    books: Vec<(String, String, String, OrderbookSnapshot)>,
    config: &AggBookConfig,
    max_levels: usize,
) -> (Vec<AggLevel>, Vec<AggLevel>) {
    use std::collections::BTreeMap;

    let mut bid_map: BTreeMap<i64, AggLevel> = BTreeMap::new();
    let mut ask_map: BTreeMap<i64, AggLevel> = BTreeMap::new();

    for (exchange, account_id, inst_name, snapshot) in books {
        for level in &snapshot.bids {
            let key = price_key(level.price, config.tick_size);
            let sp = snapped_price(level.price, config.tick_size);
            let entry = bid_map.entry(key).or_insert_with(|| AggLevel {
                price: sp,
                total_size: 0.0,
                contributions: vec![],
            });
            entry.total_size += level.size;
            entry.contributions.push(AggContribution {
                exchange: exchange.clone(),
                account_id: account_id.clone(),
                instrument_name: inst_name.clone(),
                size: level.size,
            });
        }

        for level in &snapshot.asks {
            let key = price_key(level.price, config.tick_size);
            let sp = snapped_price(level.price, config.tick_size);
            let entry = ask_map.entry(key).or_insert_with(|| AggLevel {
                price: sp,
                total_size: 0.0,
                contributions: vec![],
            });
            entry.total_size += level.size;
            entry.contributions.push(AggContribution {
                exchange: exchange.clone(),
                account_id: account_id.clone(),
                instrument_name: inst_name.clone(),
                size: level.size,
            });
        }
    }

    // Bids: descending (highest first) — BTreeMap is ascending so reverse
    let bids: Vec<AggLevel> = bid_map
        .into_values()
        .rev()
        .take(max_levels)
        .collect();

    // Asks: ascending (lowest first)
    let asks: Vec<AggLevel> = ask_map
        .into_values()
        .take(max_levels)
        .collect();

    (bids, asks)
}

// ── AggBookManager ────────────────────────────────────────────────────────

/// Exchanges that support public WS orderbook feeds.
fn has_ws_book(exchange: &str) -> bool {
    matches!(exchange, "deribit" | "okx" | "bybit" | "binance" | "hyperliquid")
}

pub struct AggBookManager {
    handles: Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>,
}

impl AggBookManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            handles: Mutex::new(HashMap::new()),
        })
    }

    pub async fn start(&self, config: AggBookConfig, accounts: Vec<Account>, app: AppHandle) {
        self.stop(&config.id).await;

        let config_id = config.id.clone();

        let handle = tauri::async_runtime::spawn(async move {
            let kind = match config.instrument_kind.as_str() {
                "perpetual_linear" | "perpetual_inverse" => "future",
                "future"  => "future",
                "spot"    => "spot",
                "option"  => "option",
                _         => return,
            };

            let max_levels = config.max_levels.min(200) as usize;

            // ── Step 1: resolve instrument per account via REST (once) ─────
            let relevant: Vec<&Account> = accounts
                .iter()
                .filter(|a| config.account_ids.contains(&a.id))
                .collect();

            // (account_id, exchange, instrument_name, testnet)
            let mut instrument_map: Vec<(String, String, String, bool)> = Vec::new();

            for account in &relevant {
                match dispatch::fetch_instruments(&account.exchange, &config.base_symbol, kind, account.testnet).await {
                    Ok(instruments) => {
                        for inst in &instruments {
                            if is_matching_instrument(inst, &config) {
                                instrument_map.push((
                                    account.id.clone(),
                                    account.exchange.clone(),
                                    inst.instrument_name.clone(),
                                    account.testnet,
                                ));
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[agg_book] fetch_instruments failed for {}: {}", account.exchange, e);
                    }
                }
            }

            let no_instrument_ids: Vec<String> = config.account_ids
                .iter()
                .filter(|id| !instrument_map.iter().any(|(aid, _, _, _)| aid == *id))
                .cloned()
                .collect();

            // ── Step 2: spawn WS feeds (or REST fallback) per account ─────
            // Each feed writes to a watch channel; the merge loop reads all.
            struct Feed {
                account_id: String,
                exchange: String,
                instrument: String,
                rx: watch::Receiver<Option<OrderbookSnapshot>>,
                _handle: tauri::async_runtime::JoinHandle<()>,
            }

            let mut feeds: Vec<Feed> = Vec::new();
            let mut rest_fallbacks: Vec<(String, String, String)> = Vec::new(); // (account_id, exchange, instrument)

            for (account_id, exchange, instrument, testnet) in &instrument_map {
                let (tx, rx) = watch::channel::<Option<OrderbookSnapshot>>(None);

                if has_ws_book(exchange) {
                    let handle = match exchange.as_str() {
                        "deribit"     => ws_book::spawn_deribit(instrument.clone(), *testnet, tx),
                        "okx"         => ws_book::spawn_okx(instrument.clone(), *testnet, tx),
                        "bybit"       => ws_book::spawn_bybit(instrument.clone(), tx),
                        "binance"     => ws_book::spawn_binance(instrument.clone(), tx),
                        "hyperliquid" => ws_book::spawn_hyperliquid(instrument.clone(), tx),
                        _ => unreachable!(),
                    };
                    feeds.push(Feed {
                        account_id: account_id.clone(),
                        exchange: exchange.clone(),
                        instrument: instrument.clone(),
                        rx,
                        _handle: handle,
                    });
                } else {
                    // REST fallback: spawn a polling task that writes to watch
                    let inst = instrument.clone();
                    let ex = exchange.clone();
                    let tn = *testnet;
                    let depth = max_levels as u32;
                    let h = tauri::async_runtime::spawn(async move {
                        loop {
                            match dispatch::fetch_orderbook(&ex, &inst, depth, tn).await {
                                Ok(snap) => { let _ = tx.send(Some(snap)); }
                                Err(e)   => { eprintln!("[agg_book] REST poll error {}: {}", ex, e); }
                            }
                            tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                        }
                    });
                    feeds.push(Feed {
                        account_id: account_id.clone(),
                        exchange: exchange.clone(),
                        instrument: instrument.clone(),
                        rx,
                        _handle: h,
                    });
                }
            }

            // ── Step 3: merge loop ─────────────────────────────────────────
            // Poll receivers at ~100ms; emit to frontend only on top-of-book change
            // or every 10s heartbeat. This is lightweight — no HTTP requests.

            let mut last_best_bid: f64 = -1.0;
            let mut last_best_ask: f64 = -1.0;
            let mut last_emit_ts: i64 = 0;
            const HEARTBEAT_MS: i64 = 10_000;
            // Minimum ms between frontend emits (even if book changes rapidly)
            const MIN_EMIT_INTERVAL_MS: i64 = 100;

            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                let now_ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64;

                let mut exchange_status: HashMap<String, String> = HashMap::new();
                let mut books: Vec<(String, String, String, OrderbookSnapshot)> = Vec::new();

                for feed in &feeds {
                    let snap = feed.rx.borrow().clone();
                    match snap {
                        Some(s) => {
                            exchange_status.insert(feed.account_id.clone(), "ok".to_string());
                            books.push((feed.exchange.clone(), feed.account_id.clone(), feed.instrument.clone(), s));
                        }
                        None => {
                            exchange_status.insert(feed.account_id.clone(), "connecting".to_string());
                        }
                    }
                }

                for id in &no_instrument_ids {
                    exchange_status.entry(id.clone()).or_insert_with(|| "no_instrument".to_string());
                }

                let (bids, asks) = merge_books(books, &config, max_levels);

                let best_bid = bids.first().map(|l| l.price).unwrap_or(0.0);
                let best_ask = asks.first().map(|l| l.price).unwrap_or(0.0);
                let price_changed = (best_bid - last_best_bid).abs() > f64::EPSILON
                    || (best_ask - last_best_ask).abs() > f64::EPSILON;
                let heartbeat_due = now_ts - last_emit_ts >= HEARTBEAT_MS;
                let rate_ok = now_ts - last_emit_ts >= MIN_EMIT_INTERVAL_MS;

                if rate_ok && (price_changed || heartbeat_due) {
                    last_best_bid = best_bid;
                    last_best_ask = best_ask;
                    last_emit_ts = now_ts;

                    let snapshot = AggBookSnapshot {
                        config_id: config.id.clone(),
                        name: config.name.clone(),
                        base_symbol: config.base_symbol.clone(),
                        instrument_kind: config.instrument_kind.clone(),
                        bids: bids.iter().take(EMIT_LEVELS).map(|l| l.to_emit()).collect(),
                        asks: asks.iter().take(EMIT_LEVELS).map(|l| l.to_emit()).collect(),
                        exchange_status,
                        timestamp: now_ts,
                    };

                    let _ = app.emit("agg_book_update", &snapshot);
                }
            }
        });

        let mut handles = self.handles.lock().await;
        handles.insert(config_id, handle);
    }

    pub async fn stop(&self, config_id: &str) {
        let mut handles = self.handles.lock().await;
        if let Some(handle) = handles.remove(config_id) {
            handle.abort();
        }
    }

    pub async fn stop_all(&self) {
        let mut handles = self.handles.lock().await;
        for (_, handle) in handles.drain() {
            handle.abort();
        }
    }
}
