use serde::{Deserialize, Serialize};

// ── Account ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub name: String,
    /// "deribit" | "okx" | "bybit" | "coincall" | "binance" | "mexc" | "hyperliquid" | "uniswap" | "bullish"
    pub exchange: String,
    /// CeFi: API key  |  DeFi: wallet address (public)
    pub api_key: String,
    /// CeFi: API secret  |  DeFi: private key (encrypted)
    pub api_secret: String,
    /// Required for OKX
    #[serde(default)]
    pub passphrase: Option<String>,
    pub testnet: bool,
    #[serde(default = "default_tif")]
    pub default_tif: String,
    #[serde(default)]
    pub default_post_only: bool,
    #[serde(default)]
    pub risk_limit: f64,
    /// Rate limit tier: "tier1" | "tier2" | "vip1" … "vip5" | "market_maker"
    #[serde(default = "default_rate_tier")]
    pub rate_tier: String,
    /// DeFi only: JSON-RPC endpoint URL (e.g. Infura, Alchemy) for on-chain calls
    #[serde(default)]
    pub rpc_url: Option<String>,
    /// DeFi only: EVM chain ID (1=Ethereum, 42161=Arbitrum, 8453=Base, etc.)
    #[serde(default)]
    pub chain_id: Option<u64>,
}

fn default_tif() -> String {
    "good_til_cancelled".to_string()
}

fn default_rate_tier() -> String {
    "tier1".to_string()
}

// ── General Settings ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettings {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_currency")]
    pub default_currency: String,
    #[serde(default = "default_number_locale")]
    pub number_locale: String,
    #[serde(default = "default_price_decimals")]
    pub price_decimals: i32,
    #[serde(default = "default_size_decimals")]
    pub size_decimals: i32,
    #[serde(default = "default_confirm_orders")]
    pub confirm_orders: bool,
    /// Comma-separated list of coins to show in Account Summary (empty = show all)
    #[serde(default)]
    pub watched_coins: String,
    /// Bot/instance ID encoded into client order IDs (default = 1)
    #[serde(default = "default_bot_id")]
    pub bot_id: u16,
    /// How often the Rust backend emits book/ticker events to the frontend (ms).
    /// Lower = more responsive; higher = less CPU/memory pressure. Default: 80ms.
    #[serde(default = "default_book_emit_interval_ms")]
    pub book_emit_interval_ms: u32,
}

fn default_theme() -> String { "dark".to_string() }
fn default_currency() -> String { "BTC".to_string() }
fn default_number_locale() -> String { "en-US".to_string() }
fn default_price_decimals() -> i32 { 2 }
fn default_size_decimals() -> i32 { 4 }
fn default_confirm_orders() -> bool { true }
fn default_bot_id() -> u16 { 1 }
fn default_book_emit_interval_ms() -> u32 { 80 }

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            default_currency: default_currency(),
            number_locale: default_number_locale(),
            price_decimals: default_price_decimals(),
            size_decimals: default_size_decimals(),
            confirm_orders: default_confirm_orders(),
            watched_coins: String::new(),
            bot_id: default_bot_id(),
            book_emit_interval_ms: default_book_emit_interval_ms(),
        }
    }
}

// ── Client Info ─────────────────────────────────────────────────────────────

// ── RFQ / Pricer Settings ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RfqSettings {
    /// Annual risk-free rate as decimal (e.g. 0.05 = 5%). Default 0.05.
    #[serde(default = "rfq_default_rfr")]
    pub risk_free_rate: f64,
    /// Fallback implied vol when market data unavailable (e.g. 0.80 = 80%). Default 0.80.
    #[serde(default = "rfq_default_vol")]
    pub default_vol: f64,
    /// Exchange to fetch spot/index price from: "deribit" | "okx" | "bybit" | "coincall".
    #[serde(default = "rfq_default_source")]
    pub spot_source: String,
    /// Exchange to fetch mark IV from: "deribit" | "okx" | "bybit" | "coincall".
    #[serde(default = "rfq_default_source")]
    pub vol_source: String,
    /// Base half-spread as decimal (e.g. 0.01 = 1% either side of mid). Default 0.01.
    #[serde(default = "rfq_default_base_spread")]
    pub base_spread: f64,
    /// How much portfolio gamma imbalance widens/skews quotes. Default 0.5.
    #[serde(default = "rfq_default_gamma_sens")]
    pub gamma_sensitivity: f64,
    /// How much portfolio vega imbalance widens/skews quotes. Default 0.0005.
    #[serde(default = "rfq_default_vega_sens")]
    pub vega_sensitivity: f64,
    /// Maximum Greek-based skew as decimal (e.g. 0.05 = ±5% of mid). Default 0.05.
    #[serde(default = "rfq_default_max_skew")]
    pub max_skew: f64,
    /// Coin this account trades, used to filter incoming RFQs (e.g. "BTC" or "ETH"). Default "BTC".
    #[serde(default = "rfq_default_coin")]
    pub trading_coin: String,
    /// Automatically price and submit a quote for every new incoming RFQ seek. Default false.
    #[serde(default)]
    pub auto_quote: bool,
    /// Seconds before an auto-submitted quote is automatically cancelled. Default 30.
    #[serde(default = "rfq_default_auto_quote_timeout")]
    pub auto_quote_timeout_secs: u32,
}

fn rfq_default_rfr()              -> f64    { 0.05 }
fn rfq_default_vol()              -> f64    { 0.80 }
fn rfq_default_source()           -> String { "deribit".to_string() }
fn rfq_default_base_spread()      -> f64    { 0.01 }
fn rfq_default_gamma_sens()       -> f64    { 0.5 }
fn rfq_default_vega_sens()        -> f64    { 0.0005 }
fn rfq_default_max_skew()         -> f64    { 0.05 }
fn rfq_default_coin()             -> String { "BTC".to_string() }
fn rfq_default_auto_quote_timeout() -> u32  { 30 }

impl Default for RfqSettings {
    fn default() -> Self {
        Self {
            risk_free_rate:        rfq_default_rfr(),
            default_vol:           rfq_default_vol(),
            spot_source:           rfq_default_source(),
            vol_source:            rfq_default_source(),
            base_spread:           rfq_default_base_spread(),
            gamma_sensitivity:     rfq_default_gamma_sens(),
            vega_sensitivity:      rfq_default_vega_sens(),
            max_skew:              rfq_default_max_skew(),
            trading_coin:          rfq_default_coin(),
            auto_quote:            false,
            auto_quote_timeout_secs: rfq_default_auto_quote_timeout(),
        }
    }
}

// ── Client Info (legacy) ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    #[serde(default)]
    pub company_name: String,
    #[serde(default)]
    pub contact_name: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub telegram_handle: String,
    /// Comma-separated tags
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub notes: String,
}


// ── Tag ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: String,
    pub name: String,
    #[serde(default = "default_tag_color")]
    pub color: String,
}
fn default_tag_color() -> String { "#5087f2".to_string() }

// ── Client (multi-client, replaces single ClientInfo) ──────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Client {
    pub id: String,
    pub company_name: String,
    pub contact_name: String,
    pub phone: String,
    pub email: String,
    /// Comma-separated tag ids
    #[serde(default)]
    pub tag_ids: String,
    #[serde(default)]
    pub notes: String,
}

// ── Client Telegram Chat ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClientTelegramChat {
    pub id: String,
    pub client_id: String,
    pub chat_id: String,
    pub label: String,
}



#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TelegramSettings {
    #[serde(default)]
    pub bot_token: String,
    #[serde(default)]
    pub default_chat_id: String,
}

// ── Telegram Chat Info ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramChat {
    pub id: i64,
    pub kind: String,       // "private" | "group" | "supergroup" | "channel"
    pub title: Option<String>,
    pub username: Option<String>,
}

// ── Telegram Send Result ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramResult {
    pub ok: bool,
    pub message_id: Option<i64>,
    pub error: Option<String>,
}


// ── Orderbook ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderbookLevel {
    pub price: f64,
    pub size:  f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderbookSnapshot {
    pub instrument_name: String,
    pub bids: Vec<OrderbookLevel>,
    pub asks: Vec<OrderbookLevel>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instrument {
    pub instrument_name: String,
    pub kind: String,
    pub base_currency: String,
    pub quote_currency: String,
    pub settlement_currency: String,
    pub is_active: bool,
    pub tick_size: f64,
    pub min_trade_amount: f64,
    /// Quantity increment (lot/step size). Falls back to min_trade_amount when None.
    #[serde(default)]
    pub qty_step: Option<f64>,
    pub contract_size: Option<f64>,
    pub option_type: Option<String>,
    pub strike: Option<f64>,
    pub expiration_timestamp: Option<i64>,
}

// ── Deribit Ticker ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticker {
    pub instrument_name: String,
    pub best_bid_price: Option<f64>,
    pub best_ask_price: Option<f64>,
    pub best_bid_amount: Option<f64>,
    pub best_ask_amount: Option<f64>,
    pub last_price: Option<f64>,
    pub mark_price: Option<f64>,
    /// Spot/cash index price (e.g. Deribit BTC index, Bybit indexPrice)
    pub index_price: Option<f64>,
    /// Forward/underlying price used for BS model (Deribit: underlying_price, Bybit: underlyingPrice, OKX: fwdPx, CoInCall: underlyingPrice)
    pub underlying_price: Option<f64>,
    pub open_interest: Option<f64>,
    pub stats: TickerStats,
    pub mark_iv: Option<f64>,
    pub bid_iv: Option<f64>,
    pub ask_iv: Option<f64>,
    pub delta: Option<f64>,
    pub gamma: Option<f64>,
    pub vega: Option<f64>,
    pub theta: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerStats {
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub price_change: Option<f64>,
    pub volume: Option<f64>,
    pub volume_usd: Option<f64>,
}

// ── Deribit Order ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub order_id: String,
    pub instrument_name: String,
    pub direction: String,
    pub order_type: String,
    pub order_state: String,
    pub price: Option<f64>,
    pub amount: f64,
    pub filled_amount: f64,
    pub average_price: Option<f64>,
    pub post_only: bool,
    pub time_in_force: String,
    pub creation_timestamp: i64,
    pub last_update_timestamp: i64,
}

// ── Trade (filled order) ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trade {
    pub trade_id: String,
    pub account_id: String,
    pub account_name: String,
    pub exchange: String,
    pub instrument_name: String,
    pub direction: String,
    pub amount: f64,
    pub price: f64,
    pub fee: f64,
    pub fee_currency: String,
    pub timestamp: i64,
    pub order_id: String,
}

// ── Transaction Log ────────────────────────────────────────────────────────
//
// Canonical format based on Deribit's `private/get_transaction_log`.
// Bybit and CoInCall fields are mapped to this structure.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionLog {
    /// Exchange-generated unique id for this ledger entry
    pub id: String,
    /// Unix milliseconds
    pub timestamp: i64,
    /// Instrument name (empty for non-trade entries like deposits)
    pub instrument_name: String,
    /// "trade" | "delivery" | "settlement" | "transfer_in" | "transfer_out"
    /// | "deposit" | "withdrawal" | "fee" | "funding" | "option_exercise" | "other"
    pub transaction_type: String,
    /// Exchange-specific category (e.g. Bybit: linear/inverse/option/spot)
    pub category: String,
    /// "buy" | "sell" | "" (empty for non-directional entries)
    pub side: String,
    /// Trade quantity / transfer amount
    pub amount: f64,
    /// Trade price (0 for non-trade entries)
    pub price: f64,
    /// Fee charged for this entry (positive = expense)
    pub fee: f64,
    /// Currency of the fee
    pub fee_currency: String,
    /// Settlement currency of this account (BTC, ETH, USDC, USDT, …)
    pub currency: String,
    /// Realised PnL contributed by this entry (Deribit: profit_as_cashflow; Bybit: cashFlow)
    pub profit_as_cashflow: f64,
    /// Wallet balance after this entry
    pub balance: f64,
    /// Balance change from this entry
    pub change: f64,
    /// Exchange-supplied trade id (empty for non-trade entries)
    pub trade_id: String,
    /// Exchange-supplied order id
    pub order_id: String,
    /// Human-readable extra info / notes
    pub info: String,
    /// Mark price at time of entry (0 if not available)
    pub mark_price: f64,
    /// Index price at time of entry (0 if not available)
    pub index_price: f64,
    /// Account equity after this entry (wallet balance for most exchanges)
    pub equity: f64,
    /// Notional position value at time of entry (price × amount, or exchange-supplied)
    pub position: f64,
    /// Base currency of the instrument (e.g., BTC)
    pub base_currency: String,
    /// Quote currency of the instrument (e.g., USD)
    pub quote_currency: String,
    /// Funding fee for this entry (Bybit: separate funding field; others: 0)
    pub funding: f64,
}

// ── Place Order Request ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceOrderRequest {
    pub account_id: String,
    pub instrument_name: String,
    pub side: String,
    pub order_type: String,
    pub amount: f64,
    pub price: Option<f64>,
    pub time_in_force: Option<String>,
    pub post_only: Option<bool>,
    pub label: Option<String>,
    /// System-assigned client order ID (encoded timestamp + bot_id + machine_hash)
    pub client_order_id: Option<String>,
}

// ── Order Result ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResult {
    pub success: bool,
    pub order: Option<Order>,
    pub error: Option<String>,
}

// ── Account Summary ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub currency: String,
    pub equity: f64,
    pub available_funds: f64,
    pub initial_margin: f64,
    pub maintenance_margin: f64,
    pub unrealized_pl: f64,
}

// ── Position ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub instrument_name: String,
    pub direction: String,  // "long" / "short"
    pub size: f64,
    pub average_price: f64,
    pub mark_price: f64,
    pub mark_iv: f64,       // implied vol (0.0 if not available)
    pub unrealized_pnl: f64,
    pub delta: f64,
    pub gamma: f64,
    pub theta: f64,
    pub vega: f64,
}

// ── Deribit Auth Token ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthToken {
    pub access_token: String,
    pub expires_in: i64,
    pub scope: String,
    pub token_type: String,
}

// ── Telegram Broadcast ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadcastPart {
    pub id: String,
    pub broadcast_id: String,
    /// "text" | "photo" | "document"
    pub part_type: String,
    pub file_path: String,
    pub caption: String,
    pub sort_order: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadcastSend {
    pub id: String,
    pub broadcast_id: String,
    pub part_id: String,
    pub chat_id: i64,
    pub client_name: String,
    /// "pending" | "sent" | "failed"
    pub status: String,
    pub error_msg: String,
    pub message_id: Option<i64>,
    pub attempt_count: i32,
    pub last_attempt: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Broadcast {
    pub id: String,
    pub subject: String,
    pub text_body: String,
    pub parse_mode: String,
    /// "group" | "clients" | "tag"
    pub recip_type: String,
    pub recip_value: String,
    /// "pending" | "sending" | "done" | "partial_fail"
    pub status: String,
    pub created_at: i64,
    /// Summary counts (populated when fetching list)
    #[serde(default)]
    pub total: i32,
    #[serde(default)]
    pub sent: i32,
    #[serde(default)]
    pub failed: i32,
}

/// Request payload to create a broadcast
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBroadcastRequest {
    pub subject: String,
    pub text_body: String,
    pub parse_mode: String,
    pub recip_type: String,
    pub recip_value: String,
    pub attachments: Vec<BroadcastAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadcastAttachment {
    /// "photo" | "document"
    pub kind: String,
    pub file_path: String,
    pub caption: String,
}

// ── Reference Data (canonical cross-exchange instrument model) ─────────────
//
// Symbol format (all uppercase):
//   spot:      BASE-QUOTE                    e.g. BTC-USDT
//   perpetual: BASE-QUOTE-PERPETUAL          e.g. BTC-USD-PERPETUAL
//   future:    BASE-QUOTE-EXPIRY             e.g. BTC-USD-20240329
//   option:    BASE-QUOTE-STRIKE-EXPIRY-C/P  e.g. BTC-USD-50000-20240329-C

/// Per-exchange identifier and market specs for a canonical instrument.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VenueRef {
    pub exchange: String,
    /// Exchange's own symbol name (used for WS subscriptions and order placement).
    pub exchange_symbol: String,
    pub tick_size: f64,
    pub min_trade_amount: f64,
    /// Quantity increment (lot/step size). Always ≥ min_trade_amount.
    pub qty_step: f64,
    pub contract_size: Option<f64>,
    pub settlement_currency: String,
}

/// Canonical instrument shared across exchanges.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceData {
    /// System canonical symbol (our format, uppercase).
    pub symbol: String,
    /// "spot" | "perpetual" | "future" | "option"
    pub kind: String,
    pub base: String,
    pub quote: String,
    pub strike: Option<f64>,
    /// Expiry date as "YYYYMMDD"; None for perpetuals and spot.
    pub expiry: Option<String>,
    /// "C" or "P"; only set for options.
    pub option_type: Option<String>,
    pub is_active: bool,
    /// Per-exchange venue specs (one entry per exchange that lists this instrument).
    pub venues: Vec<VenueRef>,
}

/// Build a canonical symbol string from component parts.
pub fn build_symbol(
    kind: &str,
    base: &str,
    quote: &str,
    strike: Option<f64>,
    expiry: Option<&str>,
    option_type: Option<&str>,
) -> String {
    let b = base.to_uppercase();
    let q = quote.to_uppercase();
    match kind {
        "spot" => format!("{}-{}", b, q),
        "perpetual" => format!("{}-{}-PERPETUAL", b, q),
        "future" => match expiry {
            Some(e) => format!("{}-{}-{}", b, q, e),
            None    => format!("{}-{}-PERPETUAL", b, q),
        },
        "option" => {
            let s = strike.unwrap_or(0.0);
            let e = expiry.unwrap_or("00000000");
            let o = option_type.unwrap_or("C").to_uppercase();
            let strike_str = if s > 0.0 && s == s.floor() {
                format!("{}", s as i64)
            } else {
                format!("{}", s)
            };
            format!("{}-{}-{}-{}-{}", b, q, strike_str, e, o)
        }
        _ => b, // token
    }
}

/// Convert a Unix timestamp (milliseconds) to a "YYYYMMDD" string.
pub fn ts_ms_to_date(ts_ms: i64) -> String {
    let mut rem = (ts_ms / 86_400_000) as i32;
    let mut year = 1970i32;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let days_in_year = if leap { 366 } else { 365 };
        if rem < days_in_year { break; }
        rem -= days_in_year;
        year += 1;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let month_days: [i32; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 0usize;
    while month < 12 && rem >= month_days[month] {
        rem -= month_days[month];
        month += 1;
    }
    format!("{:04}{:02}{:02}", year, month + 1, rem + 1)
}

const FAR_FUTURE_MS: i64 = 4_102_444_800_000; // ~year 2100

fn norm_option_type(s: &str) -> Option<String> {
    match s.to_uppercase().as_str() {
        "C" | "CALL" => Some("C".to_string()),
        "P" | "PUT"  => Some("P".to_string()),
        _ => None,
    }
}

/// Convert an exchange-specific `Instrument` into a canonical `ReferenceData`.
pub fn instrument_to_ref(exchange: &str, inst: &Instrument) -> ReferenceData {
    let kind = match inst.kind.as_str() {
        "spot"  => "spot",
        "option" => "option",
        "perpetual" => "perpetual",
        "future" => match inst.expiration_timestamp {
            None | Some(0)             => "perpetual",
            Some(ts) if ts > FAR_FUTURE_MS => "perpetual",
            _                          => "future",
        },
        _ => "future",
    };

    // Options also need expiry for correct canonical symbols and cascade filtering.
    let expiry = match kind {
        "future" | "option" => inst.expiration_timestamp.map(ts_ms_to_date),
        _ => None,
    };

    let opt_type = inst.option_type.as_deref().and_then(norm_option_type);
    let base  = inst.base_currency.to_uppercase();
    let quote = inst.quote_currency.to_uppercase();

    let symbol = build_symbol(kind, &base, &quote, inst.strike, expiry.as_deref(), opt_type.as_deref());

    let venue = VenueRef {
        exchange: exchange.to_string(),
        exchange_symbol: inst.instrument_name.clone(),
        tick_size: inst.tick_size,
        min_trade_amount: inst.min_trade_amount,
        qty_step: inst.qty_step.unwrap_or(inst.min_trade_amount),
        contract_size: inst.contract_size,
        settlement_currency: inst.settlement_currency.to_uppercase(),
    };

    ReferenceData {
        symbol,
        kind: kind.to_string(),
        base,
        quote,
        strike: inst.strike,
        expiry,
        option_type: opt_type,
        is_active: inst.is_active,
        venues: vec![venue],
    }
}

// ── Market Data Events (emitted by backend WS tasks) ─────────────────────

/// Tauri event: `market://book`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketBookEvent {
    pub symbol: String,
    pub exchange: String,
    pub exchange_symbol: String,
    /// Top bids as [price, size] pairs, best first.
    pub bids: Vec<[f64; 2]>,
    /// Top asks as [price, size] pairs, best first.
    pub asks: Vec<[f64; 2]>,
    pub timestamp: i64,
}

/// Tauri event: `market://ticker`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketTickerEvent {
    pub symbol: String,
    pub exchange: String,
    pub exchange_symbol: String,
    pub last: Option<f64>,
    pub mark: Option<f64>,
    pub index: Option<f64>,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    pub bid_iv: Option<f64>,
    pub ask_iv: Option<f64>,
    pub mark_iv: Option<f64>,
    pub delta: Option<f64>,
    pub gamma: Option<f64>,
    pub vega: Option<f64>,
    pub theta: Option<f64>,
    pub open_interest: Option<f64>,
    pub price_change_24h: Option<f64>,
    pub volume_24h: Option<f64>,
    pub high_24h: Option<f64>,
    pub low_24h: Option<f64>,
    pub timestamp: i64,
}
