//! Bullish exchange REST API client.
//!
//! Auth flow (HMAC):
//!   1. `GET /trading-api/v1/users/hmac/login` signed with HMAC-SHA256
//!      → returns a JWT token (valid ~30 min, cached per api_key)
//!   2. All private GET requests use `Authorization: Bearer {jwt}`
//!   3. Private POST/DELETE also include `BX-SIGNATURE`, `BX-TIMESTAMP`, `BX-NONCE`
//!      Signature: HMAC-SHA256(secret, SHA256(ts + nonce + METHOD + path + body))
//!
//! `api_key`    → BX-PUBLIC-KEY
//! `api_secret` → HMAC secret key
//! `passphrase` → trading account ID (optional; fetched automatically if omitted)

use reqwest::{Client, header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE}};
use serde_json::{json, Value};
use hmac::{Hmac, Mac};
use sha2::{Sha256, Digest};
use std::collections::HashMap;
use std::sync::{OnceLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::models::{
    Account, AccountSummary, Instrument, Order, OrderResult, OrderbookLevel,
    OrderbookSnapshot, PlaceOrderRequest, Position, Ticker, TickerStats, Trade,
    TransactionLog,
};

type HmacSha256 = Hmac<Sha256>;

pub const BASE: &str = "https://api.exchange.bullish.com";

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn now_us() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_micros() as u64
}

fn pf(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn pf0(v: &Value) -> f64 { pf(v).unwrap_or(0.0) }

// ── JWT Token Cache ────────────────────────────────────────────────────────

struct TokenEntry {
    token:        String,
    expires_at_ms: u64,
}

static TOKEN_CACHE: OnceLock<Mutex<HashMap<String, TokenEntry>>> = OnceLock::new();

fn token_cache() -> &'static Mutex<HashMap<String, TokenEntry>> {
    TOKEN_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Public alias so `ws/bullish.rs` can reuse the cached JWT.
pub async fn get_jwt_pub(api_key: &str, api_secret: &str) -> Result<String, String> {
    get_jwt(api_key, api_secret).await
}

async fn get_jwt(api_key: &str, api_secret: &str) -> Result<String, String> {
    {
        let cache = token_cache().lock().unwrap();
        if let Some(e) = cache.get(api_key) {
            if e.expires_at_ms > now_ms() + 60_000 {
                return Ok(e.token.clone());
            }
        }
    }

    let ts    = now_ms().to_string();
    let nonce = now_us().to_string();
    let path  = "/trading-api/v1/users/hmac/login";
    let msg   = format!("{}{}{}{}", ts, nonce, "GET", path);

    let mut mac = HmacSha256::new_from_slice(api_secret.as_bytes()).map_err(|e| e.to_string())?;
    mac.update(msg.as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());

    let resp: Value = Client::new()
        .get(format!("{}{}", BASE, path))
        .header("BX-PUBLIC-KEY", api_key)
        .header("BX-NONCE",      &nonce)
        .header("BX-SIGNATURE",  &sig)
        .header("BX-TIMESTAMP",  &ts)
        .header(CONTENT_TYPE, "application/json")
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    let token = resp["token"].as_str()
        .ok_or_else(|| format!("Bullish login failed: {}", resp))?
        .to_string();

    let expires_at_ms = resp["expireTime"].as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp_millis() as u64)
        .unwrap_or_else(|| now_ms() + 29 * 60 * 1000);

    token_cache().lock().unwrap().insert(api_key.to_string(), TokenEntry { token: token.clone(), expires_at_ms });
    Ok(token)
}

/// Sign a private request. Uses double-hash: SHA256(payload) → HMAC-SHA256(secret, hex_digest).
fn sign(ts: &str, nonce: &str, method: &str, path: &str, body: &str, secret: &str) -> Result<String, String> {
    let payload = format!("{}{}{}{}{}", ts, nonce, method, path, body);
    let hash = Sha256::digest(payload.as_bytes());
    let hash_hex = hex::encode(hash);
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).map_err(|e| e.to_string())?;
    mac.update(hash_hex.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

fn auth_headers(jwt: &str, sig: &str, ts: &str, nonce: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(AUTHORIZATION,  HeaderValue::from_str(&format!("Bearer {}", jwt)).unwrap_or_else(|_| HeaderValue::from_static("")));
    h.insert(CONTENT_TYPE,   HeaderValue::from_static("application/json"));
    h.insert("BX-SIGNATURE", HeaderValue::from_str(sig).unwrap_or_else(|_| HeaderValue::from_static("")));
    h.insert("BX-TIMESTAMP", HeaderValue::from_str(ts).unwrap_or_else(|_| HeaderValue::from_static("")));
    h.insert("BX-NONCE",     HeaderValue::from_str(nonce).unwrap_or_else(|_| HeaderValue::from_static("")));
    h
}

async fn get_trading_account_id(account: &Account) -> Result<String, String> {
    if let Some(p) = &account.passphrase {
        let p = p.trim();
        if !p.is_empty() { return Ok(p.to_string()); }
    }

    let jwt = get_jwt(&account.api_key, &account.api_secret).await?;
    let resp: Value = Client::new()
        .get(format!("{}/trading-api/v1/accounts/trading-accounts", BASE))
        .header(AUTHORIZATION, format!("Bearer {}", jwt))
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    resp.as_array()
        .and_then(|a| a.first())
        .and_then(|a| a["tradingAccountId"].as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No Bullish trading accounts found".to_string())
}

// ── Public Endpoints ───────────────────────────────────────────────────────

pub async fn fetch_instruments(currency: &str, kind: &str) -> Result<Vec<Instrument>, String> {
    let resp: Value = Client::new()
        .get(format!("{}/trading-api/v1/markets", BASE))
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    let arr = resp.as_array().ok_or("Expected array from /markets")?;
    let cur_upper = currency.to_uppercase();

    let mut instruments = Vec::new();
    for m in arr {
        if m["marketEnabled"].as_bool() != Some(true) { continue; }
        if m["createOrderEnabled"].as_bool() != Some(true) { continue; }

        let base  = m["baseSymbol"].as_str().unwrap_or("").to_uppercase();
        let quote = m["quoteSymbol"].as_str().unwrap_or("").to_uppercase();

        if !cur_upper.is_empty() && base != cur_upper && quote != cur_upper { continue; }

        // Bullish is spot-only. Honour `kind` filter: accept empty, "spot", or "future" (catch-all).
        if !kind.is_empty() && kind != "spot" && kind != "future" { continue; }

        let tick_size = m["tickSize"].as_str()
            .and_then(|s| s.trim().parse::<f64>().ok()).unwrap_or(0.01);
        let min_qty = m["minQuantityLimit"].as_str()
            .and_then(|s| s.trim().parse::<f64>().ok()).unwrap_or(0.0);

        instruments.push(Instrument {
            instrument_name:      m["symbol"].as_str().unwrap_or("").to_string(),
            kind:                 "spot".to_string(),
            base_currency:        base.clone(),
            quote_currency:       quote.clone(),
            settlement_currency:  quote,
            is_active:            true,
            tick_size,
            min_trade_amount:     min_qty,
            qty_step:             Some(min_qty),
            contract_size:        Some(1.0),
            option_type:          None,
            strike:               None,
            expiration_timestamp: None,
        });
    }
    Ok(instruments)
}

pub async fn fetch_ticker(instrument_name: &str) -> Result<Ticker, String> {
    let t: Value = Client::new()
        .get(format!("{}/trading-api/v1/markets/{}/tick", BASE, instrument_name))
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    Ok(Ticker {
        instrument_name:  instrument_name.to_string(),
        best_bid_price:   pf(&t["bestBid"]),
        best_ask_price:   pf(&t["bestAsk"]),
        best_bid_amount:  pf(&t["bidVolume"]),
        best_ask_amount:  pf(&t["askVolume"]),
        last_price:       pf(&t["last"]),
        mark_price:       pf(&t["currentPrice"]),
        index_price:      pf(&t["currentPrice"]),
        underlying_price: None,
        open_interest:    None,
        stats: TickerStats {
            high:         pf(&t["high"]),
            low:          pf(&t["low"]),
            price_change: pf(&t["change"]),
            volume:       pf(&t["baseVolume"]),
            volume_usd:   pf(&t["quoteVolume"]),
        },
        mark_iv: None, bid_iv: None, ask_iv: None,
        delta: None, gamma: None, theta: None, vega: None,
    })
}

pub async fn fetch_orderbook(instrument_name: &str, depth: u32) -> Result<OrderbookSnapshot, String> {
    let url = format!(
        "{}/trading-api/v1/markets/{}/orderbook/hybrid?depth={}",
        BASE, instrument_name, depth
    );
    let ob: Value = Client::new().get(&url)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    let parse = |arr: &Value| -> Vec<OrderbookLevel> {
        arr.as_array().unwrap_or(&vec![]).iter().take(depth as usize).filter_map(|l| {
            Some(OrderbookLevel {
                price: pf(&l["price"])?,
                size:  pf(&l["priceLevelQuantity"])?,
            })
        }).collect()
    };

    Ok(OrderbookSnapshot {
        instrument_name: instrument_name.to_string(),
        bids: parse(&ob["bids"]),
        asks: parse(&ob["asks"]),
        timestamp: now_ms() as i64,
    })
}

// ── Private Endpoints ──────────────────────────────────────────────────────

pub async fn place_order(req: &PlaceOrderRequest, account: &Account) -> Result<OrderResult, String> {
    let jwt                = get_jwt(&account.api_key, &account.api_secret).await?;
    let trading_account_id = get_trading_account_id(account).await?;

    let ts    = now_ms().to_string();
    let nonce = now_us().to_string();
    let path  = "/trading-api/v2/orders";

    let order_type = if req.post_only.unwrap_or(false) {
        "POST_ONLY"
    } else {
        match req.order_type.to_lowercase().as_str() {
            "market" => "MKT",
            _        => "LMT",
        }
    };

    let tif = match req.time_in_force.as_deref().unwrap_or("good_til_cancelled") {
        "immediate_or_cancel" => "IOC",
        "fill_or_kill"        => "FOK",
        "post_only"           => "POST_ONLY",
        _                     => "GTC",
    };

    let client_oid = req.client_order_id.clone().unwrap_or_else(|| nonce.clone());
    let body = json!({
        "symbol":           req.instrument_name,
        "commandType":      "V3CreateOrder",
        "type":             order_type,
        "side":             req.side.to_uppercase(),
        "quantity":         format!("{:.8}", req.amount),
        "price":            req.price.map(|p| format!("{:.8}", p)).unwrap_or_default(),
        "timeInForce":      tif,
        "allowBorrow":      false,
        "clientOrderId":    client_oid,
        "tradingAccountId": trading_account_id,
    });
    let body_str = serde_json::to_string(&body).map_err(|e| e.to_string())?;
    let sig = sign(&ts, &nonce, "POST", path, &body_str, &account.api_secret)?;

    let resp: Value = Client::new()
        .post(format!("{}{}", BASE, path))
        .headers(auth_headers(&jwt, &sig, &ts, &nonce))
        .body(body_str)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    let order_id = resp["orderId"].as_str().or_else(|| resp["id"].as_str()).unwrap_or("").to_string();
    if order_id.is_empty() {
        return Ok(OrderResult { success: false, order: None, error: Some(resp.to_string()) });
    }

    Ok(OrderResult {
        success: true,
        order: Some(Order {
            order_id,
            instrument_name:       req.instrument_name.clone(),
            direction:             req.side.to_lowercase(),
            order_type:            req.order_type.to_lowercase(),
            order_state:           "open".to_string(),
            price:                 req.price,
            amount:                req.amount,
            filled_amount:         0.0,
            average_price:         None,
            post_only:             req.post_only.unwrap_or(false),
            time_in_force:         req.time_in_force.clone().unwrap_or_else(|| "good_til_cancelled".to_string()),
            creation_timestamp:    now_ms() as i64,
            last_update_timestamp: now_ms() as i64,
        }),
        error: None,
    })
}

pub async fn cancel_order(order_id: &str, instrument_name: Option<&str>, account: &Account) -> Result<bool, String> {
    let jwt                = get_jwt(&account.api_key, &account.api_secret).await?;
    let trading_account_id = get_trading_account_id(account).await?;

    let ts    = now_ms().to_string();
    let nonce = now_us().to_string();
    let path  = format!("/trading-api/v2/orders/{}", order_id);

    let body = json!({
        "commandType":      "V1CancelOrder",
        "tradingAccountId": trading_account_id,
        "symbol":           instrument_name.unwrap_or(""),
    });
    let body_str = serde_json::to_string(&body).map_err(|e| e.to_string())?;
    let sig = sign(&ts, &nonce, "DELETE", &path, &body_str, &account.api_secret)?;

    let status = Client::new()
        .delete(format!("{}{}", BASE, path))
        .headers(auth_headers(&jwt, &sig, &ts, &nonce))
        .body(body_str)
        .send().await.map_err(|e| e.to_string())?
        .status();
    Ok(status.is_success())
}

fn parse_order(o: &Value) -> Option<Order> {
    let status = o["status"].as_str().unwrap_or("");
    let order_state = match status {
        "OPEN" | "PENDING_NEW" | "PARTIALLY_FILLED" => "open",
        "FILLED"                                     => "filled",
        "CANCELED" | "CANCELLED" | "EXPIRED" | "REJECTED" => "cancelled",
        other => other,
    };
    let post_only = o["timeInForce"].as_str().unwrap_or("") == "POST_ONLY";
    let tif = match o["timeInForce"].as_str().unwrap_or("GTC") {
        "IOC"       => "immediate_or_cancel",
        "FOK"       => "fill_or_kill",
        "POST_ONLY" => "post_only",
        _           => "good_til_cancelled",
    };
    Some(Order {
        order_id:              o["orderId"].as_str().unwrap_or("").to_string(),
        instrument_name:       o["symbol"].as_str().unwrap_or("").to_string(),
        direction:             o["side"].as_str().unwrap_or("BUY").to_lowercase(),
        order_type:            match o["type"].as_str().unwrap_or("LMT") {
            "MKT" => "market", "STOP_LIMIT" => "stop_limit", _ => "limit",
        }.to_string(),
        order_state:           order_state.to_string(),
        price:                 pf(&o["price"]),
        amount:                pf0(&o["quantity"]),
        filled_amount:         pf0(&o["cumulativeQuantity"]),
        average_price:         pf(&o["averagePrice"]),
        post_only,
        time_in_force:         tif.to_string(),
        creation_timestamp:    o["createdAtTimestamp"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
        last_update_timestamp: o["updatedAtTimestamp"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
    })
}

pub async fn get_open_orders(instrument_name: &str, account: &Account) -> Result<Vec<Order>, String> {
    let jwt                = get_jwt(&account.api_key, &account.api_secret).await?;
    let trading_account_id = get_trading_account_id(account).await?;

    let mut url = format!("{}/trading-api/v2/orders?tradingAccountId={}&status=OPEN", BASE, trading_account_id);
    if !instrument_name.is_empty() { url.push_str(&format!("&symbol={}", instrument_name)); }

    let resp: Value = Client::new().get(&url)
        .header(AUTHORIZATION, format!("Bearer {}", jwt))
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    let arr = resp["data"].as_array()
        .or_else(|| resp.as_array())
        .cloned().unwrap_or_default();
    Ok(arr.iter().filter_map(parse_order).collect())
}

pub async fn get_all_open_orders(account: &Account) -> Result<Vec<Order>, String> {
    get_open_orders("", account).await
}

pub async fn get_account_summary(currency: &str, account: &Account) -> Result<AccountSummary, String> {
    let jwt                = get_jwt(&account.api_key, &account.api_secret).await?;
    let trading_account_id = get_trading_account_id(account).await?;

    let resp: Value = Client::new()
        .get(format!("{}/trading-api/v1/accounts/assets?tradingAccountId={}", BASE, trading_account_id))
        .header(AUTHORIZATION, format!("Bearer {}", jwt))
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    let cur_upper = currency.to_uppercase();
    let arr = resp.as_array().or_else(|| resp["data"].as_array()).cloned().unwrap_or_default();

    let mut equity    = 0.0f64;
    let mut available = 0.0f64;
    for asset in &arr {
        let sym = asset["assetSymbol"].as_str().unwrap_or("").to_uppercase();
        if sym == cur_upper {
            equity    = pf0(&asset["totalQuantity"]);
            available = pf0(&asset["availableQuantity"]);
            break;
        }
    }

    Ok(AccountSummary {
        currency: currency.to_string(),
        equity,
        available_funds: available,
        initial_margin:   0.0,
        maintenance_margin: 0.0,
        unrealized_pl:    0.0,
    })
}

pub async fn get_trade_history(account: &Account, start_ms: i64, end_ms: i64) -> Result<Vec<Trade>, String> {
    let jwt                = get_jwt(&account.api_key, &account.api_secret).await?;
    let trading_account_id = get_trading_account_id(account).await?;

    let url = format!(
        "{}/trading-api/v2/fills?tradingAccountId={}&createdAfter={}&limit=200",
        BASE, trading_account_id, start_ms
    );
    let resp: Value = Client::new().get(&url)
        .header(AUTHORIZATION, format!("Bearer {}", jwt))
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    let arr = resp["data"].as_array()
        .or_else(|| resp.as_array())
        .cloned().unwrap_or_default();

    Ok(arr.iter().filter_map(|t| {
        let ts: i64 = t["createdAtTimestamp"].as_str()?.parse().ok()?;
        if ts < start_ms || ts > end_ms { return None; }
        Some(Trade {
            trade_id:        t["fillId"].as_str().unwrap_or("").to_string(),
            account_id:      account.id.clone(),
            account_name:    account.name.clone(),
            exchange:        "bullish".to_string(),
            instrument_name: t["symbol"].as_str().unwrap_or("").to_string(),
            direction:       match t["side"].as_str().unwrap_or("BUY") { "SELL" => "sell", _ => "buy" }.to_string(),
            amount:          pf0(&t["quantity"]),
            price:           pf0(&t["price"]),
            fee:             pf0(&t["fee"]),
            fee_currency:    t["feeCurrency"].as_str().unwrap_or("").to_string(),
            timestamp:       ts,
            order_id:        t["orderId"].as_str().unwrap_or("").to_string(),
        })
    }).collect())
}

pub async fn get_positions(_currency: &str, _account: &Account) -> Result<Vec<Position>, String> {
    // Bullish is a spot exchange — no derivatives positions
    Ok(vec![])
}

/// Extract base coin from a Bullish spot symbol, e.g. "BTCUSDT" → "BTC".
fn extract_base(symbol: &str) -> String {
    // Known quote suffixes, longest first so "USDT" beats "USD"
    for suffix in &["USDC", "USDT", "BUSD", "USD", "BTC", "ETH", "BNB"] {
        if symbol.ends_with(suffix) && symbol.len() > suffix.len() {
            return symbol[..symbol.len() - suffix.len()].to_string();
        }
    }
    symbol.to_string()
}

/// Fetch trade fills as a canonical transaction log.
/// Bullish is spot-only so there is no dedicated ledger endpoint;
/// fills are the closest equivalent.
pub async fn get_transaction_log(
    account: &Account,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<TransactionLog>, String> {
    let jwt                = get_jwt(&account.api_key, &account.api_secret).await?;
    let trading_account_id = get_trading_account_id(account).await?;
    let client = Client::new();
    let mut logs: Vec<TransactionLog> = Vec::new();
    // Paginate using `createdAfter` + the timestamp of the last fill seen.
    let mut after_ms = start_ms;

    loop {
        let url = format!(
            "{}/trading-api/v2/fills?tradingAccountId={}&createdAfter={}&limit=200",
            BASE, trading_account_id, after_ms
        );
        let resp: Value = client.get(&url)
            .header(AUTHORIZATION, format!("Bearer {}", jwt))
            .send().await.map_err(|e| e.to_string())?
            .json().await.map_err(|e| e.to_string())?;

        let arr = resp["data"].as_array()
            .or_else(|| resp.as_array())
            .cloned()
            .unwrap_or_default();

        if arr.is_empty() { break; }

        let mut last_ts = after_ms;
        for fill in &arr {
            let ts: i64 = fill["createdAtTimestamp"].as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if ts < start_ms || ts > end_ms { continue; }
            if ts > last_ts { last_ts = ts; }

            let symbol         = fill["symbol"].as_str().unwrap_or("");
            let currency       = extract_base(symbol);
            let base_currency  = currency.clone();
            let quote_currency = symbol[currency.len()..].to_string();
            let fee_cur        = fill["feeCurrency"].as_str().unwrap_or("").to_string();

            logs.push(TransactionLog {
                id:                 fill["fillId"].as_str().unwrap_or("").to_string(),
                timestamp:          ts,
                instrument_name:    symbol.to_string(),
                transaction_type:   "trade".to_string(),
                side:               match fill["side"].as_str().unwrap_or("BUY") {
                                        "SELL" => "sell".to_string(),
                                        _      => "buy".to_string(),
                                    },
                amount:             pf0(&fill["quantity"]),
                price:              pf0(&fill["price"]),
                fee:                pf0(&fill["fee"]),
                fee_currency:       fee_cur,
                currency,
                profit_as_cashflow: 0.0,
                balance:            0.0,
                change:             0.0,
                trade_id:           fill["fillId"].as_str().unwrap_or("").to_string(),
                order_id:           fill["orderId"].as_str().unwrap_or("").to_string(),
                info:               String::new(),
                mark_price:         0.0,
                index_price:        0.0,
                equity:             0.0,
                position:           pf0(&fill["quantity"]) * pf0(&fill["price"]),
                base_currency,
                quote_currency,
            });
        }

        // Stop if we received fewer than a full page or didn't advance
        if arr.len() < 200 || last_ts <= after_ms { break; }
        after_ms = last_ts + 1; // advance past last seen fill
    }

    logs.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(logs)
}
