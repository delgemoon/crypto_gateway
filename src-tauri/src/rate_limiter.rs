/// Hierarchical Token-Bucket Rate Limiter — Per-Exchange, Per-Account-Tier
///
/// Architecture:
///   - Exchange-scoped groups: `"{exchange}:{public|private}"` (e.g. `"deribit:private"`)
///   - Endpoint sub-limits within each group (e.g. `"place_order"`, `"cancel_order"`)
///   - Per-account token buckets sized by the account's VIP/fee tier
///
/// Usage:
/// ```
/// // Setup
/// let limiter = default_rate_limiter();
/// limiter.configure_account("acc_id", "deribit", "tier1");
///
/// // On each API call
/// limiter.check_account("deribit", "private", "place_order", "acc_id", 1.0)?;
/// ```
///
/// Exchange tier reference (req/s per account):
///
/// | Tier        | Deribit | Bybit orders | OKX orders | CoInCall |
/// |-------------|---------|--------------|------------|----------|
/// | tier1       |  10     |  10          |  20        |  10      |
/// | tier2       |  20     |  20          |  40        |  20      |
/// | vip1        |  50     |  30          |  60        |  30      |
/// | vip2        | 100     |  40          |  90        |  50      |
/// | vip3        | 200     |  50          | 150        |  80      |
/// | vip4        | 500     |  60          | 200        | 100      |
/// | vip5        |1000     | 100          | 300        | 150      |
/// | market_maker| 2000    | 100          | 300        | 200      |

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use serde::Serialize;

// ── Tier definitions ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RateTier {
    Tier1,
    Tier2,
    Vip1,
    Vip2,
    Vip3,
    Vip4,
    Vip5,
    MarketMaker,
}

impl RateTier {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "tier2"        => Self::Tier2,
            "vip1"         => Self::Vip1,
            "vip2"         => Self::Vip2,
            "vip3"         => Self::Vip3,
            "vip4"         => Self::Vip4,
            "vip5"         => Self::Vip5,
            "market_maker" | "mm" => Self::MarketMaker,
            _              => Self::Tier1, // default
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tier1       => "tier1",
            Self::Tier2       => "tier2",
            Self::Vip1        => "vip1",
            Self::Vip2        => "vip2",
            Self::Vip3        => "vip3",
            Self::Vip4        => "vip4",
            Self::Vip5        => "vip5",
            Self::MarketMaker => "market_maker",
        }
    }
}

/// Per-account burst/sustained rate (requests per second).
#[derive(Debug, Clone)]
pub struct AccountLimits {
    /// Burst capacity (max tokens)
    pub burst: f64,
    /// Sustained rate (tokens/sec)
    pub sustained: f64,
    /// Order placement rate (requests/sec)
    pub order_rps: f64,
    /// Cancel rate (requests/sec)
    pub cancel_rps: f64,
}

/// Return per-account limits for a given exchange + tier.
pub fn tier_limits(exchange: &str, tier: &RateTier) -> AccountLimits {
    match exchange {
        "deribit" => match tier {
            RateTier::Tier1       => AccountLimits { burst: 20.0,   sustained: 10.0,  order_rps: 10.0,  cancel_rps: 10.0  },
            RateTier::Tier2       => AccountLimits { burst: 40.0,   sustained: 20.0,  order_rps: 20.0,  cancel_rps: 20.0  },
            RateTier::Vip1        => AccountLimits { burst: 100.0,  sustained: 50.0,  order_rps: 50.0,  cancel_rps: 50.0  },
            RateTier::Vip2        => AccountLimits { burst: 200.0,  sustained: 100.0, order_rps: 100.0, cancel_rps: 100.0 },
            RateTier::Vip3        => AccountLimits { burst: 400.0,  sustained: 200.0, order_rps: 200.0, cancel_rps: 200.0 },
            RateTier::Vip4        => AccountLimits { burst: 800.0,  sustained: 500.0, order_rps: 500.0, cancel_rps: 500.0 },
            RateTier::Vip5        => AccountLimits { burst: 1000.0, sustained: 700.0, order_rps: 700.0, cancel_rps: 700.0 },
            RateTier::MarketMaker => AccountLimits { burst: 2000.0, sustained: 2000.0,order_rps: 2000.0,cancel_rps: 2000.0},
        },
        "bybit" => match tier {
            RateTier::Tier1       => AccountLimits { burst: 20.0,  sustained: 10.0,  order_rps: 10.0,  cancel_rps: 10.0  },
            RateTier::Tier2       => AccountLimits { burst: 40.0,  sustained: 20.0,  order_rps: 20.0,  cancel_rps: 20.0  },
            RateTier::Vip1        => AccountLimits { burst: 60.0,  sustained: 30.0,  order_rps: 30.0,  cancel_rps: 30.0  },
            RateTier::Vip2        => AccountLimits { burst: 80.0,  sustained: 40.0,  order_rps: 40.0,  cancel_rps: 40.0  },
            RateTier::Vip3        => AccountLimits { burst: 100.0, sustained: 50.0,  order_rps: 50.0,  cancel_rps: 50.0  },
            RateTier::Vip4        => AccountLimits { burst: 120.0, sustained: 60.0,  order_rps: 60.0,  cancel_rps: 60.0  },
            RateTier::Vip5        => AccountLimits { burst: 200.0, sustained: 100.0, order_rps: 100.0, cancel_rps: 100.0 },
            RateTier::MarketMaker => AccountLimits { burst: 400.0, sustained: 200.0, order_rps: 200.0, cancel_rps: 200.0 },
        },
        "okx" => match tier {
            RateTier::Tier1       => AccountLimits { burst: 40.0,  sustained: 20.0,  order_rps: 20.0,  cancel_rps: 20.0  },
            RateTier::Tier2       => AccountLimits { burst: 80.0,  sustained: 40.0,  order_rps: 40.0,  cancel_rps: 40.0  },
            RateTier::Vip1        => AccountLimits { burst: 120.0, sustained: 60.0,  order_rps: 60.0,  cancel_rps: 60.0  },
            RateTier::Vip2        => AccountLimits { burst: 180.0, sustained: 90.0,  order_rps: 90.0,  cancel_rps: 90.0  },
            RateTier::Vip3        => AccountLimits { burst: 300.0, sustained: 150.0, order_rps: 150.0, cancel_rps: 150.0 },
            RateTier::Vip4        => AccountLimits { burst: 400.0, sustained: 200.0, order_rps: 200.0, cancel_rps: 200.0 },
            RateTier::Vip5        => AccountLimits { burst: 600.0, sustained: 300.0, order_rps: 300.0, cancel_rps: 300.0 },
            RateTier::MarketMaker => AccountLimits { burst: 600.0, sustained: 300.0, order_rps: 300.0, cancel_rps: 300.0 },
        },
        "coincall" => match tier {
            RateTier::Tier1       => AccountLimits { burst: 20.0,  sustained: 10.0,  order_rps: 10.0,  cancel_rps: 10.0  },
            RateTier::Tier2       => AccountLimits { burst: 40.0,  sustained: 20.0,  order_rps: 20.0,  cancel_rps: 20.0  },
            RateTier::Vip1        => AccountLimits { burst: 60.0,  sustained: 30.0,  order_rps: 30.0,  cancel_rps: 30.0  },
            RateTier::Vip2        => AccountLimits { burst: 100.0, sustained: 50.0,  order_rps: 50.0,  cancel_rps: 50.0  },
            RateTier::Vip3        => AccountLimits { burst: 160.0, sustained: 80.0,  order_rps: 80.0,  cancel_rps: 80.0  },
            RateTier::Vip4        => AccountLimits { burst: 200.0, sustained: 100.0, order_rps: 100.0, cancel_rps: 100.0 },
            RateTier::Vip5        => AccountLimits { burst: 300.0, sustained: 150.0, order_rps: 150.0, cancel_rps: 150.0 },
            RateTier::MarketMaker => AccountLimits { burst: 400.0, sustained: 200.0, order_rps: 200.0, cancel_rps: 200.0 },
        },
        "binance" => match tier {
            RateTier::Tier1       => AccountLimits { burst: 100.0, sustained: 50.0,  order_rps: 10.0,  cancel_rps: 10.0  },
            RateTier::Tier2       => AccountLimits { burst: 200.0, sustained: 100.0, order_rps: 20.0,  cancel_rps: 20.0  },
            RateTier::Vip1        => AccountLimits { burst: 400.0, sustained: 200.0, order_rps: 50.0,  cancel_rps: 50.0  },
            RateTier::Vip2        => AccountLimits { burst: 600.0, sustained: 300.0, order_rps: 100.0, cancel_rps: 100.0 },
            RateTier::Vip3        => AccountLimits { burst: 800.0, sustained: 400.0, order_rps: 150.0, cancel_rps: 150.0 },
            RateTier::Vip4        => AccountLimits { burst: 1000.0,sustained: 500.0, order_rps: 200.0, cancel_rps: 200.0 },
            RateTier::Vip5        => AccountLimits { burst: 1200.0,sustained: 600.0, order_rps: 300.0, cancel_rps: 300.0 },
            RateTier::MarketMaker => AccountLimits { burst: 2000.0,sustained: 1000.0,order_rps: 500.0, cancel_rps: 500.0 },
        },
        "mexc" => match tier {
            RateTier::Tier1       => AccountLimits { burst: 20.0,  sustained: 10.0,  order_rps: 5.0,   cancel_rps: 5.0   },
            RateTier::Tier2       => AccountLimits { burst: 40.0,  sustained: 20.0,  order_rps: 10.0,  cancel_rps: 10.0  },
            RateTier::Vip1        => AccountLimits { burst: 80.0,  sustained: 40.0,  order_rps: 20.0,  cancel_rps: 20.0  },
            RateTier::Vip2        => AccountLimits { burst: 120.0, sustained: 60.0,  order_rps: 30.0,  cancel_rps: 30.0  },
            RateTier::Vip3        => AccountLimits { burst: 200.0, sustained: 100.0, order_rps: 50.0,  cancel_rps: 50.0  },
            RateTier::Vip4        => AccountLimits { burst: 300.0, sustained: 150.0, order_rps: 75.0,  cancel_rps: 75.0  },
            RateTier::Vip5        => AccountLimits { burst: 400.0, sustained: 200.0, order_rps: 100.0, cancel_rps: 100.0 },
            RateTier::MarketMaker => AccountLimits { burst: 600.0, sustained: 300.0, order_rps: 150.0, cancel_rps: 150.0 },
        },
        // Hyperliquid: moderate rate limits (no official published tiers)
        "hyperliquid" => match tier {
            RateTier::Tier1       => AccountLimits { burst: 20.0,  sustained: 10.0,  order_rps: 5.0,   cancel_rps: 5.0   },
            RateTier::Tier2       => AccountLimits { burst: 40.0,  sustained: 20.0,  order_rps: 10.0,  cancel_rps: 10.0  },
            RateTier::MarketMaker => AccountLimits { burst: 200.0, sustained: 100.0, order_rps: 100.0, cancel_rps: 100.0 },
            _                     => AccountLimits { burst: 60.0,  sustained: 30.0,  order_rps: 15.0,  cancel_rps: 15.0  },
        },
        // Uniswap: limited by RPC provider
        "uniswap" => match tier {
            _                     => AccountLimits { burst: 10.0,  sustained: 5.0,   order_rps: 2.0,   cancel_rps: 0.0   },
        },
        // Generic fallback for unknown exchanges
        _ => match tier {
            RateTier::Tier1       => AccountLimits { burst: 10.0,  sustained: 5.0,   order_rps: 5.0,   cancel_rps: 5.0   },
            RateTier::Tier2       => AccountLimits { burst: 20.0,  sustained: 10.0,  order_rps: 10.0,  cancel_rps: 10.0  },
            _                     => AccountLimits { burst: 50.0,  sustained: 25.0,  order_rps: 25.0,  cancel_rps: 25.0  },
        },
    }
}

// ── Config types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BucketConfig {
    pub capacity: f64,
    pub refill_per_ms: f64,
}

impl BucketConfig {
    pub fn per_second(rps: f64) -> Self {
        Self { capacity: rps, refill_per_ms: rps / 1000.0 }
    }
    pub fn burst(burst: f64, rps: f64) -> Self {
        Self { capacity: burst, refill_per_ms: rps / 1000.0 }
    }
}

// ── Token bucket ───────────────────────────────────────────────────────────

#[derive(Debug)]
struct TokenBucket {
    config: BucketConfig,
    tokens: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(config: BucketConfig) -> Self {
        let tokens = config.capacity;
        Self { config, tokens, last_refill: Instant::now() }
    }

    fn try_consume(&mut self, cost: f64) -> bool {
        let elapsed_ms = self.last_refill.elapsed().as_secs_f64() * 1000.0;
        self.tokens = (self.tokens + elapsed_ms * self.config.refill_per_ms)
            .min(self.config.capacity);
        self.last_refill = Instant::now();
        if self.tokens >= cost {
            self.tokens -= cost;
            true
        } else {
            false
        }
    }

    fn available(&self) -> f64 {
        let elapsed_ms = self.last_refill.elapsed().as_secs_f64() * 1000.0;
        (self.tokens + elapsed_ms * self.config.refill_per_ms).min(self.config.capacity)
    }

    fn refund(&mut self, cost: f64) {
        self.tokens = (self.tokens + cost).min(self.config.capacity);
    }
}

// ── Rate limit error ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RateLimitError {
    pub level: &'static str,
    pub name: String,
    pub available: f64,
    pub required: f64,
}

impl std::fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Rate limited at {} '{}': {:.1}/{:.1} tokens available",
            self.level, self.name, self.available, self.required)
    }
}

// ── Per-account bucket set ─────────────────────────────────────────────────

struct AccountBuckets {
    exchange: String,
    tier: String,
    /// General private endpoint bucket
    general: TokenBucket,
    /// Order placement bucket
    order: TokenBucket,
    /// Order cancellation bucket
    cancel: TokenBucket,
}

impl AccountBuckets {
    fn new(exchange: &str, tier: &RateTier) -> Self {
        let limits = tier_limits(exchange, tier);
        Self {
            exchange: exchange.to_string(),
            tier: tier.as_str().to_string(),
            general: TokenBucket::new(BucketConfig::burst(limits.burst, limits.sustained)),
            order:   TokenBucket::new(BucketConfig::per_second(limits.order_rps)),
            cancel:  TokenBucket::new(BucketConfig::per_second(limits.cancel_rps)),
        }
    }

    fn check(&mut self, endpoint: &str, cost: f64) -> Result<(), RateLimitError> {
        // Always check general budget first
        let avail = self.general.available();
        if !self.general.try_consume(cost) {
            return Err(RateLimitError {
                level: "account",
                name: format!("{}:general", self.exchange),
                available: avail,
                required: cost,
            });
        }

        // Then check endpoint-specific bucket
        match endpoint {
            "place_order" | "batch_order" => {
                let avail = self.order.available();
                if !self.order.try_consume(cost) {
                    self.general.refund(cost);
                    return Err(RateLimitError {
                        level: "account_endpoint",
                        name: format!("{}:order", self.exchange),
                        available: avail,
                        required: cost,
                    });
                }
            }
            "cancel_order" | "cancel_all" => {
                let avail = self.cancel.available();
                if !self.cancel.try_consume(cost) {
                    self.general.refund(cost);
                    return Err(RateLimitError {
                        level: "account_endpoint",
                        name: format!("{}:cancel", self.exchange),
                        available: avail,
                        required: cost,
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }
}

// ── Exchange-level group ───────────────────────────────────────────────────

struct ExchangeGroup {
    bucket: TokenBucket,
    endpoints: HashMap<String, TokenBucket>,
}

impl ExchangeGroup {
    fn new(config: BucketConfig) -> Self {
        Self { bucket: TokenBucket::new(config), endpoints: HashMap::new() }
    }
}

// ── RateLimiter ────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<RateLimiterInner>>,
}

struct RateLimiterInner {
    /// "{exchange}:{public|private}" → exchange-level group bucket + endpoint buckets
    groups: HashMap<String, ExchangeGroup>,
    /// account_id → per-account tier buckets
    accounts: HashMap<String, AccountBuckets>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(RateLimiterInner {
            groups: HashMap::new(),
            accounts: HashMap::new(),
        }))}
    }

    // ── Setup ──────────────────────────────────────────────────────────

    pub fn add_group(&self, group: &str, config: BucketConfig) {
        let mut inner = self.inner.lock().unwrap();
        let entry = inner.groups.entry(group.to_string())
            .or_insert_with(|| ExchangeGroup::new(config.clone()));
        entry.bucket = TokenBucket::new(config);
    }

    pub fn add_endpoint(&self, group: &str, endpoint: &str, config: BucketConfig) {
        let mut inner = self.inner.lock().unwrap();
        let g = inner.groups.entry(group.to_string())
            .or_insert_with(|| ExchangeGroup::new(BucketConfig::per_second(f64::MAX)));
        g.endpoints.insert(endpoint.to_string(), TokenBucket::new(config));
    }

    /// Register (or update) per-account tier limits.
    /// Call this after loading accounts from DB, and whenever tier changes.
    pub fn configure_account(&self, account_id: &str, exchange: &str, tier_str: &str) {
        let tier = RateTier::from_str(tier_str);
        let mut inner = self.inner.lock().unwrap();
        inner.accounts.insert(account_id.to_string(), AccountBuckets::new(exchange, &tier));
    }

    /// Remove account buckets (call when account is deleted).
    pub fn remove_account(&self, account_id: &str) {
        self.inner.lock().unwrap().accounts.remove(account_id);
    }

    // ── Checking ───────────────────────────────────────────────────────

    /// Full three-level check: exchange group → endpoint → per-account tier.
    ///
    /// - `exchange`: "deribit" | "bybit" | "okx" | "coincall"
    /// - `group`:    "public" | "private"
    /// - `endpoint`: "place_order" | "cancel_order" | "orderbook" | "" (skip endpoint)
    /// - `account_id`: account identifier (pass "" to skip per-account check)
    /// - `cost`:     token cost (usually 1.0)
    pub fn check_account(
        &self,
        exchange: &str,
        group: &str,
        endpoint: &str,
        account_id: &str,
        cost: f64,
    ) -> Result<(), RateLimitError> {
        let group_key = format!("{}:{}", exchange, group);
        let mut inner = self.inner.lock().unwrap();

        // Level 1 + 2: exchange-level group + endpoint
        if let Some(g) = inner.groups.get_mut(&group_key) {
            let avail = g.bucket.available();
            if !g.bucket.try_consume(cost) {
                return Err(RateLimitError {
                    level: "exchange_group",
                    name: group_key,
                    available: avail,
                    required: cost,
                });
            }

            if !endpoint.is_empty() {
                if let Some(ep) = g.endpoints.get_mut(endpoint) {
                    let avail = ep.available();
                    if !ep.try_consume(cost) {
                        g.bucket.refund(cost);
                        return Err(RateLimitError {
                            level: "exchange_endpoint",
                            name: format!("{}:{}", group_key, endpoint),
                            available: avail,
                            required: cost,
                        });
                    }
                }
            }
        }

        // Level 3: per-account tier bucket
        if !account_id.is_empty() {
            if let Some(acc) = inner.accounts.get_mut(account_id) {
                if let Err(e) = acc.check(endpoint, cost) {
                    // Refund exchange-level tokens
                    let group_key2 = format!("{}:{}", exchange, group);
                    if let Some(g) = inner.groups.get_mut(&group_key2) {
                        g.bucket.refund(cost);
                        if !endpoint.is_empty() {
                            if let Some(ep) = g.endpoints.get_mut(endpoint) {
                                ep.refund(cost);
                            }
                        }
                    }
                    return Err(e);
                }
            }
            // If no bucket configured for this account, allow through (graceful degradation)
        }

        Ok(())
    }

    /// Legacy check API — maps to check_account with no exchange scope.
    /// Kept for backward compat with existing code and tests.
    pub fn check(
        &self,
        group: &str,
        endpoint: &str,
        _key_class: &str,
        _key: &str,
        cost: f64,
    ) -> Result<(), RateLimitError> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(g) = inner.groups.get_mut(group) {
            let avail = g.bucket.available();
            if !g.bucket.try_consume(cost) {
                return Err(RateLimitError {
                    level: "group",
                    name: group.to_string(),
                    available: avail,
                    required: cost,
                });
            }
            if !endpoint.is_empty() {
                if let Some(ep) = g.endpoints.get_mut(endpoint) {
                    let avail = ep.available();
                    if !ep.try_consume(cost) {
                        g.bucket.refund(cost);
                        return Err(RateLimitError {
                            level: "endpoint",
                            name: endpoint.to_string(),
                            available: avail,
                            required: cost,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    pub fn check_group_endpoint(&self, group: &str, endpoint: &str) -> Result<(), RateLimitError> {
        self.check(group, endpoint, "", "", 1.0)
    }

    // ── Status ─────────────────────────────────────────────────────────

    pub fn status(&self) -> RateLimiterStatus {
        let inner = self.inner.lock().unwrap();
        let groups = inner.groups.iter().map(|(name, g)| {
            let endpoints = g.endpoints.iter()
                .map(|(ep, b)| (ep.clone(), b.available()))
                .collect();
            GroupStatus {
                name: name.clone(),
                available: g.bucket.available(),
                capacity: g.bucket.config.capacity,
                endpoints,
            }
        }).collect();

        let accounts = inner.accounts.iter().map(|(id, acc)| {
            AccountStatus {
                account_id:         id.clone(),
                exchange:           acc.exchange.clone(),
                tier:               acc.tier.clone(),
                general_available:  acc.general.available(),
                general_capacity:   acc.general.config.capacity,
                order_available:    acc.order.available(),
                order_capacity:     acc.order.config.capacity,
                cancel_available:   acc.cancel.available(),
                cancel_capacity:    acc.cancel.config.capacity,
            }
        }).collect();

        RateLimiterStatus { groups, accounts }
    }
}

impl Default for RateLimiter {
    fn default() -> Self { Self::new() }
}

// ── Status types (serializable for frontend) ──────────────────────────────

#[derive(Debug, Serialize)]
pub struct GroupStatus {
    pub name: String,
    pub available: f64,
    pub capacity: f64,
    pub endpoints: HashMap<String, f64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub account_id: String,
    pub exchange: String,
    pub tier: String,
    pub general_available: f64,
    pub general_capacity: f64,
    pub order_available: f64,
    pub order_capacity: f64,
    pub cancel_available: f64,
    pub cancel_capacity: f64,
}

#[derive(Debug, Serialize)]
pub struct RateLimiterStatus {
    pub groups: Vec<GroupStatus>,
    pub accounts: Vec<AccountStatus>,
}

// ── Default application rate limit config ─────────────────────────────────

/// Build a rate limiter with exchange-scoped groups.
/// Call `configure_account()` separately for each account after loading from DB.
pub fn default_rate_limiter() -> RateLimiter {
    let rl = RateLimiter::new();

    for exchange in &["deribit", "bybit", "okx", "coincall"] {
        // Public endpoints — generous exchange-wide budget
        let pub_rps = match *exchange {
            "deribit"  => 1000.0,
            "bybit"    =>  600.0,
            "okx"      => 1000.0,
            "coincall" =>  300.0,
            _          =>  200.0,
        };
        let pub_key = format!("{}:public", exchange);
        rl.add_group(&pub_key, BucketConfig::per_second(pub_rps));
        rl.add_endpoint(&pub_key, "orderbook",   BucketConfig::per_second(pub_rps * 0.5));
        rl.add_endpoint(&pub_key, "instruments", BucketConfig::per_second(pub_rps * 0.2));
        rl.add_endpoint(&pub_key, "ticker",      BucketConfig::per_second(pub_rps * 0.3));

        // Private endpoints — exchange-wide shared budget
        let priv_rps = match *exchange {
            "deribit"  => 2000.0, // Deribit's overall private limit is high; per-account handles the real cap
            "bybit"    =>  600.0,
            "okx"      =>  600.0,
            "coincall" =>  200.0,
            _          =>  200.0,
        };
        let priv_key = format!("{}:private", exchange);
        rl.add_group(&priv_key, BucketConfig::per_second(priv_rps));
        rl.add_endpoint(&priv_key, "place_order",   BucketConfig::per_second(priv_rps * 0.5));
        rl.add_endpoint(&priv_key, "cancel_order",  BucketConfig::per_second(priv_rps * 0.5));
        rl.add_endpoint(&priv_key, "open_orders",   BucketConfig::per_second(priv_rps * 0.3));
        rl.add_endpoint(&priv_key, "trade_history", BucketConfig::per_second(priv_rps * 0.2));
        rl.add_endpoint(&priv_key, "positions",     BucketConfig::per_second(priv_rps * 0.3));
        rl.add_endpoint(&priv_key, "account",       BucketConfig::per_second(priv_rps * 0.1));
    }

    rl
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_limits_vary_by_exchange() {
        let d1 = tier_limits("deribit", &RateTier::Tier1);
        let dv5 = tier_limits("deribit", &RateTier::Vip5);
        assert!(dv5.order_rps > d1.order_rps);

        let b1 = tier_limits("bybit", &RateTier::Tier1);
        let bv5 = tier_limits("bybit", &RateTier::Vip5);
        assert!(bv5.order_rps > b1.order_rps);
    }

    #[test]
    fn exchange_group_enforced() {
        let rl = RateLimiter::new();
        rl.add_group("deribit:private", BucketConfig { capacity: 3.0, refill_per_ms: 0.0 });
        assert!(rl.check_account("deribit", "private", "", "", 1.0).is_ok());
        assert!(rl.check_account("deribit", "private", "", "", 1.0).is_ok());
        assert!(rl.check_account("deribit", "private", "", "", 1.0).is_ok());
        assert!(rl.check_account("deribit", "private", "", "", 1.0).is_err()); // 4th should fail
    }

    #[test]
    fn per_account_tier_limit() {
        let rl = RateLimiter::new();
        rl.add_group("deribit:private", BucketConfig { capacity: 1000.0, refill_per_ms: 0.0 });
        rl.configure_account("acc1", "deribit", "tier1");
        // Tier1 deribit: order_rps=10 => order bucket capacity=10
        for _ in 0..10 {
            assert!(rl.check_account("deribit", "private", "place_order", "acc1", 1.0).is_ok());
        }
        // 11th should fail on account_endpoint (order bucket exhausted)
        let err = rl.check_account("deribit", "private", "place_order", "acc1", 1.0).unwrap_err();
        assert!(err.level == "account" || err.level == "account_endpoint");
    }

    #[test]
    fn vip_tier_higher_limit() {
        let rl = RateLimiter::new();
        rl.add_group("okx:private", BucketConfig { capacity: 10000.0, refill_per_ms: 0.0 });
        rl.configure_account("vip_acc", "okx", "vip5");
        // Vip5 okx: order_rps=300 => order bucket capacity=300
        let mut ok_count = 0;
        for _ in 0..400 {
            if rl.check_account("okx", "private", "place_order", "vip_acc", 1.0).is_ok() {
                ok_count += 1;
            }
        }
        assert_eq!(ok_count, 300);
    }

    #[test]
    fn legacy_check_still_works() {
        let rl = RateLimiter::new();
        rl.add_group("private", BucketConfig { capacity: 3.0, refill_per_ms: 0.0 });
        assert!(rl.check("private", "", "per_account", "acc1", 1.0).is_ok());
        assert!(rl.check("private", "", "per_account", "acc1", 1.0).is_ok());
        assert!(rl.check("private", "", "per_account", "acc1", 1.0).is_ok());
        assert!(rl.check("private", "", "per_account", "acc1", 1.0).is_err());
    }
}
