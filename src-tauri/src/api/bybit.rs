use reqwest::{Client, header::{HeaderMap, HeaderValue, CONTENT_TYPE}};
use serde_json::{json, Value};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::api::models::{
    Account, AccountSummary, Instrument, Order, OrderResult, OrderbookLevel, OrderbookSnapshot,
    PlaceOrderRequest, Position, Ticker, TickerStats, Trade, TransactionLog,
};

type HmacSha256 = Hmac<Sha256>;

const BYBIT_BASE: &str      = "https://api.bybit.com";
const BYBIT_TEST_BASE: &str = "https://api-testnet.bybit.com";
const RECV_WINDOW: u64      = 5000;

/// Millisecond offset: server_time - local_time, updated by sync_server_time().
static TIME_OFFSET_MS: AtomicI64 = AtomicI64::new(0);

fn base(testnet: bool) -> &'static str {
    if testnet { BYBIT_TEST_BASE } else { BYBIT_BASE }
}

fn now_ms() -> u64 {
    let local = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64;
    (local + TIME_OFFSET_MS.load(Ordering::Relaxed)).max(0) as u64
}

/// Fetch Bybit server time and store the clock offset so all subsequent
/// requests use a timestamp that matches the exchange.
pub async fn sync_server_time() {
    let url = format!("{}/v5/market/time", BYBIT_BASE);
    if let Ok(resp) = Client::new().get(&url).send().await {
        if let Ok(v) = resp.json::<Value>().await {
            let server_ms = v["result"]["timeNano"].as_str()
                .and_then(|s| s.parse::<i64>().ok())
                .map(|ns| ns / 1_000_000)
                .or_else(|| v["result"]["timeSecond"].as_str()
                    .and_then(|s| s.parse::<i64>().ok())
                    .map(|s| s * 1000));
            if let Some(server_ms) = server_ms {
                let local_ms = SystemTime::now().duration_since(UNIX_EPOCH)
                    .unwrap_or_default().as_millis() as i64;
                let offset = server_ms - local_ms;
                TIME_OFFSET_MS.store(offset, Ordering::Relaxed);
                eprintln!("[bybit] server time offset: {}ms", offset);
            }
        }
    }
}

fn sign(msg: &str, secret: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(msg.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Headers for GET. `query_string` is the raw query WITHOUT leading "?".
fn auth_get(api_key: &str, secret: &str, query_string: &str) -> HeaderMap {
    let ts  = now_ms();
    let msg = format!("{}{}{}{}", ts, api_key, RECV_WINDOW, query_string);
    let sig = sign(&msg, secret);
    build_headers(api_key, &sig, ts)
}

/// Headers for POST. `body` is the JSON body string.
fn auth_post(api_key: &str, secret: &str, body: &str) -> HeaderMap {
    let ts  = now_ms();
    let msg = format!("{}{}{}{}", ts, api_key, RECV_WINDOW, body);
    let sig = sign(&msg, secret);
    let mut h = build_headers(api_key, &sig, ts);
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    h
}

fn build_headers(api_key: &str, sig: &str, ts: u64) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("X-BAPI-API-KEY",      HeaderValue::from_str(api_key).unwrap_or_else(|_| HeaderValue::from_static("")));
    h.insert("X-BAPI-SIGN",         HeaderValue::from_str(sig).unwrap_or_else(|_| HeaderValue::from_static("")));
    h.insert("X-BAPI-TIMESTAMP",    HeaderValue::from_str(&ts.to_string()).unwrap());
    h.insert("X-BAPI-RECV-WINDOW",  HeaderValue::from_str(&RECV_WINDOW.to_string()).unwrap());
    h
}

/// Infer Bybit v5 category from instrument symbol:
/// - contains "-"              → option  (e.g. BTC-29MAR24-50000-C)
/// - ends with "USD" (not "T"/"C") → inverse  (e.g. BTCUSD, ETHUSD — coin-margined)
/// - otherwise                 → linear  (e.g. BTCUSDT, ETHUSDC — USDT/USDC-margined)
pub fn infer_category(symbol: &str) -> &'static str {
    if symbol.contains('-') { return "option"; }
    // "BTCUSD" ends with 'D' after "US"; "BTCUSDT" ends with 'T' — not "USD"
    if symbol.ends_with("USD") { return "inverse"; }
    "linear"
}

/// Fetch instruments for a specific Bybit v5 category.
async fn fetch_by_category(currency: &str, category: &'static str, kind_label: &str) -> Result<Vec<Instrument>, String> {
    // Use limit=1000 for options (many strikes per expiry); 500 for others
    let limit = if category == "option" { 1000 } else { 500 };
    let url = format!("{}/v5/market/instruments-info?category={}&baseCoin={}&limit={}", BYBIT_BASE, category, currency, limit);

    let resp: Value = Client::new().get(&url)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["retCode"].as_i64() != Some(0) {
        return Err(resp["retMsg"].as_str().unwrap_or("Bybit error").to_string());
    }

    let list = resp["result"]["list"].as_array().ok_or("no list")?;
    let instruments = list.iter()
        .filter(|i| {
            // Include Trading, Delivering (same-day expiry being settled), and PreDelivery.
            // Exclude only "Closed" / "Settling" post-settlement.
            let s = i["status"].as_str().unwrap_or("Trading");
            !matches!(s, "Closed" | "Settling" | "CLOSED" | "SETTLING")
        })
        .map(|i| {
            let sym = i["symbol"].as_str().unwrap_or("");
            // Parse expiry from symbol as fallback (BTC-07JUN26-50000-C → "07JUN26")
            let sym_expiry_ms: Option<i64> = {
                let parts: Vec<&str> = sym.split('-').collect();
                if parts.len() >= 2 {
                    // Bybit date format: "07JUN26" = DD+MON+YY
                    let s = parts[1];
                    if s.len() == 7 {
                        let dd: i32 = s[0..2].parse().unwrap_or(0);
                        let yy: i32 = s[5..7].parse().unwrap_or(0);
                        let mm = match &s[2..5] {
                            "JAN" => 1, "FEB" => 2, "MAR" => 3, "APR" => 4,
                            "MAY" => 5, "JUN" => 6, "JUL" => 7, "AUG" => 8,
                            "SEP" => 9, "OCT" => 10, "NOV" => 11, "DEC" => 12,
                            _ => 0,
                        };
                        if dd > 0 && mm > 0 {
                            // Bybit options expire at 08:00 UTC
                            let year = 2000 + yy;
                            // Simple unix ms calculation: approximate via chrono-like math
                            // Days from 1970-01-01 to year-01-01
                            let days_to_year: i64 = {
                                let y = year as i64;
                                365 * (y - 1970) + (y - 1969) / 4 - (y - 1901) / 100 + (y - 1601) / 400
                            };
                            let month_days: [i64; 12] = [31,28,31,30,31,30,31,31,30,31,30,31];
                            let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
                            let days_to_month: i64 = month_days[..(mm as usize - 1)].iter().sum::<i64>()
                                + if leap && mm > 2 { 1 } else { 0 };
                            let total_days = days_to_year + days_to_month + (dd as i64 - 1);
                            Some(total_days * 86_400_000 + 8 * 3_600_000)
                        } else { None }
                    } else { None }
                } else { None }
            };

            Instrument {
            instrument_name:      sym.to_string(),
            kind:                 kind_label.to_string(),
            base_currency:        i["baseCoin"].as_str().unwrap_or(currency).to_string(),
            quote_currency:       i["quoteCoin"].as_str().unwrap_or("USDT").to_string(),
            settlement_currency:  i["settleCoin"].as_str().unwrap_or("USDT").to_string(),
            is_active:            true,
            tick_size:            i["priceFilter"]["tickSize"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.01),
            min_trade_amount:     i["lotSizeFilter"]["minOrderQty"].as_str().and_then(|s| s.parse().ok()).unwrap_or(1.0),
            qty_step:             i["lotSizeFilter"]["qtyStep"].as_str().and_then(|s| s.parse().ok()),
            contract_size:        None,
            option_type: {
                let from_field = i["optionsType"].as_str()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_lowercase());
                if from_field.is_some() { from_field } else {
                    sym.rsplit('-').next()
                        .map(|s| s.to_lowercase())
                        .filter(|s| s == "c" || s == "p" || s == "call" || s == "put")
                }
            },
            strike: {
                let v = &i["strikePrice"];
                v.as_str().and_then(|s| s.parse::<f64>().ok())
                    .or_else(|| v.as_f64())
                    .or_else(|| {
                        let parts: Vec<&str> = sym.split('-').collect();
                        if parts.len() >= 3 { parts[2].parse::<f64>().ok() } else { None }
                    })
            },
            expiration_timestamp: {
                // deliveryTime = 0 for same-day expiry on Bybit → use symbol-parsed date
                let v = &i["deliveryTime"];
                let from_field = v.as_str().and_then(|s| s.parse::<i64>().ok())
                    .or_else(|| v.as_i64())
                    .filter(|&t| t > 0);
                from_field.or(sym_expiry_ms)
            },
        }})
        .collect();

    Ok(instruments)
}

pub async fn fetch_instruments(currency: &str, kind: &str) -> Result<Vec<Instrument>, String> {
    match kind {
        "option" => fetch_by_category(currency, "option", "option").await,
        "spot"   => fetch_by_category(currency, "spot", "spot").await,
        _ => {
            // For futures: fetch both linear (USDT-margined) and inverse (USD/coin-margined),
            // then combine. The frontend uses the quote_currency field to distinguish them.
            let mut all = fetch_by_category(currency, "linear", "future").await.unwrap_or_default();
            let inverse = fetch_by_category(currency, "inverse", "future").await.unwrap_or_default();
            all.extend(inverse);
            Ok(all)
        }
    }
}

pub async fn fetch_ticker(instrument_name: &str) -> Result<Ticker, String> {
    let category = infer_category(instrument_name);
    let url = format!("{}/v5/market/tickers?category={}&symbol={}", BYBIT_BASE, category, instrument_name);

    let resp: Value = Client::new().get(&url)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["retCode"].as_i64() != Some(0) {
        return Err(resp["retMsg"].as_str().unwrap_or("Bybit error").to_string());
    }

    let d = &resp["result"]["list"][0];
    let pct = d["price24hPcnt"].as_str().and_then(|s| s.parse::<f64>().ok()).map(|v| v * 100.0);

    Ok(Ticker {
        instrument_name:  instrument_name.to_string(),
        best_bid_price:   sf64(&d["bid1Price"]),
        best_ask_price:   sf64(&d["ask1Price"]),
        best_bid_amount:  sf64(&d["bid1Size"]),
        best_ask_amount:  sf64(&d["ask1Size"]),
        last_price:       sf64(&d["lastPrice"]),
        mark_price:       sf64(&d["markPrice"]),
        index_price:      sf64(&d["indexPrice"]),
        underlying_price: sf64(&d["underlyingPrice"]),
        open_interest:    sf64(&d["openInterest"]),
        stats: TickerStats {
            high:         sf64(&d["highPrice24h"]),
            low:          sf64(&d["lowPrice24h"]),
            price_change: pct,
            volume:       sf64(&d["volume24h"]),
            volume_usd:   sf64(&d["turnover24h"]),
        },
        mark_iv: sf64(&d["markIv"]),
        bid_iv:  sf64(&d["bidIv"]),
        ask_iv:  sf64(&d["askIv"]),
        delta:   sf64(&d["delta"]),
        gamma:   sf64(&d["gamma"]),
        vega:    sf64(&d["vega"]),
        theta:   sf64(&d["theta"]),
    })
}

// ── Authenticated ──────────────────────────────────────────────────────────

pub async fn place_order(req: &PlaceOrderRequest, account: &Account) -> Result<OrderResult, String> {
    let url      = format!("{}/v5/order/create", base(account.testnet));
    let category = infer_category(&req.instrument_name);
    let side     = if req.side == "buy" { "Buy" } else { "Sell" };

    let order_type = if req.post_only.unwrap_or(false) {
        "Limit"  // Post-only is set via timeInForce = "PostOnly"
    } else {
        match req.order_type.as_str() { "market" => "Market", _ => "Limit" }
    };

    let tif = if req.post_only.unwrap_or(false) {
        "PostOnly".to_string()
    } else {
        match req.time_in_force.as_deref().unwrap_or("good_til_cancelled") {
            "immediate_or_cancel" => "IOC",
            "fill_or_kill"        => "FOK",
            _                     => "GTC",
        }.to_string()
    };

    let mut body = json!({
        "category": category,
        "symbol": req.instrument_name,
        "side": side,
        "orderType": order_type,
        "qty": req.amount.to_string(),
        "timeInForce": tif,
    });
    if let Some(px) = req.price { body["price"] = json!(px.to_string()); }
    // Bybit orderLinkId: max 36 chars; truncate if needed
    if let Some(ref clord_id) = req.client_order_id {
        let link_id = if clord_id.len() > 36 { &clord_id[..36] } else { clord_id.as_str() };
        body["orderLinkId"] = json!(link_id);
    }

    let body_str = serde_json::to_string(&body).unwrap();
    let headers  = auth_post(&account.api_key, &account.api_secret, &body_str);

    let resp: Value = Client::new().post(&url)
        .headers(headers).body(body_str)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["retCode"].as_i64() != Some(0) {
        return Ok(OrderResult { success: false, order: None, error: Some(resp["retMsg"].as_str().unwrap_or("Bybit order failed").to_string()) });
    }

    Ok(OrderResult {
        success: true,
        order: Some(Order {
            order_id:             resp["result"]["orderId"].as_str().unwrap_or("").to_string(),
            instrument_name:      req.instrument_name.clone(),
            direction:            req.side.clone(),
            order_type:           req.order_type.clone(),
            order_state:          "New".to_string(),
            price:                req.price,
            amount:               req.amount,
            filled_amount:        0.0,
            average_price:        None,
            post_only:            req.post_only.unwrap_or(false),
            time_in_force:        req.time_in_force.clone().unwrap_or_else(|| "good_til_cancelled".to_string()),
            creation_timestamp:   0,
            last_update_timestamp: 0,
        }),
        error: None,
    })
}

pub async fn cancel_order(order_id: &str, instrument_name: Option<&str>, account: &Account) -> Result<bool, String> {
    let url      = format!("{}/v5/order/cancel", base(account.testnet));
    let symbol   = instrument_name.unwrap_or("");
    let category = infer_category(symbol);

    eprintln!("[bybit] cancel_order: orderId={} symbol={} category={}", order_id, symbol, category);

    let body = json!({ "category": category, "symbol": symbol, "orderId": order_id });
    let body_str = serde_json::to_string(&body).unwrap();
    let headers  = auth_post(&account.api_key, &account.api_secret, &body_str);

    let resp: Value = Client::new().post(&url)
        .headers(headers).body(body_str)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    eprintln!("[bybit] cancel_order response: {}", resp);

    let ret_code = resp["retCode"].as_i64().unwrap_or(-1);
    if ret_code != 0 {
        let msg = resp["retMsg"].as_str().unwrap_or("Unknown Bybit error");
        return Err(format!("Bybit cancel error ({}): {}", ret_code, msg));
    }
    Ok(true)
}

pub async fn get_open_orders(instrument_name: &str, account: &Account) -> Result<Vec<Order>, String> {
    let category = infer_category(instrument_name);
    let qs  = format!("category={}&symbol={}", category, instrument_name);
    let url = format!("{}/v5/order/realtime?{}", base(account.testnet), qs);
    let headers = auth_get(&account.api_key, &account.api_secret, &qs);

    let resp: Value = Client::new().get(&url)
        .headers(headers)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["retCode"].as_i64() != Some(0) {
        return Err(resp["retMsg"].as_str().unwrap_or("Bybit error").to_string());
    }

    let orders = resp["result"]["list"].as_array().map(|arr| arr.iter().map(|o| {
        let tif_raw = o["timeInForce"].as_str().unwrap_or("GTC");
        Order {
            order_id:             o["orderId"].as_str().unwrap_or("").to_string(),
            instrument_name:      o["symbol"].as_str().unwrap_or("").to_string(),
            direction:            o["side"].as_str().unwrap_or("").to_lowercase(),
            order_type:           o["orderType"].as_str().unwrap_or("").to_lowercase(),
            order_state:          o["orderStatus"].as_str().unwrap_or("").to_string(),
            price:                sf64(&o["price"]),
            amount:               sf64(&o["qty"]).unwrap_or(0.0),
            filled_amount:        sf64(&o["cumExecQty"]).unwrap_or(0.0),
            average_price:        sf64(&o["avgPrice"]),
            post_only:            tif_raw == "PostOnly",
            time_in_force:        tif_to_generic(tif_raw),
            creation_timestamp:   o["createdTime"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
            last_update_timestamp: o["updatedTime"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
        }
    }).collect()).unwrap_or_default();

    Ok(orders)
}

/// Get ALL open orders for this account across option and linear categories.
pub async fn get_all_open_orders(account: &Account) -> Result<Vec<Order>, String> {
    let mut all = Vec::new();
    for category in &["option", "linear"] {
        let qs = format!("category={}&limit=200", category);
        let url = format!("{}/v5/order/realtime?{}", base(account.testnet), qs);
        let headers = auth_get(&account.api_key, &account.api_secret, &qs);
        let resp: Value = match Client::new().get(&url).headers(headers).send().await {
            Ok(r) => r.json().await.unwrap_or(Value::Null),
            Err(_) => continue,
        };
        if resp["retCode"].as_i64() != Some(0) { continue; }
        if let Some(list) = resp["result"]["list"].as_array() {
            all.extend(list.iter().map(|o| {
                let tif_raw = o["timeInForce"].as_str().unwrap_or("GTC");
                Order {
                    order_id:              o["orderId"].as_str().unwrap_or("").to_string(),
                    instrument_name:       o["symbol"].as_str().unwrap_or("").to_string(),
                    direction:             o["side"].as_str().unwrap_or("").to_lowercase(),
                    order_type:            o["orderType"].as_str().unwrap_or("").to_lowercase(),
                    order_state:           o["orderStatus"].as_str().unwrap_or("").to_string(),
                    price:                 sf64(&o["price"]),
                    amount:                sf64(&o["qty"]).unwrap_or(0.0),
                    filled_amount:         sf64(&o["cumExecQty"]).unwrap_or(0.0),
                    average_price:         sf64(&o["avgPrice"]),
                    post_only:             tif_raw == "PostOnly",
                    time_in_force:         tif_to_generic(tif_raw),
                    creation_timestamp:    o["createdTime"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
                    last_update_timestamp: o["updatedTime"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
                }
            }));
        }
    }
    Ok(all)
}

/// Get trade history (executions) for this account.
pub async fn get_trade_history(account: &Account, start_ms: i64, end_ms: i64) -> Result<Vec<Trade>, String> {
    let mut all = Vec::new();
    for category in &["option", "linear"] {
        let mut qs = format!("category={}&limit=200", category);
        if start_ms > 0 { qs.push_str(&format!("&startTime={}", start_ms)); }
        if end_ms   > 0 { qs.push_str(&format!("&endTime={}", end_ms)); }
        let url = format!("{}/v5/execution/list?{}", base(account.testnet), qs);
        let headers = auth_get(&account.api_key, &account.api_secret, &qs);
        let resp: Value = match Client::new().get(&url).headers(headers).send().await {
            Ok(r) => r.json().await.unwrap_or(Value::Null),
            Err(_) => continue,
        };
        if resp["retCode"].as_i64() != Some(0) { continue; }
        if let Some(list) = resp["result"]["list"].as_array() {
            for t in list {
                all.push(Trade {
                    trade_id:        t["execId"].as_str().unwrap_or("").to_string(),
                    account_id:      String::new(),
                    account_name:    String::new(),
                    exchange:        "bybit".to_string(),
                    instrument_name: t["symbol"].as_str().unwrap_or("").to_string(),
                    direction:       t["side"].as_str().unwrap_or("").to_lowercase(),
                    amount:          sf64(&t["execQty"]).unwrap_or(0.0),
                    price:           sf64(&t["execPrice"]).unwrap_or(0.0),
                    fee:             sf64(&t["execFee"]).unwrap_or(0.0),
                    fee_currency:    t["feeCurrency"].as_str().unwrap_or("").to_string(),
                    timestamp:       t["execTime"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
                    order_id:        t["orderId"].as_str().unwrap_or("").to_string(),
                });
            }
        }
    }
    Ok(all)
}

pub async fn get_account_summary(currency: &str, account: &Account) -> Result<AccountSummary, String> {
    // For UNIFIED accounts we pass the coin filter only when a specific coin is requested.
    // Account-level totals (totalEquity, totalAvailableBalance, totalInitialMargin,
    // totalMaintenanceMargin, totalPerpUPL) are always in USD.
    let qs = if currency.to_uppercase() == "USD" || currency.is_empty() {
        "accountType=UNIFIED".to_string()
    } else {
        format!("accountType=UNIFIED&coin={}", currency.to_uppercase())
    };
    let url = format!("{}/v5/account/wallet-balance?{}", base(account.testnet), qs);
    let headers = auth_get(&account.api_key, &account.api_secret, &qs);

    let resp: Value = Client::new().get(&url)
        .headers(headers)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["retCode"].as_i64() != Some(0) {
        return Err(resp["retMsg"].as_str().unwrap_or("Bybit error").to_string());
    }

    let acct = &resp["result"]["list"][0];

    // Use account-level USD totals for margin figures (these are accurate for UNIFIED accounts).
    // `availableToWithdraw` per-coin is deprecated since Jan 2025.
    let equity   = sf64(&acct["totalEquity"]).unwrap_or(0.0);
    let avail    = sf64(&acct["totalAvailableBalance"]).unwrap_or(0.0);
    let im       = sf64(&acct["totalInitialMargin"]).unwrap_or(0.0);
    let mm       = sf64(&acct["totalMaintenanceMargin"]).unwrap_or(0.0);
    let upnl     = sf64(&acct["totalPerpUPL"]).unwrap_or(0.0);

    // If a specific coin was requested, also try per-coin equity as fallback.
    let (final_equity, final_avail, final_im, final_mm, final_upnl) =
        if currency.to_uppercase() != "USD" && !currency.is_empty() {
            let coin_arr = acct["coin"].as_array();
            if let Some(coin) = coin_arr.and_then(|arr| {
                arr.iter().find(|c| c["coin"].as_str().map(|s| s.eq_ignore_ascii_case(currency)).unwrap_or(false))
            }) {
                // Per-coin equity (don't fall back to account USD total if coin equity is 0)
                let c_equity = sf64(&coin["equity"]).unwrap_or(0.0);
                // Per-coin IM = totalPositionIM + totalOrderIM
                let c_im = sf64(&coin["totalPositionIM"]).unwrap_or(0.0)
                         + sf64(&coin["totalOrderIM"]).unwrap_or(0.0);
                let c_mm   = sf64(&coin["totalPositionMM"]).unwrap_or(0.0);
                let c_upnl = sf64(&coin["unrealisedPnl"]).unwrap_or(0.0);
                // Per-coin available: walletBalance - totalPositionIM - totalOrderIM - locked
                let wb    = sf64(&coin["walletBalance"]).unwrap_or(0.0);
                let lk    = sf64(&coin["locked"]).unwrap_or(0.0);
                let c_avail = (wb - c_im - lk).max(0.0);
                (c_equity, c_avail, c_im, c_mm, c_upnl)
            } else {
                // Coin not found in response (zero balance) → return all zeros.
                // Do NOT fall back to account-level USD totals which would show
                // wrong large numbers against a zero-equity coin label.
                (0.0, 0.0, 0.0, 0.0, 0.0)
            }
        } else {
            // "USD" → account-level totals in USD
            (equity, avail, im, mm, upnl)
        };

    Ok(AccountSummary {
        currency:           currency.to_string(),
        equity:             final_equity,
        available_funds:    final_avail,
        initial_margin:     final_im,
        maintenance_margin: final_mm,
        unrealized_pl:      final_upnl,
    })
}

/// Get open positions across option and linear categories.
pub async fn get_positions(currency: &str, account: &Account) -> Result<Vec<Position>, String> {
    let mut all: Vec<Position> = Vec::new();
    // Bybit options use settleCoin (e.g. BTC, ETH); linear use USD/USDT settle
    let categories: &[(&str, &str)] = &[
        ("option", currency),
        ("linear", "USDT"),
    ];
    for (category, settle) in categories {
        let qs = format!("category={}&settleCoin={}&limit=200", category, settle);
        let url = format!("{}/v5/position/list?{}", base(account.testnet), qs);
        let headers = auth_get(&account.api_key, &account.api_secret, &qs);
        let resp: Value = match Client::new().get(&url).headers(headers).send().await {
            Ok(r) => r.json().await.unwrap_or(Value::Null),
            Err(_) => continue,
        };
        if resp["retCode"].as_i64() != Some(0) { continue; }
        if let Some(list) = resp["result"]["list"].as_array() {
            all.extend(list.iter().filter_map(|p| {
                let size: f64 = sf64(&p["size"]).unwrap_or(0.0);
                if size == 0.0 { return None; }
                Some(Position {
                    instrument_name: p["symbol"].as_str().unwrap_or("").to_string(),
                    direction:       p["side"].as_str().map(|s| s.to_lowercase()).unwrap_or_default(),
                    size,
                    average_price:   sf64(&p["avgPrice"]).unwrap_or(0.0),
                    mark_price:      sf64(&p["markPrice"]).unwrap_or(0.0),
                    mark_iv:         sf64(&p["markIv"]).unwrap_or(0.0),
                    unrealized_pnl:  sf64(&p["unrealisedPnl"]).unwrap_or(0.0),
                    delta:           sf64(&p["delta"]).unwrap_or(0.0),
                    gamma:           sf64(&p["gamma"]).unwrap_or(0.0),
                    theta:           sf64(&p["theta"]).unwrap_or(0.0),
                    vega:            sf64(&p["vega"]).unwrap_or(0.0),
                })
            }));
        }
    }
    Ok(all)
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn sf64(v: &Value) -> Option<f64> {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_f64())
}

fn tif_to_generic(s: &str) -> String {
    match s {
        "IOC"      => "immediate_or_cancel".to_string(),
        "FOK"      => "fill_or_kill".to_string(),
        _          => "good_til_cancelled".to_string(),
    }
}

pub async fn fetch_orderbook(instrument_name: &str, depth: u32) -> Result<OrderbookSnapshot, String> {
    let client = Client::new();
    let category = infer_category(instrument_name);
    let url = format!(
        "{}/v5/market/orderbook?category={}&symbol={}&limit={}",
        BYBIT_BASE, category, instrument_name, depth
    );

    let resp: Value = client.get(&url).send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    if resp["retCode"].as_i64() == Some(0) {
        let result = &resp["result"];
        let parse_levels = |arr: &Value| -> Vec<OrderbookLevel> {
            arr.as_array().map(|a| a.iter().filter_map(|item| {
                let arr = item.as_array()?;
                Some(OrderbookLevel {
                    price: arr.get(0)?.as_str()?.parse().ok()?,
                    size:  arr.get(1)?.as_str()?.parse().ok()?,
                })
            }).collect()).unwrap_or_default()
        };
        Ok(OrderbookSnapshot {
            instrument_name: instrument_name.to_string(),
            bids: parse_levels(&result["b"]),
            asks: parse_levels(&result["a"]),
            timestamp: result["ts"].as_i64().unwrap_or(ts),
        })
    } else {
        Err(resp["retMsg"].as_str().unwrap_or("Unknown Bybit error").to_string())
    }
}

/// Map Bybit `type` field to canonical transaction_type.
fn map_bybit_type(t: &str) -> String {
    match t {
        "TRADE"               => "trade".to_string(),
        "SETTLEMENT"          => "settlement".to_string(),
        "DELIVERY_EXERCISE"   => "delivery".to_string(),
        "AUTO_DELEVERAGE"     => "delivery".to_string(),
        "LIQUIDATION"         => "liquidation".to_string(),
        "TRANSFER_IN"         => "transfer_in".to_string(),
        "TRANSFER_OUT"        => "transfer_out".to_string(),
        "INTEREST"            => "interest".to_string(),
        "FUNDING_FEE"         => "funding".to_string(),
        "FEE_REFUND"          => "fee_refund".to_string(),
        "BONUS_INCOME"        => "bonus_income".to_string(),
        // Unknown types: pass through as-is (lowercased) instead of "other"
        other                 => other.to_lowercase(),
    }
}

/// Extract the base coin from a Bybit symbol for grouping/filtering.
/// Examples:
///   "BTCUSDT"               → "BTC"
///   "BTCUSDC"               → "BTC"
///   "ETHUSDT"               → "ETH"
///   "BTCUSD"  (inverse)     → "BTC"
///   "BTC-10JAN25-50000-C"   → "BTC"
///   "SOLUSDT-PERP"          → "SOL"
///   ""                      → "" (pass-through for non-trade entries)
fn extract_base_coin(symbol: &str) -> String {
    if symbol.is_empty() { return String::new(); }
    // Option format: BASE-DDMMMYY-STRIKE-C/P
    if let Some(pos) = symbol.find('-') {
        return symbol[..pos].to_string();
    }
    // Strip known quote currencies (longest first to avoid partial matches)
    for quote in &["USDT", "USDC", "BUSD", "USD", "BTC", "ETH"] {
        if let Some(base) = symbol.strip_suffix(quote) {
            // Strip optional trailing suffixes like -PERP
            let base = base.trim_end_matches("-PERP");
            if !base.is_empty() {
                return base.to_string();
            }
        }
    }
    symbol.to_string()
}

/// Fetch transaction log from Bybit for a given account.
/// Bybit limits each request to a 7-day window — we chunk the full range automatically.
/// Enriches entries with markPrice/indexPrice from execution list and
/// computes per-currency equity by walking backward from current wallet balance.
pub async fn get_transaction_log(
    account: &Account,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<TransactionLog>, String> {
    const SEVEN_DAYS_MS: i64 = 7 * 24 * 60 * 60 * 1000;
    let client = Client::new();
    let mut all_logs: Vec<TransactionLog> = Vec::new();

    // Split the requested range into ≤7-day windows
    let mut chunk_start = start_ms;
    while chunk_start < end_ms {
        let chunk_end = (chunk_start + SEVEN_DAYS_MS).min(end_ms);

        let mut cursor = String::new();
        loop {
            let qs = if cursor.is_empty() {
                format!(
                    "accountType=UNIFIED&startTime={}&endTime={}&limit=200",
                    chunk_start, chunk_end
                )
            } else {
                format!(
                    "accountType=UNIFIED&startTime={}&endTime={}&limit=200&cursor={}",
                    chunk_start, chunk_end, cursor
                )
            };
            let url = format!("{}/v5/account/transaction-log?{}", base(account.testnet), qs);
            let headers = auth_get(&account.api_key, &account.api_secret, &qs);
            let resp: Value = match client.get(&url).headers(headers).send().await {
                Ok(r) => r.json().await.unwrap_or(Value::Null),
                Err(e) => return Err(format!("Bybit transaction-log request failed: {}", e)),
            };
            if resp["retCode"].as_i64() != Some(0) {
                let code = resp["retCode"].as_i64().unwrap_or(-1);
                let msg  = resp["retMsg"].as_str().unwrap_or("unknown");
                return Err(format!("Bybit transaction-log error {}: {}", code, msg));
            }
            let list = match resp["result"]["list"].as_array() {
                Some(l) => l.clone(),
                None => break,
            };
            for e in &list {
                let settle_ccy = e["currency"].as_str().unwrap_or("").to_string();
                let symbol     = e["symbol"].as_str().unwrap_or("");
                // Use base coin (BTC, ETH, SOL…) for grouping/filtering,
                // keep settlement currency (USDT, USDC…) in fee_currency.
                let base_coin  = if symbol.is_empty() {
                    settle_ccy.clone()
                } else {
                    extract_base_coin(symbol)
                };
                let side_raw = e["side"].as_str().unwrap_or("None");
                let side = match side_raw {
                    "Buy"  => "buy",
                    "Sell" => "sell",
                    _      => "",
                };
                let ttype = map_bybit_type(e["type"].as_str().unwrap_or(""));
                all_logs.push(TransactionLog {
                    id:                 e["id"].as_str().unwrap_or("").to_string(),
                    timestamp:          e["transactionTime"].as_str()
                                            .and_then(|s| s.parse::<i64>().ok())
                                            .unwrap_or(0),
                    instrument_name:    symbol.to_string(),
                    transaction_type:   ttype,
                    side:               side.to_string(),
                    amount:             e["qty"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    price:              e["tradePrice"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    fee:                e["fee"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    fee_currency:       settle_ccy.clone(),
                    currency:           base_coin.clone(),
                    profit_as_cashflow: e["cashFlow"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    balance:            e["cashBalance"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    change:             e["change"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    trade_id:           e["tradeId"].as_str().unwrap_or("").to_string(),
                    order_id:           e["orderId"].as_str().unwrap_or("").to_string(),
                    info:               String::new(),
                    mark_price:         0.0,
                    index_price:        0.0,
                    // Bybit's cashBalance already IS the post-transaction wallet balance.
                    // Equity (wallet + unrealized PnL) is not available in the tx log.
                    equity:             0.0,
                    position:          e["size" ].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    base_currency:      base_coin.clone(),
                    quote_currency:     settle_ccy.clone(),
                });
            }
            let next_cursor = resp["result"]["nextPageCursor"].as_str().unwrap_or("").to_string();
            if next_cursor.is_empty() || list.is_empty() { break; }
            cursor = next_cursor;
        }

        chunk_start = chunk_end + 1;
    }

    // ── Fetch deposits and withdrawals from asset endpoints ──────────────────
    // These are NOT in /v5/account/transaction-log — they need separate calls.
    let deposit_logs = fetch_deposit_withdrawal_logs(account, start_ms, end_ms, "deposit").await;
    let withdraw_logs = fetch_deposit_withdrawal_logs(account, start_ms, end_ms, "withdrawal").await;
    all_logs.extend(deposit_logs);
    all_logs.extend(withdraw_logs);

    all_logs.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    // ── Enrich with mark/index price from execution list ─────────────────────
    let exec_prices = fetch_execution_prices(account, start_ms, end_ms).await;
    for log in &mut all_logs {
        if let Some(&(mark, idx)) = exec_prices.get(&log.order_id) {
            log.mark_price  = mark;
            log.index_price = idx;
        }
    }

    // ── Backward equity calculation per settlement currency ───────────────────
    // Group entries by their settlement currency (fee_currency: USDT, USDC, …).
    // Fetch today's equity for each coin then walk newest→oldest, undoing each
    // transaction's `change` to reconstruct the running equity at every row.
    let settle_ccys: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        all_logs.iter()
            .filter_map(|l| {
                let c = l.fee_currency.clone();
                if !c.is_empty() && seen.insert(c.clone()) { Some(c) } else { None }
            })
            .collect()
    };
    for ccy in &settle_ccys {
        let current_equity = get_account_summary(ccy, account).await
            .map(|s| s.equity)
            .unwrap_or(0.0);
        if current_equity == 0.0 { continue; }
        let mut running = current_equity;
        // all_logs is sorted newest→oldest, so iterating forward walks backward in time
        for log in all_logs.iter_mut() {
            if &log.fee_currency == ccy {
                log.equity = running;
                running -= log.change; // undo this entry's change to get balance before it
            }
        }
    }

    Ok(all_logs)
}

/// Build orderId → (markPrice, indexPrice) map from execution list.
async fn fetch_execution_prices(
    account: &Account,
    start_ms: i64,
    end_ms: i64,
) -> HashMap<String, (f64, f64)> {
    const SEVEN_DAYS_MS: i64 = 7 * 24 * 60 * 60 * 1000;
    let client = Client::new();
    let mut map: HashMap<String, (f64, f64)> = HashMap::new();

    for category in &["option", "linear", "inverse"] {
        let mut chunk_start = start_ms;
        while chunk_start < end_ms {
            let chunk_end = (chunk_start + SEVEN_DAYS_MS).min(end_ms);
            let mut cursor = String::new();
            loop {
                let qs = if cursor.is_empty() {
                    format!("category={}&startTime={}&endTime={}&limit=100", category, chunk_start, chunk_end)
                } else {
                    format!("category={}&startTime={}&endTime={}&limit=100&cursor={}", category, chunk_start, chunk_end, cursor)
                };
                let url = format!("{}/v5/execution/list?{}", base(account.testnet), qs);
                let headers = auth_get(&account.api_key, &account.api_secret, &qs);
                let resp: Value = match client.get(&url).headers(headers).send().await {
                    Ok(r) => r.json().await.unwrap_or(Value::Null),
                    Err(_) => break,
                };
                if resp["retCode"].as_i64() != Some(0) { break; }
                let list = match resp["result"]["list"].as_array() {
                    Some(l) => l.clone(),
                    None => break,
                };
                for e in &list {
                    let order_id = e["orderId"].as_str().unwrap_or("").to_string();
                    if !order_id.is_empty() {
                        let mark  = sf64(&e["markPrice"]).unwrap_or(0.0);
                        let index = sf64(&e["indexPrice"]).unwrap_or(0.0);
                        map.entry(order_id).or_insert((mark, index));
                    }
                }
                let next = resp["result"]["nextPageCursor"].as_str().unwrap_or("").to_string();
                if next.is_empty() || list.is_empty() { break; }
                cursor = next;
            }
            chunk_start = chunk_end + 1;
        }
    }
    map
}

/// Fetch deposit or withdrawal records from Bybit asset endpoints.
/// `kind` must be either `"deposit"` or `"withdrawal"`.
async fn fetch_deposit_withdrawal_logs(
    account: &Account,
    start_ms: i64,
    end_ms:   i64,
    kind:     &str,
) -> Vec<TransactionLog> {
    let client = Client::new();
    let mut logs: Vec<TransactionLog> = Vec::new();
    let is_deposit = kind == "deposit";
    let tx_type    = if is_deposit { "deposit" } else { "withdrawal" };
    let path       = if is_deposit { "/v5/asset/deposit/query-record" } else { "/v5/asset/withdraw/query-record" };
    let mut cursor = String::new();

    // Deposit status codes: 0=unknown,1=ToBeConfirmed,2=Processing,3=Success,4=Failed
    let deposit_status_label = |v: &Value| -> String {
        match v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())) {
            Some(0) => "Unknown".into(), Some(1) => "Pending".into(),
            Some(2) => "Processing".into(), Some(3) => "Success".into(),
            Some(4) => "Failed".into(),
            _ => v.as_str().unwrap_or("").to_string(),
        }
    };

    loop {
        let qs = if cursor.is_empty() {
            format!("startTime={}&endTime={}&limit=50", start_ms, end_ms)
        } else {
            format!("startTime={}&endTime={}&limit=50&cursor={}", start_ms, end_ms, cursor)
        };
        let url = format!("{}{path}?{qs}", base(account.testnet));
        let headers = auth_get(&account.api_key, &account.api_secret, &qs);
        let resp: Value = match client.get(&url).headers(headers).send().await {
            Ok(r) => r.json().await.unwrap_or(Value::Null),
            Err(_) => break,
        };
        if resp["retCode"].as_i64() != Some(0) {
            eprintln!("[bybit] {} query error: {:?}", kind, resp["retMsg"]);
            break;
        }
        let rows = match resp["result"]["rows"].as_array() {
            Some(r) => r.clone(),
            None => break,
        };
        for row in &rows {
            let coin: String = row["coin"].as_str().unwrap_or("").to_string();
            let amount: f64  = row["amount"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let fee: f64     = if is_deposit {
                row["depositFee"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0)
            } else {
                row["withdrawFee"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0)
            };
            let change = if is_deposit { amount } else { -(amount + fee) };
            // Deposit uses successAt (ms string); withdrawal uses createTime
            let ts_field = if is_deposit { "successAt" } else { "createTime" };
            let ts: i64  = row[ts_field].as_str()
                .and_then(|s| s.parse().ok()).unwrap_or(0);
            let id = if is_deposit {
                row["txID"].as_str().unwrap_or("").to_string()
            } else {
                row["withdrawId"].as_str().unwrap_or("").to_string()
            };
            let status = if is_deposit {
                deposit_status_label(&row["status"])
            } else {
                row["status"].as_str().unwrap_or("").to_string()
            };

            logs.push(TransactionLog {
                id,
                timestamp:          ts,
                instrument_name:    String::new(),
                transaction_type:   tx_type.to_string(),
                side:               String::new(),
                amount,
                price:              0.0,
                fee,
                fee_currency:       coin.clone(),
                currency:           coin.clone(),
                profit_as_cashflow: change,
                balance:            0.0,
                change,
                trade_id:           String::new(),
                order_id:           String::new(),
                info:               status,
                mark_price:         0.0,
                index_price:        0.0,
                equity:             0.0,
                position:           0.0,
                base_currency:      coin.clone(),
                quote_currency:     coin.clone(),
            });
        }
        let next = resp["result"]["nextPageCursor"].as_str().unwrap_or("").to_string();
        if next.is_empty() || rows.is_empty() { break; }
        cursor = next;
    }
    logs
}
