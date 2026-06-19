/// CoInCall exchange API implementation
///
/// Auth scheme: HMAC-SHA256 (hex, UPPERCASE)
/// Prehash: `METHOD + URI + ? + sorted_user_params [&] + uuid=api_key&ts=ts_ms&x-req-ts-diff=5000`
/// Docs: https://docs.coincall.com/#coincall-api-v2-0-1
/// Testnet: https://beta.seizeyouralpha.com

use reqwest::{Client, header::{HeaderMap, HeaderValue, CONTENT_TYPE}};
use serde_json::{json, Value};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::atomic::{AtomicI64, Ordering};
use chrono::DateTime;

use crate::api::models::{
    Account, AccountSummary, Instrument, Order, OrderResult, OrderbookLevel, OrderbookSnapshot,
    PlaceOrderRequest, Position, Ticker, TickerStats, Trade, TransactionLog,
};

type HmacSha256 = Hmac<Sha256>;

const CC_BASE: &str    = "https://api.coincall.com";
const CC_TEST: &str    = "https://beta.seizeyouralpha.com";
const TS_DIFF: u64     = 5000;

/// Millisecond offset: server_time - local_time, updated by sync_server_time().
static TIME_OFFSET_MS: AtomicI64 = AtomicI64::new(0);

fn base(testnet: bool) -> &'static str {
    if testnet { CC_TEST } else { CC_BASE }
}

fn now_ms() -> u64 {
    let local = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64;
    (local + TIME_OFFSET_MS.load(Ordering::Relaxed)).max(0) as u64
}

/// Fetch CoInCall server time and store the clock offset so all subsequent
/// requests use a timestamp that matches the exchange.
pub async fn sync_server_time() {
    let url = format!("{}/time", CC_BASE);
    if let Ok(resp) = Client::new().get(&url).send().await {
        if let Ok(v) = resp.json::<Value>().await {
            let server_ms = v["data"]["serverTime"].as_i64()
                .or_else(|| v["data"].as_i64())
                .or_else(|| v["timestamp"].as_i64());
            if let Some(server_ms) = server_ms {
                let local_ms = SystemTime::now().duration_since(UNIX_EPOCH)
                    .unwrap_or_default().as_millis() as i64;
                let offset = server_ms - local_ms;
                TIME_OFFSET_MS.store(offset, Ordering::Relaxed);
                eprintln!("[coincall] server time offset: {}ms", offset);
            }
        }
    }
}

// ── Signing ────────────────────────────────────────────────────────────────

/// Compute CoInCall HMAC-SHA256 signature (UPPERCASE hex).
///
/// Prehash layout:
///   WITH user params:    `METHOD + URI + ?` + sorted_user_params + `&uuid=K&ts=T&x-req-ts-diff=D`
///   WITHOUT user params: `METHOD + URI + ?uuid=K&ts=T&x-req-ts-diff=D`
fn cc_sign(
    method: &str,
    uri: &str,
    sorted_user_params: &str,
    api_key: &str,
    ts: u64,
    secret: &str,
) -> String {
    let auth = format!("uuid={}&ts={}&x-req-ts-diff={}", api_key, ts, TS_DIFF);
    let prehash = if sorted_user_params.is_empty() {
        format!("{}{}?{}", method, uri, auth)
    } else {
        format!("{}{}?{}&{}", method, uri, sorted_user_params, auth)
    };
    eprintln!("[coincall] prehash: {}", prehash);
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(prehash.as_bytes());
    hex::encode(mac.finalize().into_bytes()).to_uppercase()
}

/// Sort `&[(key, value)]` pairs alphabetically → `k1=v1&k2=v2`
fn sorted_kv(pairs: &[(&str, String)]) -> String {
    let mut v: Vec<(&str, &str)> = pairs.iter().map(|(k, vv)| (*k, vv.as_str())).collect();
    v.sort_by_key(|(k, _)| *k);
    v.iter().map(|(k, vv)| format!("{}={}", k, vv)).collect::<Vec<_>>().join("&")
}

/// Extract sortable params from a JSON object body (remove nulls, sort keys).
fn sorted_json_params(body: &Value) -> String {
    let Some(obj) = body.as_object() else { return String::new(); };
    let mut pairs: Vec<(String, String)> = obj
        .iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| {
            let val = match v {
                Value::String(s)  => s.clone(),
                Value::Number(n)  => n.to_string(),
                Value::Bool(b)    => b.to_string(),
                other             => other.to_string(), // arrays / nested objects → compact JSON
            };
            (k.clone(), val)
        })
        .collect();
    pairs.sort_by_key(|(k, _)| k.clone());
    pairs.iter().map(|(k, v)| format!("{}={}", k, v)).collect::<Vec<_>>().join("&")
}

fn build_headers(api_key: &str, sig: &str, ts: u64) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("X-CC-APIKEY",    HeaderValue::from_str(api_key).unwrap_or_else(|_| HeaderValue::from_static("")));
    h.insert("sign",           HeaderValue::from_str(sig).unwrap_or_else(|_| HeaderValue::from_static("")));
    h.insert("ts",             HeaderValue::from_str(&ts.to_string()).unwrap());
    h.insert("X-REQ-TS-DIFF", HeaderValue::from_str(&TS_DIFF.to_string()).unwrap());
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    h
}

fn auth_get(api_key: &str, secret: &str, uri: &str, params: &[(&str, String)]) -> (HeaderMap, String) {
    let ts = now_ms();
    let sorted = sorted_kv(params);
    let sig = cc_sign("GET", uri, &sorted, api_key, ts, secret);
    let query = sorted; // already sorted for signing = same for request
    (build_headers(api_key, &sig, ts), query)
}

fn auth_post(api_key: &str, secret: &str, uri: &str, body: &Value) -> HeaderMap {
    let ts = now_ms();
    let sorted = sorted_json_params(body);
    let sig = cc_sign("POST", uri, &sorted, api_key, ts, secret);
    build_headers(api_key, &sig, ts)
}

// ── WebSocket URL (authenticated) ────────────────────────────────────────────
//
// CoInCall WS endpoint pattern:
//   Production:  wss://ws.coincall.com/{options|futures|spot}/ws
//   Testnet:     wss://betaws.seizeyouralpha.com/{options|futures|spot}/ws
//
// Sign prehash: `GET/users/self/verify?apiKey=KEY&ts=TS`

const WS_PROD: &str = "wss://ws.coincall.com";
const WS_TEST: &str = "wss://betaws.seizeyouralpha.com";

/// Map kind string to CoInCall WS/HTTP channel name.
pub fn kind_to_channel(kind: &str) -> &'static str {
    match kind {
        "option"    => "options",
        "future" | "perpetual" => "futures",
        _           => "spot",
    }
}

pub fn get_ws_url(account: &Account, kind: &str) -> Result<String, String> {
    let api_key = &account.api_key;
    let secret  = if account.api_secret.is_empty() {
        return Err("CoInCall account missing API secret".to_string());
    } else {
        &account.api_secret
    };
    let ts = now_ms();

    let prehash = format!("GET/users/self/verify?apiKey={}&ts={}", api_key, ts);
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).map_err(|e| e.to_string())?;
    mac.update(prehash.as_bytes());
    let sign = hex::encode(mac.finalize().into_bytes()).to_uppercase();

    let base_ws = if account.testnet { WS_TEST } else { WS_PROD };
    let channel = kind_to_channel(kind);
    // Spot endpoint has an extra /ws path segment; options and futures do not.
    let url = if channel == "spot" {
        format!("{}/{}/ws?code=10&uuid={}&ts={}&sign={}&apiKey={}", base_ws, channel, api_key, ts, sign, api_key)
    } else {
        format!("{}/{}?code=10&uuid={}&ts={}&sign={}&apiKey={}", base_ws, channel, api_key, ts, sign, api_key)
    };
    Ok(url)
}

// ── Instruments (public) ───────────────────────────────────────────────────

pub async fn fetch_instruments(currency: &str, kind: &str, testnet: bool) -> Result<Vec<Instrument>, String> {
    if kind == "option" {
        fetch_option_instruments(currency, testnet).await
    } else {
        fetch_future_instruments(currency, testnet).await
    }
}

async fn fetch_option_instruments(currency: &str, testnet: bool) -> Result<Vec<Instrument>, String> {
    let url = format!("{}/open/option/getInstruments/{}", base(testnet), currency.to_uppercase());
    let resp: Value = Client::new()
        .get(&url)
        .header(CONTENT_TYPE, "application/json")
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["code"].as_i64() != Some(0) {
        return Err(resp["msg"].as_str().unwrap_or("CoInCall error").to_string());
    }

    let instruments = resp["data"].as_array().ok_or("no data")?
        .iter()
        .filter(|i| i["isActive"].as_bool().unwrap_or(true))
        .map(|i| {
            let symbol = i["symbolName"].as_str().unwrap_or("").to_string();
            // symbol format: BTCUSD-14SEP23-22500-C  →  last segment is C or P
            let parts: Vec<&str> = symbol.split('-').collect();
            let option_type = parts.last().map(|t| match *t {
                "C" => "call".to_string(),
                _   => "put".to_string(),
            });
            Instrument {
                instrument_name:      symbol,
                kind:                 "option".to_string(),
                base_currency:        i["baseCurrency"].as_str().unwrap_or(currency).to_string(),
                quote_currency:       "USD".to_string(),
                settlement_currency:  "USD".to_string(),
                is_active:            i["isActive"].as_bool().unwrap_or(true),
                tick_size:            i["tickSize"].as_f64().unwrap_or(0.1),
                min_trade_amount:     i["minQty"].as_f64().unwrap_or(0.01),
                qty_step:             None,
                contract_size:        Some(0.01),
                option_type,
                strike:               i["strike"].as_f64(),
                expiration_timestamp: i["expirationTimestamp"].as_i64(),
            }
        })
        .collect();
    Ok(instruments)
}

async fn fetch_future_instruments(currency: &str, testnet: bool) -> Result<Vec<Instrument>, String> {
    // CoInCall futures config is in the public config endpoint
    let url = format!("{}/open/public/config/v1", base(testnet));
    let resp: Value = Client::new()
        .get(&url)
        .header(CONTENT_TYPE, "application/json")
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["code"].as_i64() != Some(0) {
        return Err(resp["msg"].as_str().unwrap_or("CoInCall error").to_string());
    }

    let mut instruments = Vec::new();
    if let Some(futures_cfg) = resp["data"]["futuresConfig"].as_object() {
        for (sym, cfg) in futures_cfg {
            let base_cur = cfg["base"].as_str().unwrap_or("BTC").to_string();
            if !base_cur.eq_ignore_ascii_case(currency) { continue; }
            let settle = cfg["settle"].as_str().unwrap_or("USD").to_string();
            instruments.push(Instrument {
                instrument_name:      sym.clone(),
                kind:                 "perpetual".to_string(),
                base_currency:        base_cur,
                quote_currency:       settle.clone(),
                settlement_currency:  settle,
                is_active:            true,
                tick_size:            cfg["tickSize"].as_f64().unwrap_or(0.1),
                min_trade_amount:     cfg["minQty"].as_f64().unwrap_or(0.001),
                qty_step:             None,
                contract_size:        None,
                option_type:          None,
                strike:               None,
                expiration_timestamp: None,
            });
        }
    }
    Ok(instruments)
}

// ── Index price (public) ───────────────────────────────────────────────────

/// Fetch the index price for a coin from CoInCall's public index endpoint.
/// coin: "BTC" | "ETH" etc.
pub async fn fetch_index_price(coin: &str, testnet: bool) -> Result<f64, String> {
    // CoInCall index endpoint: /open/public/index/v1?currency=BTC
    let url = format!("{}/open/public/index/v1?currency={}", base(testnet), coin.to_uppercase());
    let resp: Value = Client::new()
        .get(&url)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    eprintln!("[coincall] fetch_index_price {} resp: {}", coin, serde_json::to_string(&resp).unwrap_or_default());

    if resp["code"].as_i64() == Some(0) {
        if let Some(price) = resp["data"]["indexPrice"].as_f64()
            .or_else(|| resp["data"]["price"].as_f64())
            .or_else(|| resp["data"].as_f64())
        {
            return Ok(price);
        }
    }
    Err(resp["msg"].as_str().unwrap_or("CoInCall index price error").to_string())
}

// ── Ticker (public) ────────────────────────────────────────────────────────

/// Fetch option detail from GET /open/option/detail/v1/{symbol}.
/// Returns mark price, IV, greeks, and underlying index price.
pub async fn fetch_ticker(instrument_name: &str, testnet: bool) -> Result<Ticker, String> {
    let url = format!("{}/open/option/detail/v1/{}", base(testnet), instrument_name);

    let resp: Value = Client::new()
        .get(&url)
        .header(CONTENT_TYPE, "application/json")
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    eprintln!("[coincall] fetch_ticker {} resp: {}", instrument_name,
        serde_json::to_string(&resp).unwrap_or_default());

    if resp["code"].as_i64() != Some(0) {
        return Err(resp["msg"].as_str().unwrap_or("CoInCall ticker error").to_string());
    }

    let d = &resp["data"];

    Ok(Ticker {
        instrument_name:  instrument_name.to_string(),
        best_bid_price:   d["bidPrice"].as_f64(),
        best_ask_price:   d["askPrice"].as_f64(),
        best_bid_amount:  d["bidSize"].as_f64(),
        best_ask_amount:  d["askSize"].as_f64(),
        last_price:       d["lastPrice"].as_f64(),
        mark_price:       d["markPrice"].as_f64(),
        index_price:      d["indexPrice"].as_f64(),
        underlying_price: d["underlyingPrice"].as_f64(),
        open_interest:    d["openInterest"].as_f64(),
        stats: TickerStats {
            high:         d["price24hHigh"].as_f64(),
            low:          d["price24hLow"].as_f64(),
            price_change: d["changeRate"].as_f64(),
            volume:       d["volume24h"].as_f64(),
            volume_usd:   d["volumeUsd24h"].as_f64(),
        },
        mark_iv:  d["iv"].as_f64(),
        bid_iv:   d["bidIv"].as_f64(),
        ask_iv:   d["askIv"].as_f64(),
        delta:    d["delta"].as_f64(),
        gamma:    d["gamma"].as_f64(),
        vega:     d["vega"].as_f64(),
        theta:    d["theta"].as_f64(),
    })
}

// ── Enums / helpers ────────────────────────────────────────────────────────

fn trade_side(side: &str) -> i32 {
    if side.eq_ignore_ascii_case("buy") { 1 } else { 2 }
}

/// CoInCall tradeType: 1=LIMIT, 2=MARKET, 3=POST_ONLY, 4=STOP_LIMIT, 5=STOP_MARKET
fn trade_type(order_type: &str, post_only: bool) -> i32 {
    if post_only { return 3; }
    match order_type {
        "market"      => 2,
        "stop_limit"  => 4,
        "stop_market" => 5,
        _             => 1, // limit
    }
}

fn tif_code(tif: Option<&str>) -> Option<&'static str> {
    match tif {
        Some("immediate_or_cancel") => Some("IOC"),
        Some("fill_or_kill")        => Some("FOK"),
        Some("good_til_cancelled")  => Some("GTC"),
        _                           => None, // API defaults to GTC
    }
}

/// True for option symbols like `BTCUSD-10JAN25-89000-C` / `...-P`
fn is_option_symbol(symbol: &str) -> bool {
    symbol.ends_with("-C") || symbol.ends_with("-P")
}

/// Returns (create_uri, cancel_uri, pending_uri, history_uri) for the given symbol.
fn order_uris(symbol: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    if is_option_symbol(symbol) {
        (
            "/open/option/order/create/v1",
            "/open/option/order/cancel/v1",
            "/open/option/order/pending/v1",
            "/open/option/order/history/v1",
        )
    } else {
        (
            "/open/futures/order/create/v1",
            "/open/futures/order/cancel/v1",
            "/open/futures/order/pending/v1",
            "/open/futures/order/history/v1",
        )
    }
}

/// Parse an order from either options or futures pending/history response.
fn parse_order(o: &Value) -> Order {
    let side_num = o["tradeSide"].as_i64().unwrap_or(1);
    let type_num = o["tradeType"].as_i64().unwrap_or(1);
    Order {
        order_id:              o["orderId"].as_i64().unwrap_or(0).to_string(),
        instrument_name:       o["symbol"].as_str().unwrap_or("").to_string(),
        direction:             if side_num == 1 { "buy" } else { "sell" }.to_string(),
        order_type:            match type_num { 2 => "market", 3 => "limit_post", _ => "limit" }.to_string(),
        order_state:           "open".to_string(),
        price:                 o["price"].as_f64(),
        amount:                o["qty"].as_f64().unwrap_or(0.0),
        filled_amount:         o["fillQty"].as_f64().unwrap_or(0.0),
        average_price:         o["avgPrice"].as_f64(),
        post_only:             type_num == 3,
        time_in_force:         "good_til_cancelled".to_string(),
        creation_timestamp:    o["createTime"].as_i64().unwrap_or(0),
        last_update_timestamp: o["createTime"].as_i64().unwrap_or(0),
    }
}

// ── Place Order (SIGNED) ───────────────────────────────────────────────────

pub async fn place_order(req: &PlaceOrderRequest, account: &Account) -> Result<OrderResult, String> {
    let base_url = base(account.testnet);
    let uri = order_uris(&req.instrument_name).0;
    let ts = now_ms();

    let mut body = json!({
        "symbol":    req.instrument_name,
        "tradeSide": trade_side(&req.side),
        "tradeType": trade_type(&req.order_type, req.post_only.unwrap_or(false)),
        "qty":       req.amount,
    });

    if let Some(price) = req.price {
        if req.order_type != "market" {
            body["price"] = json!(price);
        }
    }

    if let Some(tif) = tif_code(req.time_in_force.as_deref()) {
        body["timeInForce"] = json!(tif);
    }
    // CoInCall clientOrderId field
    if let Some(ref clord_id) = req.client_order_id {
        body["clientOrderId"] = json!(clord_id);
    }

    let headers = auth_post(&account.api_key, &account.api_secret, uri, &body);
    let resp: Value = Client::new()
        .post(&format!("{}{}", base_url, uri))
        .headers(headers)
        .json(&body)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["code"].as_i64() != Some(0) {
        let err = resp["msg"].as_str().unwrap_or("CoInCall order error").to_string();
        return Ok(OrderResult { success: false, order: None, error: Some(err) });
    }

    let order_id = match &resp["data"] {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other            => other.to_string(),
    };

    let order = Order {
        order_id: order_id.clone(),
        instrument_name:      req.instrument_name.clone(),
        direction:            req.side.clone(),
        order_type:           req.order_type.clone(),
        order_state:          "open".to_string(),
        price:                req.price,
        amount:               req.amount,
        filled_amount:        0.0,
        average_price:        None,
        post_only:            req.post_only.unwrap_or(false),
        time_in_force:        req.time_in_force.clone().unwrap_or_else(|| "good_til_cancelled".to_string()),
        creation_timestamp:   ts as i64,
        last_update_timestamp: ts as i64,
    };
    Ok(OrderResult { success: true, order: Some(order), error: None })
}

// ── Cancel Order (SIGNED) ──────────────────────────────────────────────────

pub async fn cancel_order(
    order_id: &str,
    instrument_name: Option<&str>,
    account: &Account,
) -> Result<bool, String> {
    let base_url = base(account.testnet);
    let uri = order_uris(instrument_name.unwrap_or("")).1;

    // orderId is a numeric i64 on CoInCall
    let oid: i64 = order_id.parse().unwrap_or(0);
    let body = json!({ "orderId": oid });
    let headers = auth_post(&account.api_key, &account.api_secret, uri, &body);

    let resp: Value = Client::new()
        .post(&format!("{}{}", base_url, uri))
        .headers(headers)
        .json(&body)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    eprintln!("[coincall] cancel_order response: {}", resp);
    if resp["code"].as_i64() != Some(0) {
        let msg = resp["msg"].as_str().unwrap_or("Unknown CoInCall error");
        let code = resp["code"].as_i64().unwrap_or(-1);
        return Err(format!("CoInCall cancel error ({}): {}", code, msg));
    }
    Ok(true)
}

// ── Open Orders (SIGNED) ───────────────────────────────────────────────────

async fn fetch_pending_orders(uri: &str, symbol_filter: Option<&str>, account: &Account) -> Vec<Order> {
    let base_url = base(account.testnet);
    let mut params: Vec<(&str, String)> = vec![("pageSize", "200".to_string())];
    if let Some(sym) = symbol_filter {
        if !sym.is_empty() {
            params.push(("symbol", sym.to_string()));
        }
    }
    let (headers, query) = auth_get(&account.api_key, &account.api_secret, uri, &params);
    let resp: Value = match Client::new()
        .get(&format!("{}{}?{}", base_url, uri, query))
        .headers(headers)
        .send().await
    {
        Ok(r) => r.json().await.unwrap_or(Value::Null),
        Err(_) => Value::Null,
    };
    if resp["code"].as_i64() != Some(0) { return vec![]; }
    resp["data"]["list"].as_array().unwrap_or(&vec![]).iter().map(parse_order).collect()
}

pub async fn get_open_orders(instrument_name: &str, account: &Account) -> Result<Vec<Order>, String> {
    if instrument_name.is_empty() {
        // Fetch both option and futures open orders and merge
        let (opt, fut) = tokio::join!(
            fetch_pending_orders("/open/option/order/pending/v1",  None, account),
            fetch_pending_orders("/open/futures/order/pending/v1", None, account),
        );
        let mut all = opt;
        all.extend(fut);
        return Ok(all);
    }
    let uri = order_uris(instrument_name).2;
    let orders = fetch_pending_orders(uri, Some(instrument_name), account).await;
    Ok(orders)
}

/// Get ALL open orders (options + futures merged).
pub async fn get_all_open_orders(account: &Account) -> Result<Vec<Order>, String> {
    get_open_orders("", account).await
}

/// Get trade history (filled orders) for this account (options + futures merged).
pub async fn get_trade_history(account: &Account, start_ms: i64, end_ms: i64) -> Result<Vec<Trade>, String> {
    let base_url = base(account.testnet);
    let mut user_params: Vec<(&str, String)> = vec![("pageSize", "200".to_string())];
    if start_ms > 0 { user_params.push(("startTime", start_ms.to_string())); }
    if end_ms   > 0 { user_params.push(("endTime",   end_ms.to_string())); }

    let parse_resp = |resp: &Value| -> Vec<Trade> {
        if resp["code"].as_i64() != Some(0) { return vec![]; }
        resp["data"]["list"].as_array().unwrap_or(&vec![]).iter().map(|t| {
            let side_num = t["tradeSide"].as_i64().unwrap_or(1);
            Trade {
                trade_id:        t["tradeId"].as_str().map(|s| s.to_string())
                                 .or_else(|| t["tradeId"].as_i64().map(|n| n.to_string()))
                                 .unwrap_or_default(),
                account_id:      String::new(),
                account_name:    String::new(),
                exchange:        "coincall".to_string(),
                instrument_name: t["symbol"].as_str().unwrap_or("").to_string(),
                direction:       if side_num == 1 { "buy" } else { "sell" }.to_string(),
                amount:          t["qty"].as_f64().unwrap_or(0.0),
                price:           t["price"].as_f64().unwrap_or(0.0),
                fee:             t["fee"].as_f64().unwrap_or(0.0),
                fee_currency:    t["feeCurrency"].as_str().unwrap_or("USD").to_string(),
                timestamp:       t["createTime"].as_i64().unwrap_or(0),
                order_id:        t["orderId"].as_i64().map(|n| n.to_string()).unwrap_or_default(),
            }
        }).collect()
    };

    let (opt_h, opt_q) = auth_get(&account.api_key, &account.api_secret, "/open/option/order/history/v1", &user_params);
    let opt_resp: Value = match Client::new()
        .get(&format!("{}/open/option/order/history/v1?{}", base_url, opt_q))
        .headers(opt_h).send().await {
            Ok(r) => r.json().await.unwrap_or(Value::Null),
            Err(_) => Value::Null,
        };

    let (fut_h, fut_q) = auth_get(&account.api_key, &account.api_secret, "/open/futures/order/history/v1", &user_params);
    let fut_resp: Value = match Client::new()
        .get(&format!("{}/open/futures/order/history/v1?{}", base_url, fut_q))
        .headers(fut_h).send().await {
            Ok(r) => r.json().await.unwrap_or(Value::Null),
            Err(_) => Value::Null,
        };

    let mut trades = parse_resp(&opt_resp);
    trades.extend(parse_resp(&fut_resp));
    trades.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(trades)
}

pub async fn get_account_summary(currency: &str, account: &Account) -> Result<AccountSummary, String> {
    let base_url = base(account.testnet);
    let uri = "/open/account/summary/v1";
    let (headers, _) = auth_get(&account.api_key, &account.api_secret, uri, &[]);

    let resp: Value = Client::new()
        .get(&format!("{}{}", base_url, uri))
        .headers(headers)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["code"].as_i64() != Some(0) {
        return Err(resp["msg"].as_str().unwrap_or("CoInCall error").to_string());
    }

    let d = &resp["data"];

    // Find the requested currency in the accounts array; fall back to summary-level fields
    let (equity, avail, im, mm, upnl) = if let Some(accs) = d["accounts"].as_array() {
        let acc = accs.iter().find(|a| {
            a["coin"].as_str().map(|c| c.eq_ignore_ascii_case(currency)).unwrap_or(false)
        });
        if let Some(a) = acc {
            let parse = |v: &Value| v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_f64()).unwrap_or(0.0);
            (parse(&a["equityAmount"]), parse(&a["availableBalance"]),
             parse(&a["imAmount"]), parse(&a["mmAmount"]), parse(&a["unrealizedAmount"]))
        } else {
            // Coin not found in accounts array → return zeros (not account-level totals
            // which would show wrong large numbers against a zero-equity coin label)
            (0.0, 0.0, 0.0, 0.0, 0.0)
        }
    } else {
        parse_summary_top(d)
    };

    Ok(AccountSummary {
        currency:           currency.to_string(),
        equity,
        available_funds:    avail,
        initial_margin:     im,
        maintenance_margin: mm,
        unrealized_pl:      upnl,
    })
}

fn parse_summary_top(d: &Value) -> (f64, f64, f64, f64, f64) {
    let p = |key: &str| d[key].as_str().and_then(|s| s.parse().ok()).or_else(|| d[key].as_f64()).unwrap_or(0.0);
    (p("equity"), p("availableMargin"), p("imAmount"), p("mmAmount"), p("unrealizedPnL"))
}

/// Fetch per-coin wallet balances from /open/account/summary/v1.
/// Returns `(coin_balance, usdt_balance)` where coin_balance is the equity of the first
/// non-USDT coin found, and usdt_balance is the USDT equity.
pub async fn get_full_balances(account: &Account) -> Result<(f64, f64), String> {
    let base_url = base(account.testnet);
    let uri = "/open/account/summary/v1";
    let (headers, _) = auth_get(&account.api_key, &account.api_secret, uri, &[]);
    let resp: Value = Client::new()
        .get(&format!("{}{}", base_url, uri))
        .headers(headers).send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;
    if resp["code"].as_i64() != Some(0) {
        return Err(resp["msg"].as_str().unwrap_or("balance error").to_string());
    }
    let parse = |v: &Value| v.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| v.as_f64()).unwrap_or(0.0);
    let mut coin_balance = 0.0_f64;
    let mut usdt_balance = 0.0_f64;
    if let Some(accs) = resp["data"]["accounts"].as_array() {
        for a in accs {
            let coin = a["coin"].as_str().unwrap_or("").to_ascii_uppercase();
            let equity = parse(&a["equityAmount"]);
            if coin == "USDT" || coin == "USD" {
                usdt_balance += equity;
            } else if equity.abs() > 0.0 {
                coin_balance += equity;  // accumulate all non-stable coin balances
            }
        }
    }
    Ok((coin_balance, usdt_balance))
}

/// Get open positions (options + futures).
pub async fn get_positions(_currency: &str, account: &Account) -> Result<Vec<Position>, String> {
    let base_url = base(account.testnet);
    let client = Client::new();
    let mut positions: Vec<Position> = Vec::new();

    let parse = |v: &Value| v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_f64()).unwrap_or(0.0);

    // ── Options positions ────────────────────────────────────────────────────
    {
        let uri = "/open/option/position/get/v1";
        let (headers, _) = auth_get(&account.api_key, &account.api_secret, uri, &[]);
        if let Ok(resp) = client.get(&format!("{}{}", base_url, uri))
            .headers(headers).send().await
        {
            if let Ok(v) = resp.json::<Value>().await {
                if v["code"].as_i64() == Some(0) {
                    if let Some(arr) = v["data"].as_array() {
                        for p in arr {
                            let size = parse(&p["qty"]);
                            if size == 0.0 { continue; }
                            let trade_side = p["tradeSide"].as_i64().unwrap_or(1);
                            positions.push(Position {
                                instrument_name: p["symbol"].as_str().unwrap_or("").to_string(),
                                direction:       if trade_side == 1 { "long".to_string() } else { "short".to_string() },
                                size,
                                average_price:   parse(&p["avgPrice"]),
                                mark_price:      parse(&p["markPrice"]),
                                mark_iv:         parse(&p["markIv"]),
                                unrealized_pnl:  parse(&p["upnl"]),
                                delta:           parse(&p["delta"]),
                                gamma:           parse(&p["gamma"]),
                                theta:           parse(&p["theta"]),
                                vega:            parse(&p["vega"]),
                            });
                        }
                    }
                } else {
                    eprintln!("[coincall] options positions error: {}", v);
                }
            }
        }
    }

    // ── Futures positions ────────────────────────────────────────────────────
    {
        let uri = "/open/futures/position/get/v1";
        let (headers, _) = auth_get(&account.api_key, &account.api_secret, uri, &[]);
        if let Ok(resp) = client.get(&format!("{}{}", base_url, uri))
            .headers(headers).send().await
        {
            if let Ok(v) = resp.json::<Value>().await {
                if v["code"].as_i64() == Some(0) {
                    if let Some(arr) = v["data"].as_array() {
                        for p in arr {
                            let size = parse(&p["qty"]).abs();
                            if size == 0.0 { continue; }
                            let trade_side = p["tradeSide"].as_i64().unwrap_or(1);
                            positions.push(Position {
                                instrument_name: p["symbol"].as_str().unwrap_or("").to_string(),
                                direction:       if trade_side == 1 { "long".to_string() } else { "short".to_string() },
                                size,
                                average_price:   parse(&p["avgPrice"]),
                                mark_price:      parse(&p["markPrice"]),
                                mark_iv:         0.0,
                                unrealized_pnl:  parse(&p["upnl"]),
                                delta:           parse(&p["delta"]),
                                gamma:           0.0,
                                theta:           0.0,
                                vega:            0.0,
                            });
                        }
                    }
                } else {
                    eprintln!("[coincall] futures positions error: {}", v);
                }
            }
        }
    }

    Ok(positions)
}

// ── RFQ / Block Trade (SIGNED) ─────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RfqLeg {
    pub instrument_name: String,
    pub side: String,
    pub qty: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RfqResponse {
    pub request_id: String,
    pub create_time: i64,
    pub expiry_time:  i64,
    pub legs:  Vec<RfqLeg>,
    pub state: String,
}

/// Create an RFQ (multi-leg block trade request).
pub async fn create_rfq(legs: Vec<RfqLeg>, account: &Account) -> Result<RfqResponse, String> {
    let base_url = base(account.testnet);
    let uri = "/open/option/blocktrade/request/create/v1";

    let legs_json: Vec<Value> = legs.iter().map(|l| json!({
        "instrumentName": l.instrument_name,
        "side": l.side,
        "qty":  l.qty,
    })).collect();

    let body = json!({ "legs": legs_json });
    // For signing, the legs array is serialised as a JSON string value
    // This follows Example 2 pattern where array values are stringified inline
    let headers = auth_post(&account.api_key, &account.api_secret, uri, &body);

    let resp: Value = Client::new()
        .post(&format!("{}{}", base_url, uri))
        .headers(headers)
        .json(&body)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["code"].as_i64() != Some(0) {
        return Err(resp["msg"].as_str().unwrap_or("RFQ error").to_string());
    }

    let d = &resp["data"];
    let rfq_legs: Vec<RfqLeg> = d["legs"].as_array().unwrap_or(&vec![])
        .iter()
        .map(|l| RfqLeg {
            instrument_name: l["instrumentName"].as_str().unwrap_or("").to_string(),
            side:            l["side"].as_str().unwrap_or("").to_string(),
            qty:             l["qty"].as_str().unwrap_or("0").to_string(),
        })
        .collect();

    Ok(RfqResponse {
        request_id:  d["requestId"].as_str().unwrap_or("").to_string(),
        create_time: d["createTime"].as_i64().unwrap_or(0),
        expiry_time:  d["expiryTime"].as_i64().unwrap_or(0),
        legs:  rfq_legs,
        state: d["state"].as_str().unwrap_or("").to_string(),
    })
}

/// Cancel an active RFQ.
pub async fn cancel_rfq(request_id: &str, account: &Account) -> Result<bool, String> {
    let base_url = base(account.testnet);
    let uri = "/open/option/blocktrade/request/cancel/v1";
    let body = json!({ "requestId": request_id });
    let headers = auth_post(&account.api_key, &account.api_secret, uri, &body);

    let resp: Value = Client::new()
        .post(&format!("{}{}", base_url, uri))
        .headers(headers)
        .json(&body)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    Ok(resp["code"].as_i64() == Some(0))
}

/// Get list of user's RFQs.
/// `role`: "MAKER" to see incoming seeks from takers, "TAKER" to see your own seeks, None for all.
pub async fn get_rfq_list(account: &Account, role: Option<&str>, rfq_state: Option<&str>) -> Result<Value, String> {
    let base_url = base(account.testnet);
    let uri = "/open/option/blocktrade/rfqList/v1";

    let mut params: Vec<(&str, String)> = vec![];
    if let Some(r) = role {
        params.push(("role", r.to_string()));
    }
    // Allow explicit state override; default MAKER→OPEN, history→CLOSE
    if let Some(s) = rfq_state {
        params.push(("state", s.to_string()));
    } else if role == Some("MAKER") {
        params.push(("state", "OPEN".to_string()));
    }

    let (headers, query) = auth_get(&account.api_key, &account.api_secret, uri, &params);
    let url = if query.is_empty() {
        format!("{}{}", base_url, uri)
    } else {
        format!("{}{}?{}", base_url, uri, query)
    };

    let resp: Value = Client::new()
        .get(&url)
        .headers(headers)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    eprintln!("[coincall] get_rfq_list({:?}) resp: {}", role, serde_json::to_string(&resp).unwrap_or_default());

    if resp["code"].as_i64() != Some(0) {
        return Err(resp["msg"].as_str().unwrap_or("RFQ list error").to_string());
    }

    // Normalise legs: API returns `quantity` in list but `qty` in create response.
    // Map everything to `qty` so the frontend type is consistent.
    let rfq_list = resp["data"]["rfqList"].clone();
    if let Some(arr) = rfq_list.as_array() {
        let normalised: Vec<Value> = arr.iter().map(|rfq| {
            let mut r = rfq.clone();
            if let Some(legs) = r["legs"].as_array() {
                let fixed: Vec<Value> = legs.iter().map(|leg| {
                    let mut l = leg.clone();
                    // If `qty` missing but `quantity` present, copy it over
                    if l["qty"].is_null() || !l["qty"].is_string() {
                        if let Some(q) = l["quantity"].as_str() {
                            l["qty"] = Value::String(q.to_string());
                        }
                    }
                    l
                }).collect();
                r["legs"] = Value::Array(fixed);
            }
            r
        }).collect();
        return Ok(Value::Array(normalised));
    }

    Ok(rfq_list)
}

/// Create a quote (maker submits prices for an incoming RFQ seek).
pub async fn create_quote(
    request_id: &str,
    quote_side: Option<&str>,
    legs: &[Value],
    account: &Account,
) -> Result<Value, String> {
    let base_url = base(account.testnet);
    let uri = "/open/option/blocktrade/quote/create/v1";

    let mut body = json!({
        "requestId": request_id.parse::<i64>().unwrap_or(0),
        "legs": legs,
    });
    if let Some(qs) = quote_side {
        body["quoteSide"] = Value::String(qs.to_string());
    }

    let headers = auth_post(&account.api_key, &account.api_secret, uri, &body);

    let resp: Value = Client::new()
        .post(&format!("{}{}", base_url, uri))
        .headers(headers)
        .json(&body)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    eprintln!("[coincall] create_quote resp: {}", serde_json::to_string(&resp).unwrap_or_default());

    if resp["code"].as_i64() != Some(0) {
        return Err(resp["msg"].as_str().unwrap_or("Create quote error").to_string());
    }

    Ok(resp["data"].clone())
}

/// Cancel a maker quote by quoteId.
pub async fn cancel_quote(quote_id: &str, account: &Account) -> Result<(), String> {
    let base_url = base(account.testnet);
    let uri = "/open/option/blocktrade/quote/cancel/v1";
    let body = json!({
        "quoteId": quote_id.parse::<i64>().unwrap_or(0),
    });
    let headers = auth_post(&account.api_key, &account.api_secret, uri, &body);
    let resp: Value = Client::new()
        .post(&format!("{}{}", base_url, uri))
        .headers(headers)
        .json(&body)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;
    eprintln!("[coincall] cancel_quote resp: {}", serde_json::to_string(&resp).unwrap_or_default());
    if resp["code"].as_i64() != Some(0) {
        return Err(resp["msg"].as_str().unwrap_or("Cancel quote error").to_string());
    }
    Ok(())
}

/// Get quotes for a specific RFQ seek.
pub async fn get_rfq_quotes(account: &Account, request_id: Option<&str>) -> Result<Value, String> {
    let base_url = base(account.testnet);
    // list-quote/v1 is the documented endpoint for a maker to see quotes on a seek
    let uri = "/open/option/blocktrade/list-quote/v1";

    let mut params: Vec<(&str, String)> = vec![];
    if let Some(rid) = request_id.filter(|s| !s.is_empty()) {
        params.push(("requestId", rid.to_string()));
    }
    // state=OPEN limits to live quotes only
    params.push(("state", "OPEN".to_string()));

    let (headers, query) = auth_get(&account.api_key, &account.api_secret, uri, &params);
    let url = format!("{}{}?{}", base_url, uri, query);

    eprintln!("[coincall] get_rfq_quotes GET {}", url);

    let resp: Value = Client::new()
        .get(&url)
        .headers(headers)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    eprintln!("[coincall] get_rfq_quotes resp: {}", serde_json::to_string(&resp).unwrap_or_default());

    if resp["code"].as_i64() != Some(0) {
        return Err(resp["msg"].as_str().unwrap_or("RFQ quotes error").to_string());
    }

    let data = &resp["data"];
    if data.is_array() {
        Ok(data.clone())
    } else if data["quoteList"].is_array() {
        Ok(data["quoteList"].clone())
    } else if data["list"].is_array() {
        Ok(data["list"].clone())
    } else {
        Ok(data.clone())
    }
}

/// Accept a specific quote for an RFQ.
pub async fn accept_quote(request_id: &str, quote_id: &str, account: &Account) -> Result<bool, String> {
    let base_url = base(account.testnet);
    let uri = "/open/option/blocktrade/request/accept/v1";
    let body = json!({ "requestId": request_id, "quoteId": quote_id });
    let headers = auth_post(&account.api_key, &account.api_secret, uri, &body);

    let resp: Value = Client::new()
        .post(&format!("{}{}", base_url, uri))
        .headers(headers)
        .json(&body)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    Ok(resp["code"].as_i64() == Some(0))
}

pub async fn fetch_orderbook(instrument_name: &str, depth: u32, testnet: bool) -> Result<OrderbookSnapshot, String> {
    let client = Client::new();
    let url = format!(
        "{}/md/orderbook?instrumentId={}&depth={}",
        base(testnet), instrument_name, depth
    );

    let resp: Value = client.get(&url).send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    if resp["code"].as_i64() != Some(0) {
        return Err(resp["msg"].as_str().unwrap_or("CoInCall orderbook error").to_string());
    }

    let result = &resp["data"];
    let parse_levels = |arr: &Value| -> Vec<OrderbookLevel> {
        arr.as_array().map(|a| a.iter().filter_map(|item| {
            let arr = item.as_array()?;
            Some(OrderbookLevel {
                price: arr.get(0)?.as_f64()?,
                size:  arr.get(1)?.as_f64()?,
            })
        }).collect()).unwrap_or_default()
    };

    Ok(OrderbookSnapshot {
        instrument_name: instrument_name.to_string(),
        bids: parse_levels(&result["bids"]),
        asks: parse_levels(&result["asks"]),
        timestamp: ts,
    })
}

/// Fetch transaction history from CoInCall, mapping trade history to `TransactionLog`.
/// Attempts to extract markPrice/indexPrice from the raw response fields.
/// Backward equity is computed per-currency using current account summary as seed.
/// Parse ISO-8601 timestamp string to milliseconds (e.g. "2025-01-18T15:36:43.199+00:00").
fn parse_iso_ms(s: &str) -> i64 {
    DateTime::parse_from_rfc3339(s)
        .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f%z"))
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
}

/// Parse base and quote currency from a CoInCall instrument name.
/// Handles dashed formats ("BTC-PERP", "BTC-USDT-PERP", "BTC-20231231-30000-C")
/// and concatenated spot formats ("BTCUSDT").
fn parse_base_quote(symbol: &str) -> (String, String) {
    if symbol.is_empty() { return (String::new(), String::new()); }
    if let Some(dash) = symbol.find('-') {
        let base = symbol[..dash].to_string();
        let second_seg = symbol[dash+1..].split('-').next().unwrap_or("");
        let quote = match second_seg {
            "USDT" | "USDC" | "USD" | "BTC" | "ETH" | "BUSD" => second_seg.to_string(),
            _ => "USD".to_string(),
        };
        return (base, quote);
    }
    for suffix in &["USDT", "USDC", "BUSD", "USD", "BTC", "ETH"] {
        if symbol.ends_with(suffix) && symbol.len() > suffix.len() {
            return (symbol[..symbol.len() - suffix.len()].to_string(), suffix.to_string());
        }
    }
    (symbol.to_string(), String::new())
}

pub async fn get_transaction_log(
    account: &Account,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<TransactionLog>, String> {
    const SEVEN_DAYS_MS: i64 = 7 * 24 * 60 * 60 * 1000;

    let requested_end = if end_ms > 0 { end_ms } else { now_ms() as i64 };
    let requested_start = if start_ms > 0 { start_ms } else { requested_end - SEVEN_DAYS_MS };
    let normalized_start = requested_start.min(requested_end);

    let mut logs: Vec<TransactionLog> = Vec::new();
    let mut window_start = normalized_start;
    while window_start <= requested_end {
        // ponytail: fixed-size 7-day windows; add concurrent fetch if this becomes slow.
        let window_end = (window_start + SEVEN_DAYS_MS - 1).min(requested_end);
        let mut chunk = get_transaction_log_window(account, window_start, window_end).await?;
        logs.append(&mut chunk);
        if window_end == requested_end {
            break;
        }
        window_start = window_end + 1;
    }

    Ok(logs)
}

async fn get_transaction_log_window(
    account: &Account,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<TransactionLog>, String> {
    let base_url = base(account.testnet);
    let client = Client::new();
    let mut logs: Vec<TransactionLog> = Vec::new();

    // ── 1. Trade fills: options + futures + spot ─────────────────────────────
    {
        // ── 1a. Options: /open/option/trade/history/v1 ────────────────────────
        // Pagination via `fromId` cursor; fields: time, tradeSide, price, qty,
        // fee, markPrice, indexPrice, tradeId, orderId, symbol, feeCurrency
        {
            let uri = "/open/option/trade/history/v1";
            let mut from_id: Option<i64> = None;
            loop {
                let mut params: Vec<(&str, String)> = vec![("pageSize", "200".to_string())];
                if start_ms > 0 { params.push(("startTime", start_ms.to_string())); }
                if end_ms   > 0 { params.push(("endTime",   end_ms.to_string())); }
                if let Some(id) = from_id { params.push(("fromId", id.to_string())); }

                let (hdr, qs) = auth_get(&account.api_key, &account.api_secret, uri, &params);
                let resp: Value = match client.get(&format!("{}{uri}?{qs}", base_url))
                    .headers(hdr).send().await {
                        Ok(r) => r.json().await.unwrap_or(Value::Null),
                        Err(_) => break,
                    };
                if resp["code"].as_i64() != Some(0) { break; }

                let list = match resp["data"]["list"].as_array() {
                    Some(l) if !l.is_empty() => l.clone(),
                    _ => break,
                };

                let mut min_id: Option<i64> = None;
                for t in &list {
                    let ts = t["time"].as_i64().unwrap_or(0);
                    if ts < start_ms || ts > end_ms { continue; }
                    let trade_id = t["tradeId"].as_i64().map(|n| n.to_string())
                                    .or_else(|| t["tradeId"].as_str().map(|s| s.to_string()))
                                    .unwrap_or_default();
                    let order_id = t["orderId"].as_i64().map(|n| n.to_string())
                                    .or_else(|| t["orderId"].as_str().map(|s| s.to_string()))
                                    .unwrap_or_default();
                    let row_id   = t["id"].as_i64().unwrap_or(0);
                    min_id = Some(min_id.map_or(row_id, |m: i64| m.min(row_id)));

                    let fee_ccy    = t["feeCurrency"].as_str().unwrap_or("USD").to_string();
                    let mark_price  = t["markPrice"].as_f64().or_else(|| t["markPrice"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0);
                    let index_price = t["indexPrice"].as_f64().or_else(|| t["indexPrice"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0);
                    let side = if t["tradeSide"].as_i64().unwrap_or(1) == 1 { "buy" } else { "sell" };
                    let sym = t["symbol"].as_str().unwrap_or("");
                    let (base_currency, quote_currency) = parse_base_quote(sym);
                    logs.push(TransactionLog {
                        id:                 trade_id.clone(),
                        timestamp:          ts,
                        instrument_name:    sym.to_string(),
                        transaction_type:   "trade".to_string(),
                        category:           String::new(),
                        side:               side.to_string(),
                        amount:             t["qty"].as_f64().unwrap_or(0.0),
                        price:              t["price"].as_f64().unwrap_or(0.0),
                        fee:                t["fee"].as_f64().unwrap_or(0.0),
                        fee_currency:       fee_ccy.clone(),
                        currency:           fee_ccy,
                        profit_as_cashflow: 0.0,
                        balance:            0.0,
                        change:             0.0,
                        trade_id,
                        order_id,
                        info:               String::new(),
                        mark_price,
                        index_price,
                        equity:             0.0,
                        position:           t["qty"].as_f64().unwrap_or(0.0) * t["price"].as_f64().unwrap_or(0.0),
                        base_currency,
                        quote_currency,
                        funding:            0.0,
                    });
                }

                let has_next = resp["data"]["hasNext"].as_bool().unwrap_or(false);
                if !has_next || min_id.is_none() { break; }
                from_id = min_id; // next page starts from the smallest id seen
            }
        }

        // ── 1b. Futures:/open/futures/trade/history/v1 ───────────────────────
        // Same pagination style; fields: time, tradeSide, price, qty, fee,
        // markPrice, indexPrice, tradeId, orderId, symbol
        {
            let uri = "/open/futures/trade/history/v1";
            let mut from_id: Option<i64> = None;
            loop {
                let mut params: Vec<(&str, String)> = vec![("pageSize", "200".to_string())];
                if start_ms > 0 { params.push(("startTime", start_ms.to_string())); }
                if end_ms   > 0 { params.push(("endTime",   end_ms.to_string())); }
                if let Some(id) = from_id { params.push(("fromId", id.to_string())); }

                let (hdr, qs) = auth_get(&account.api_key, &account.api_secret, uri, &params);
                let resp: Value = match client.get(&format!("{}{uri}?{qs}", base_url))
                    .headers(hdr).send().await {
                        Ok(r) => r.json().await.unwrap_or(Value::Null),
                        Err(_) => break,
                    };
                eprintln!("[coincall] futures trade history resp: {}", serde_json::to_string(&resp).unwrap_or_default());
                if resp["code"].as_i64() != Some(0) { break; }

                let list = match resp["data"]["list"].as_array() {
                    Some(l) if !l.is_empty() => l.clone(),
                    _ => break,
                };

                let mut min_id: Option<i64> = None;
                for t in &list {
                    let ts = t["time"].as_i64().unwrap_or(0);
                    if ts < start_ms || ts > end_ms { continue; }
                    let trade_id = t["tradeId"].as_i64().map(|n| n.to_string())
                                    .or_else(|| t["tradeId"].as_str().map(|s| s.to_string()))
                                    .unwrap_or_default();
                    let order_id = t["orderId"].as_i64().map(|n| n.to_string())
                                    .or_else(|| t["orderId"].as_str().map(|s| s.to_string()))
                                    .unwrap_or_default();
                    let row_id   = t["id"].as_i64().unwrap_or(0);
                    min_id = Some(min_id.map_or(row_id, |m: i64| m.min(row_id)));

                    let mark_price  = t["markPrice"].as_f64().or_else(|| t["markPrice"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0);
                    let index_price = t["indexPrice"].as_f64().or_else(|| t["indexPrice"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0);
                    let side = if t["tradeSide"].as_i64().unwrap_or(1) == 1 { "buy" } else { "sell" };
                    let sym = t["symbol"].as_str().unwrap_or("");
                    let (base_currency, quote_currency) = parse_base_quote(sym);
                    logs.push(TransactionLog {
                        id:                 trade_id.clone(),
                        timestamp:          ts,
                        instrument_name:    sym.to_string(),
                        transaction_type:   "trade".to_string(),
                        category:           String::new(),
                        side:               side.to_string(),
                        amount:             t["qty"].as_f64().unwrap_or(0.0),
                        price:              t["price"].as_f64().unwrap_or(0.0),
                        fee:                t["fee"].as_f64().unwrap_or(0.0),
                        fee_currency:       "USD".to_string(),
                        currency:           "USD".to_string(),
                        profit_as_cashflow: 0.0,
                        balance:            0.0,
                        change:             0.0,
                        trade_id,
                        order_id,
                        info:               String::new(),
                        mark_price,
                        index_price,
                        equity:             0.0,
                        position:           t["qty"].as_f64().unwrap_or(0.0) * t["price"].as_f64().unwrap_or(0.0),
                        base_currency,
                        quote_currency,
                        funding:            0.0,
                    });
                }

                let has_next = resp["data"]["hasNext"].as_bool().unwrap_or(false);
                if !has_next || min_id.is_none() { break; }
                from_id = min_id;
            }
        }

        // ── 1c. Spot: /open/spot/trade/fills/v1 ───────────────────────────────
        // Time-range based pagination; fields: ts, tradeSide, price, qty, fee,
        // feeCurrency, tradeId, orderId, symbol (limit up to 1000)
        {
            let uri = "/open/spot/trade/fills/v1";
            let params: Vec<(&str, String)> = vec![
                ("limit",     "1000".to_string()),
                ("startTime", start_ms.to_string()),
                ("endTime",   end_ms.to_string()),
            ];
            let (hdr, qs) = auth_get(&account.api_key, &account.api_secret, uri, &params);
            let resp: Value = match client.get(&format!("{}{uri}?{qs}", base_url))
                .headers(hdr).send().await {
                    Ok(r) => r.json().await.unwrap_or(Value::Null),
                    Err(_) => Value::Null,
                };
            if resp["code"].as_i64() == Some(0) {
                for t in resp["data"].as_array().unwrap_or(&vec![]) {
                    let ts = t["ts"].as_i64().unwrap_or(0);
                    if ts < start_ms || ts > end_ms { continue; }
                    let trade_id = t["tradeId"].as_str().map(|s| s.to_string())
                                    .or_else(|| t["tradeId"].as_i64().map(|n| n.to_string()))
                                    .unwrap_or_default();
                    let order_id = t["orderId"].as_i64().map(|n| n.to_string())
                                    .or_else(|| t["orderId"].as_str().map(|s| s.to_string()))
                                    .unwrap_or_default();
                    let fee_ccy = t["feeCurrency"].as_str().unwrap_or("").to_string();
                    let symbol  = t["symbol"].as_str().unwrap_or("");
                    // For spot, currency is the base coin (extracted from feeCurrency or symbol)
                    let base_ccy = if !fee_ccy.is_empty() { fee_ccy.clone() } else { symbol.to_string() };
                    let side = if t["tradeSide"].as_i64().unwrap_or(1) == 1 { "buy" } else { "sell" };
                    let (base_currency, quote_currency) = parse_base_quote(symbol);
                    logs.push(TransactionLog {
                        id:                 trade_id.clone(),
                        timestamp:          ts,
                        instrument_name:    symbol.to_string(),
                        transaction_type:   "trade".to_string(),
                        category:           String::new(),
                        side:               side.to_string(),
                        amount:             t["qty"].as_f64().or_else(|| t["qty"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0),
                        price:              t["price"].as_f64().or_else(|| t["price"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0),
                        fee:                t["fee"].as_f64().or_else(|| t["fee"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0),
                        fee_currency:       fee_ccy,
                        currency:           base_ccy,
                        profit_as_cashflow: 0.0,
                        balance:            0.0,
                        change:             0.0,
                        trade_id,
                        order_id,
                        info:               String::new(),
                        mark_price:         0.0,
                        index_price:        0.0,
                        equity:             0.0,
                        position:           t["qty"].as_f64().or_else(|| t["qty"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0) * t["price"].as_f64().or_else(|| t["price"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0),
                        base_currency,
                        quote_currency,
                        funding:            0.0,
                    });
                }
            }
        }
    }

    // ── 2. Deposit / Withdrawal history ─────────────────────────────────────
    {
        let mut page = 1u32;
        loop {
            let params: Vec<(&str, String)> = vec![
                ("type",      "-1".to_string()),     // -1 = all (deposit + withdrawal)
                ("page",      page.to_string()),
                ("pageSize",  "50".to_string()),
                ("startTime", start_ms.to_string()),
                ("endTime",   end_ms.to_string()),
            ];
            let (hdr, qs) = auth_get(&account.api_key, &account.api_secret, "/open/account/historyList/v1", &params);
            let resp: Value = match client.get(&format!("{}/open/account/historyList/v1?{}", base_url, qs))
                .headers(hdr).send().await {
                    Ok(r) => r.json().await.unwrap_or(Value::Null),
                    Err(_) => break,
                };

            if resp["code"].as_i64() != Some(0) { break; }

            let list = match resp["data"]["list"].as_array() {
                Some(l) => l.clone(),
                None    => break,
            };
            if list.is_empty() { break; }

            for entry in &list {
                let ts = entry["createTime"].as_str()
                    .map(|s| parse_iso_ms(s))
                    .or_else(|| entry["createTime"].as_i64())
                    .unwrap_or(0);
                if ts < start_ms || ts > end_ms { continue; }

                let side_str = entry["side"].as_str().unwrap_or("deposit");
                let tx_type = if side_str == "withdraw" { "transfer_out" } else { "transfer_in" };
                let coin = entry["coin"].as_str().unwrap_or("").to_string();
                let id = entry["transactionRecordId"].as_i64()
                    .map(|n| n.to_string())
                    .or_else(|| entry["transactionRecordId"].as_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                let info = format!(
                    "network={} status={} txId={}",
                    entry["network"].as_str().unwrap_or(""),
                    entry["status"].as_str().unwrap_or(""),
                    entry["txId"].as_str().unwrap_or(""),
                );
                logs.push(TransactionLog {
                    id:                 id.clone(),
                    timestamp:          ts,
                    instrument_name:    coin.clone(),
                    transaction_type:   tx_type.to_string(),
                    category:           String::new(),
                    side:               side_str.to_string(),
                    amount:             entry["amount"].as_f64().unwrap_or(0.0),
                    price:              0.0,
                    fee:                entry["serviceFee"].as_f64().unwrap_or(0.0),
                    fee_currency:       coin.clone(),
                    currency:           coin.clone(),
                    profit_as_cashflow: 0.0,
                    balance:            0.0,
                    change:             0.0,
                    trade_id:           String::new(),
                    order_id:           String::new(),
                    info,
                    mark_price:         0.0,
                    index_price:        0.0,
                    equity:             0.0,
                    position:           0.0,
                    base_currency:      coin,
                    quote_currency:     String::new(),
                    funding:            0.0,
                });
            }

            // Check if there are more pages
            let total: u32 = resp["data"]["total"].as_u64().unwrap_or(0) as u32;
            let fetched_so_far = (page * 50) as u32;
            if list.len() < 50 || fetched_so_far >= total { break; }
            page += 1;
        }
    }

    // ── 3. System transfer records (internal credits, transfers, bonuses) ────
    {
        let mut page = 1u32;
        loop {
            let params: Vec<(&str, String)> = vec![
                ("page",      page.to_string()),
                ("pageSize",  "50".to_string()),
                ("startTime", start_ms.to_string()),
                ("endTime",   end_ms.to_string()),
            ];
            let (hdr, qs) = auth_get(&account.api_key, &account.api_secret, "/open/account/sysTransferRecords/v1", &params);
            let resp: Value = match client.get(&format!("{}/open/account/sysTransferRecords/v1?{}", base_url, qs))
                .headers(hdr).send().await {
                    Ok(r) => r.json().await.unwrap_or(Value::Null),
                    Err(_) => break,
                };

            if resp["code"].as_i64() != Some(0) { break; }

            let list = match resp["data"]["list"].as_array() {
                Some(l) => l.clone(),
                None    => break,
            };
            if list.is_empty() { break; }

            for entry in &list {
                let ts = entry["time"].as_i64().unwrap_or(0);
                if ts < start_ms || ts > end_ms { continue; }

                // type: 0=CREDIT, 1=REWARDS, 2=transfer, 3=Trail Bonus, 4=Release Trail Bonus
                let raw_type = entry["type"].as_i64().unwrap_or(2);
                let tx_type = match raw_type {
                    0 => "credit",
                    1 => "rewards",
                    3 | 4 => "bonus",
                    _ => {
                        // side: 0=increase (transfer_in), 1=decrease (transfer_out)
                        if entry["side"].as_i64().unwrap_or(0) == 0 { "transfer_in" } else { "transfer_out" }
                    }
                };
                let side_str = if entry["side"].as_i64().unwrap_or(0) == 0 { "in" } else { "out" };
                let coin = entry["coin"].as_str().unwrap_or("").to_string();
                let tx_id = entry["txId"].as_str().map(|s| s.to_string())
                    .or_else(|| entry["txId"].as_i64().map(|n| n.to_string()))
                    .unwrap_or_default();
                let note = entry["note"].as_str().unwrap_or("").to_string();

                logs.push(TransactionLog {
                    id:                 tx_id.clone(),
                    timestamp:          ts,
                    instrument_name:    coin.clone(),
                    transaction_type:   tx_type.to_string(),
                    category:           String::new(),
                    side:               side_str.to_string(),
                    amount:             entry["creditChange"].as_f64().unwrap_or(0.0),
                    price:              0.0,
                    fee:                0.0,
                    fee_currency:       coin.clone(),
                    currency:           coin.clone(),
                    profit_as_cashflow: 0.0,
                    balance:            0.0,
                    change:             0.0,
                    trade_id:           String::new(),
                    order_id:           String::new(),
                    info:               note,
                    mark_price:         0.0,
                    index_price:        0.0,
                    equity:             0.0,
                    position:           0.0,
                    base_currency:      coin,
                    quote_currency:     String::new(),
                    funding:            0.0,
                });
            }

            let total: u32 = resp["data"]["total"].as_u64().unwrap_or(0) as u32;
            let fetched_so_far = (page * 50) as u32;
            if list.len() < 50 || fetched_so_far >= total { break; }
            page += 1;
        }
    }

    // ── 4. Futures funding rate records (/open/settle/future/record/v1) ────────
    // Response: { list: [{ id, symbol, tradeSide, qty, fundFee, fundRate, ctime }] }
    {
        let uri = "/open/settle/future/record/v1";
        let mut page = 1u32;
        loop {
            let params: Vec<(&str, String)> = vec![
                ("page",      page.to_string()),
                ("pageSize",  "50".to_string()),
                ("startTime", start_ms.to_string()),
                ("endTime",   end_ms.to_string()),
            ];
            let (hdr, qs) = auth_get(&account.api_key, &account.api_secret, uri, &params);
            let resp: Value = match client.get(&format!("{}{uri}?{qs}", base_url))
                .headers(hdr).send().await {
                    Ok(r) => r.json().await.unwrap_or(Value::Null),
                    Err(_) => break,
                };
            if resp["code"].as_i64() != Some(0) { break; }
            let list = match resp["data"]["list"].as_array() {
                Some(l) if !l.is_empty() => l.clone(),
                _ => break,
            };
            for e in &list {
                let ts = e["ctime"].as_i64().unwrap_or(0);
                if ts < start_ms || ts > end_ms { continue; }
                let id = e["id"].as_i64().map(|n| n.to_string()).unwrap_or_default();
                let symbol = e["symbol"].as_str().unwrap_or("").to_string();
                let fund_fee = e["fundFee"].as_f64().unwrap_or(0.0);
                let fund_rate = e["fundRate"].as_f64().unwrap_or(0.0);
                let side = if e["tradeSide"].as_i64().unwrap_or(1) == 1 { "buy" } else { "sell" };
                let (base_currency, quote_currency) = parse_base_quote(&symbol);
                logs.push(TransactionLog {
                    id,
                    timestamp:          ts,
                    instrument_name:    symbol,
                    transaction_type:   "funding".to_string(),
                    category:           String::new(),
                    side:               side.to_string(),
                    amount:             e["qty"].as_f64().unwrap_or(0.0),
                    price:              0.0,
                    fee:                0.0,
                    fee_currency:       "USD".to_string(),
                    currency:           "USD".to_string(),
                    profit_as_cashflow: fund_fee,  // funding fee paid/received
                    balance:            0.0,
                    change:             fund_fee,
                    trade_id:           String::new(),
                    order_id:           String::new(),
                    info:               format!("fundRate={}", fund_rate),
                    mark_price:         0.0,
                    index_price:        0.0,
                    equity:             0.0,
                    position:           0.0,
                    base_currency,
                    quote_currency,
                    funding:            0.0,
                });
            }
            let total: u32 = resp["data"]["total"].as_u64().unwrap_or(0) as u32;
            if list.len() < 50 || (page * 50) >= total { break; }
            page += 1;
        }
    }

    // ── 5. Futures delivery settlement(/open/futures/delivery/settlement/history/v1)
    // Response: { list: [{ symbol, time, qty, tradeSide, entryPrice,
    //                      settlementPrice, settlementPnL, netCashFlow, fees }] }
    {
        let uri = "/open/futures/delivery/settlement/history/v1";
        let mut page = 1u32;
        loop {
            let params: Vec<(&str, String)> = vec![
                ("page",      page.to_string()),
                ("pageSize",  "50".to_string()),
                ("startTime", start_ms.to_string()),
                ("endTime",   end_ms.to_string()),
            ];
            let (hdr, qs) = auth_get(&account.api_key, &account.api_secret, uri, &params);
            let resp: Value = match client.get(&format!("{}{uri}?{qs}", base_url))
                .headers(hdr).send().await {
                    Ok(r) => r.json().await.unwrap_or(Value::Null),
                    Err(_) => break,
                };
            if resp["code"].as_i64() != Some(0) { break; }
            let list = match resp["data"]["list"].as_array() {
                Some(l) if !l.is_empty() => l.clone(),
                _ => break,
            };
            for e in &list {
                let ts = e["time"].as_i64().unwrap_or(0);
                if ts < start_ms || ts > end_ms { continue; }
                let symbol = e["symbol"].as_str().unwrap_or("").to_string();
                // netCashFlow = actual cash P&L after fees; fall back to settlementPnL
                let net_cash = e["netCashFlow"].as_f64()
                    .or_else(|| e["settlementPnL"].as_f64())
                    .unwrap_or(0.0);
                let fee  = e["fees"].as_f64().or_else(|| e["fee"].as_f64()).unwrap_or(0.0);
                let side = if e["tradeSide"].as_i64().unwrap_or(1) == 1 { "buy" } else { "sell" };
                let qty   = e["qty"].as_f64().unwrap_or(0.0);
                let price = e["settlementPrice"].as_f64().unwrap_or(0.0);
                let (base_currency, quote_currency) = parse_base_quote(&symbol);
                logs.push(TransactionLog {
                    id:                 format!("settle-{}-{}", symbol, ts),
                    timestamp:          ts,
                    instrument_name:    symbol,
                    transaction_type:   "delivery".to_string(),
                    category:           String::new(),
                    side:               side.to_string(),
                    amount:             qty,
                    price,
                    fee,
                    fee_currency:       "USD".to_string(),
                    currency:           "USD".to_string(),
                    profit_as_cashflow: net_cash,
                    balance:            0.0,
                    change:             net_cash,
                    trade_id:           String::new(),
                    order_id:           String::new(),
                    info:               format!("entryPrice={}", e["entryPrice"].as_f64().unwrap_or(0.0)),
                    mark_price:         0.0,
                    index_price:        0.0,
                    equity:             0.0,
                    position:           price * qty,
                    base_currency,
                    quote_currency,
                    funding:            0.0,
                });
            }
            let total: u32 = resp["data"]["total"].as_u64().unwrap_or(0) as u32;
            if list.len() < 50 || (page * 50) >= total { break; }
            page += 1;
        }
    }

    // ── 6. Options exercise records (/open/settle/exercise/history/v1) ─────────
    // Response: { id, symbol/instrumentName/optionName, exerciseTime/ctime/time,
    //             tradeSide, qty, exercisePrice, netCashFlow, fees/fee }
    {
        let uri = "/open/settle/exercise/history/v1";
        let mut page = 1u32;
        loop {
            let params: Vec<(&str, String)> = vec![
                ("page",      page.to_string()),
                ("pageSize",  "50".to_string()),
                ("startTime", start_ms.to_string()),
                ("endTime",   end_ms.to_string()),
            ];
            let (hdr, qs) = auth_get(&account.api_key, &account.api_secret, uri, &params);
            let resp: Value = match client.get(&format!("{}{uri}?{qs}", base_url))
                .headers(hdr).send().await {
                    Ok(r) => r.json().await.unwrap_or(Value::Null),
                    Err(_) => break,
                };
            if resp["code"].as_i64() != Some(0) { break; }
            let list = match resp["data"]["list"].as_array() {
                Some(l) if !l.is_empty() => l.clone(),
                _ => break,
            };
            for e in &list {
                eprintln!("Debug: raw exercise record: {}", e);
                let ts = e["exerciseTime"].as_i64()
                    .or_else(|| e["ctime"].as_i64())
                    .or_else(|| e["time"].as_i64())
                    .unwrap_or(0);
                if ts < start_ms || ts > end_ms { continue; }

                let symbol = e["symbol"].as_str()
                    .or_else(|| e["instrumentName"].as_str())
                    .or_else(|| e["optionName"].as_str())
                    .unwrap_or("").to_string();
                // netCashFlow is the actual realised P&L cash amount after fees
                let net_cash = e["netCashFlow"].as_f64()
                    .or_else(|| e["pnl"].as_f64())
                    .or_else(|| e["profit"].as_f64())
                    .or_else(|| e["settlementPnL"].as_f64())
                    .unwrap_or(0.0);
                let fee = e["fees"].as_f64().or_else(|| e["fee"].as_f64()).unwrap_or(0.0);
                // qty → amount
                let amount = e["qty"].as_f64().or_else(|| e["amount"].as_f64()).unwrap_or(0.0);
                // exercisePrice is the canonical price field
                let price = e["exercisePrice"].as_f64()
                    .or_else(|| e["settlementPrice"].as_f64())
                    .unwrap_or(0.0);
                let id = e["id"].as_i64().map(|n| n.to_string())
                    .or_else(|| e["exerciseId"].as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| format!("ex-{}-{}", symbol, ts));
                let side = match e["tradeSide"].as_i64().unwrap_or(0) {
                    1 => "buy", 2 => "sell", _ => "",
                };
                let (base_currency, quote_currency) = parse_base_quote(&symbol);
                logs.push(TransactionLog {
                    id,
                    timestamp:          ts,
                    instrument_name:    symbol,
                    transaction_type:   "delivery".to_string(),
                    category:           String::new(),
                    side:               side.to_string(),
                    amount,
                    price,
                    fee,
                    fee_currency:       "USD".to_string(),
                    currency:           "USD".to_string(),
                    profit_as_cashflow: net_cash,
                    balance:            0.0,
                    change:             net_cash,
                    trade_id:           String::new(),
                    order_id:           String::new(),
                    info:               String::new(),
                    mark_price:         0.0,
                    index_price:        0.0,
                    equity:             0.0,
                    position:           price * amount,
                    base_currency,
                    quote_currency,
                    funding:            0.0,
                });
            }
            let total: u32 = resp["data"]["total"].as_u64().unwrap_or(0) as u32;
            if list.len() < 50 || (page * 50) >= total { break; }
            page += 1;
        }
    }

    logs.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    // ── Backward balance + equity reconstruction ─────────────────────────────
    // equity  = top-level `equity`           from /open/account/summary/v1
    // balance = top-level `totalDollarValue` from /open/account/summary/v1
    // Walk newest→oldest undoing each entry's `change` to reconstruct both.
    if let Ok((seed_equity, seed_balance)) = fetch_account_totals(account).await {
        let mut running_equity  = seed_equity;
        let mut running_balance = seed_balance;
        for log in logs.iter_mut() {
            log.equity  = running_equity;
            log.balance = running_balance;
            let delta = if log.change != 0.0 { log.change } else { -log.fee };
            running_equity  -= delta;
            running_balance -= delta;
        }
    }

    Ok(logs)
}

/// Fetch top-level account totals: (equity, totalDollarValue).
/// `equity`          = account-level net equity in USD
/// `totalDollarValue`= total wallet value in USD (used as "balance" seed)
async fn fetch_account_totals(account: &Account) -> Result<(f64, f64), String> {
    let base_url = base(account.testnet);
    let uri = "/open/account/summary/v1";
    let (headers, _) = auth_get(&account.api_key, &account.api_secret, uri, &[]);

    let resp: Value = Client::new()
        .get(&format!("{}{}", base_url, uri))
        .headers(headers)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["code"].as_i64() != Some(0) {
        return Err(resp["msg"].as_str().unwrap_or("CoInCall error").to_string());
    }

    let d = &resp["data"];
    let parse = |key: &str| -> f64 {
        d[key].as_f64()
            .or_else(|| d[key].as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(0.0)
    };

    Ok((parse("equity"), parse("totalDollarValue")))
}
