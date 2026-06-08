/// In-memory orderbook state maintained from WS snapshot + delta updates.
///
/// Uses `BTreeMap<i64, (f64, f64)>` (price_key → (price, size)).
/// Price key = (price * 1e8) as i64 — avoids float comparison issues.
use std::collections::BTreeMap;
use crate::api::models::{OrderbookLevel, OrderbookSnapshot};

fn price_key(price: f64) -> i64 {
    (price * 1e8) as i64
}

pub struct LocalBook {
    pub instrument_name: String,
    /// Descending order: highest key = best bid
    bids: BTreeMap<i64, (f64, f64)>,
    /// Ascending order: lowest key = best ask
    asks: BTreeMap<i64, (f64, f64)>,
    pub timestamp: i64,
}

impl LocalBook {
    pub fn new(instrument_name: &str) -> Self {
        Self {
            instrument_name: instrument_name.to_string(),
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            timestamp: 0,
        }
    }

    /// Replace entire book with a fresh snapshot.
    pub fn apply_snapshot(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], ts: i64) {
        self.bids.clear();
        self.asks.clear();
        for &(price, size) in bids {
            if size > 0.0 { self.bids.insert(price_key(price), (price, size)); }
        }
        for &(price, size) in asks {
            if size > 0.0 { self.asks.insert(price_key(price), (price, size)); }
        }
        self.timestamp = ts;
    }

    /// Apply incremental delta. size == 0 means remove the level.
    pub fn apply_delta(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], ts: i64) {
        for &(price, size) in bids {
            let key = price_key(price);
            if size == 0.0 { self.bids.remove(&key); } else { self.bids.insert(key, (price, size)); }
        }
        for &(price, size) in asks {
            let key = price_key(price);
            if size == 0.0 { self.asks.remove(&key); } else { self.asks.insert(key, (price, size)); }
        }
        self.timestamp = ts;
    }

    pub fn top_bid(&self) -> Option<f64> { self.bids.values().next_back().map(|(p, _)| *p) }
    pub fn top_ask(&self) -> Option<f64> { self.asks.values().next().map(|(p, _)| *p) }
    pub fn is_empty(&self) -> bool { self.bids.is_empty() && self.asks.is_empty() }

    pub fn to_snapshot(&self, depth: usize) -> OrderbookSnapshot {
        OrderbookSnapshot {
            instrument_name: self.instrument_name.clone(),
            bids: self.bids.values().rev().take(depth)
                .map(|&(price, size)| OrderbookLevel { price, size }).collect(),
            asks: self.asks.values().take(depth)
                .map(|&(price, size)| OrderbookLevel { price, size }).collect(),
            timestamp: self.timestamp,
        }
    }
}
