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

use crate::api::models::{
    Account, AccountSummary, Instrument, Order, OrderResult, OrderbookLevel, OrderbookSnapshot,
    PlaceOrderRequest, Position, Ticker, TickerStats, Trade,
};

type HmacSha256 = Hmac<Sha256>;

const CC_BASE: &str    = "https://api.coincall.com";
const CC_TEST: &str    = "https://beta.seizeyouralpha.com";
const TS_DIFF: u64     = 5000;

fn base(testnet: bool) -> &'static str {
    if testnet { CC_TEST } else { CC_BASE }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
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

// ── Ticker (public for price; orderbook for bid/ask) ───────────────────────

pub async fn fetch_ticker(instrument_name: &str, testnet: bool) -> Result<Ticker, String> {
    let detail_url = format!("{}/open/option/detail/v1/{}", base(testnet), instrument_name);
    let ob_url     = format!("{}/open/option/order/orderbook/v1/{}", base(testnet), instrument_name);

    // Run both requests concurrently
    let (detail_resp, ob_resp): (Value, Value) = tokio::try_join!(
        async { Client::new().get(&detail_url).header(CONTENT_TYPE, "application/json")
            .send().await.map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string()) },
        async { Client::new().get(&ob_url).header(CONTENT_TYPE, "application/json")
            .send().await.map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string()) },
    )?;

    if detail_resp["code"].as_i64() != Some(0) {
        return Err(detail_resp["msg"].as_str().unwrap_or("CoInCall ticker error").to_string());
    }

    let d  = &detail_resp["data"];
    let ob = &ob_resp["data"];

    let parse_ob_price = |arr: &Value| -> Option<f64> {
        arr.as_array()?.first()?.get("price")?.as_str()?.parse().ok()
    };
    let parse_ob_size = |arr: &Value| -> Option<f64> {
        arr.as_array()?.first()?.get("size")?.as_str()?.parse().ok()
    };

    Ok(Ticker {
        instrument_name:  instrument_name.to_string(),
        best_bid_price:   parse_ob_price(&ob["bids"]),
        best_ask_price:   parse_ob_price(&ob["asks"]),
        best_bid_amount:  parse_ob_size(&ob["bids"]),
        best_ask_amount:  parse_ob_size(&ob["asks"]),
        last_price:       d["lastPrice"].as_f64(),
        mark_price:       d["markPrice"].as_f64(),
        index_price:      d["underlyingPrice"].as_f64().or_else(|| d["indexPrice"].as_f64()),
        open_interest:    d["openInterest"].as_f64(),
        stats: TickerStats {
            high:         d["price24hHigh"].as_f64(),
            low:          d["price24hLow"].as_f64(),
            price_change: d["changeRate"].as_f64(),
            volume:       d["volume24h"].as_f64(),
            volume_usd:   d["volumeUsd24h"].as_f64(),
        },
        mark_iv:  d["iv"].as_f64(),
        bid_iv:   None,
        ask_iv:   None,
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

/// Get open positions.
pub async fn get_positions(_currency: &str, account: &Account) -> Result<Vec<Position>, String> {
    let base_url = base(account.testnet);
    let uri = "/open/option/position/list/v1";
    let (headers, _) = auth_get(&account.api_key, &account.api_secret, uri, &[]);

    let resp: Value = Client::new()
        .get(&format!("{}{}", base_url, uri))
        .headers(headers)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["code"].as_i64() != Some(0) { return Ok(vec![]); }

    let positions = resp["data"].as_array()
        .map(|arr| arr.iter().filter_map(|p| {
            let parse = |v: &Value| v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_f64()).unwrap_or(0.0);
            let size = parse(&p["qty"]);
            if size == 0.0 { return None; }
            let side = p["side"].as_i64().unwrap_or(1); // 1=buy/long, -1=sell/short
            Some(Position {
                instrument_name: p["symbol"].as_str().unwrap_or("").to_string(),
                direction:       if side >= 0 { "long".to_string() } else { "short".to_string() },
                size,
                average_price:   parse(&p["openAvgPrice"]),
                mark_price:      parse(&p["markPrice"]),
                mark_iv:         parse(&p["markIv"]),
                unrealized_pnl:  parse(&p["floatingPL"]),
                delta:           parse(&p["delta"]),
                gamma:           parse(&p["gamma"]),
                theta:           parse(&p["theta"]),
                vega:            parse(&p["vega"]),
            })
        }).collect())
        .unwrap_or_default();
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
pub async fn get_rfq_list(account: &Account) -> Result<Value, String> {
    let base_url = base(account.testnet);
    let uri = "/open/option/blocktrade/rfqList/v1";
    let (headers, _) = auth_get(&account.api_key, &account.api_secret, uri, &[]);

    let resp: Value = Client::new()
        .get(&format!("{}{}", base_url, uri))
        .headers(headers)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["code"].as_i64() != Some(0) {
        return Err(resp["msg"].as_str().unwrap_or("RFQ list error").to_string());
    }

    Ok(resp["data"]["rfqList"].clone())
}

/// Get quotes received for a specific RFQ (or all if request_id is empty).
pub async fn get_rfq_quotes(account: &Account, request_id: Option<&str>) -> Result<Value, String> {
    let base_url = base(account.testnet);
    let uri = "/open/option/blocktrade/request/getQuotesReceived/v1";

    let params: Vec<(&str, String)> = if let Some(rid) = request_id.filter(|s| !s.is_empty()) {
        vec![("requestId", rid.to_string())]
    } else {
        vec![]
    };

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

    if resp["code"].as_i64() != Some(0) {
        return Err(resp["msg"].as_str().unwrap_or("RFQ quotes error").to_string());
    }

    Ok(resp["data"].clone())
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
