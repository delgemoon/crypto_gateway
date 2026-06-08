use reqwest::{Client, header::{HeaderMap, HeaderValue, CONTENT_TYPE}};
use serde_json::{json, Value};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use base64::{Engine as _, engine::general_purpose};
use chrono::Utc;

use crate::api::models::{
    Account, AccountSummary, Instrument, Order, OrderResult, OrderbookLevel, OrderbookSnapshot,
    PlaceOrderRequest, Position, Ticker, TickerStats, Trade,
};

type HmacSha256 = Hmac<Sha256>;

const OKX_BASE: &str = "https://www.okx.com";

fn kind_to_inst_type(kind: &str) -> &'static str {
    match kind {
        "option" => "OPTION",
        "spot"   => "SPOT",
        _        => "SWAP", // futures/perpetuals
    }
}

fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

fn sign(timestamp: &str, method: &str, path_with_query: &str, body: &str, secret: &str) -> String {
    let msg = format!("{}{}{}{}", timestamp, method, path_with_query, body);
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(msg.as_bytes());
    general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

fn auth_headers(
    api_key: &str,
    secret: &str,
    passphrase: &str,
    testnet: bool,
    method: &str,
    path_with_query: &str,
    body: &str,
) -> HeaderMap {
    let ts = now_iso();
    let sig = sign(&ts, method, path_with_query, body, secret);
    let mut h = HeaderMap::new();
    h.insert("OK-ACCESS-KEY",        HeaderValue::from_str(api_key).unwrap_or_else(|_| HeaderValue::from_static("")));
    h.insert("OK-ACCESS-SIGN",       HeaderValue::from_str(&sig).unwrap_or_else(|_| HeaderValue::from_static("")));
    h.insert("OK-ACCESS-TIMESTAMP",  HeaderValue::from_str(&ts).unwrap_or_else(|_| HeaderValue::from_static("")));
    h.insert("OK-ACCESS-PASSPHRASE", HeaderValue::from_str(passphrase).unwrap_or_else(|_| HeaderValue::from_static("")));
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if testnet {
        h.insert("x-simulated-trading", HeaderValue::from_static("1"));
    }
    h
}

// ── Public ─────────────────────────────────────────────────────────────────

pub async fn fetch_instruments(currency: &str, kind: &str) -> Result<Vec<Instrument>, String> {
    let inst_type = kind_to_inst_type(kind);
    let path = match inst_type {
        "SPOT" => format!("/api/v5/public/instruments?instType=SPOT&baseCcy={}", currency),
        _      => format!("/api/v5/public/instruments?instType={}&instFamily={}-USD", inst_type, currency),
    };
    let url = format!("{}{}", OKX_BASE, path);

    let resp: Value = Client::new()
        .get(&url)
        .header(CONTENT_TYPE, "application/json")
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["code"].as_str() != Some("0") {
        return Err(resp["msg"].as_str().unwrap_or("OKX error").to_string());
    }

    let instruments = resp["data"].as_array().ok_or("no data")?
        .iter()
        .filter(|i| i["state"].as_str() == Some("live"))
        .map(|i| Instrument {
            instrument_name:     i["instId"].as_str().unwrap_or("").to_string(),
            kind:                kind.to_string(),
            base_currency:       i["ctValCcy"].as_str().or_else(|| i["baseCcy"].as_str()).unwrap_or(currency).to_string(),
            quote_currency:      i["quoteCcy"].as_str().unwrap_or("USD").to_string(),
            settlement_currency: i["settleCcy"].as_str().unwrap_or("USD").to_string(),
            is_active:           true,
            tick_size:           parse_str_f64(&i["tickSz"], 0.01),
            min_trade_amount:    parse_str_f64(&i["minSz"], 1.0),
            contract_size:       i["ctVal"].as_str().and_then(|s| s.parse().ok()),
            option_type:         non_empty_str(&i["optType"]),
            strike:              i["stk"].as_str().and_then(|s| s.parse().ok()),
            expiration_timestamp: i["expTime"].as_str().and_then(|s| s.parse().ok()),
        })
        .collect();

    Ok(instruments)
}

pub async fn fetch_ticker(instrument_name: &str) -> Result<Ticker, String> {
    let url = format!("{}/api/v5/market/ticker?instId={}", OKX_BASE, instrument_name);

    let resp: Value = Client::new()
        .get(&url)
        .header(CONTENT_TYPE, "application/json")
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["code"].as_str() != Some("0") {
        return Err(resp["msg"].as_str().unwrap_or("OKX error").to_string());
    }

    let d = &resp["data"][0];
    let last  = d["last"].as_str().and_then(|s| s.parse::<f64>().ok());
    let open  = d["open24h"].as_str().and_then(|s| s.parse::<f64>().ok());
    let pct   = match (last, open) {
        (Some(l), Some(o)) if o != 0.0 => Some((l - o) / o * 100.0),
        _ => None,
    };

    Ok(Ticker {
        instrument_name:  instrument_name.to_string(),
        best_bid_price:   str_to_f64(&d["bidPx"]),
        best_ask_price:   str_to_f64(&d["askPx"]),
        best_bid_amount:  str_to_f64(&d["bidSz"]),
        best_ask_amount:  str_to_f64(&d["askSz"]),
        last_price:       last,
        mark_price:       last,
        index_price:      None,
        open_interest:    str_to_f64(&d["openInterest"]),
        stats: TickerStats {
            high:         str_to_f64(&d["high24h"]),
            low:          str_to_f64(&d["low24h"]),
            price_change: pct,
            volume:       str_to_f64(&d["vol24h"]),
            volume_usd:   str_to_f64(&d["volCcy24h"]),
        },
        mark_iv: None, bid_iv: None, ask_iv: None,
        delta: None, gamma: None, vega: None, theta: None,
    })
}

// ── Authenticated ──────────────────────────────────────────────────────────

pub async fn place_order(req: &PlaceOrderRequest, account: &Account) -> Result<OrderResult, String> {
    let path = "/api/v5/trade/order";
    let passphrase = account.passphrase.as_deref().unwrap_or("");

    let ord_type = if req.post_only.unwrap_or(false) {
        "post_only"
    } else {
        match req.order_type.as_str() { "market" => "market", _ => "limit" }
    };

    let mut body = json!({
        "instId": req.instrument_name,
        "tdMode": "cross",
        "side": req.side,
        "ordType": ord_type,
        "sz": req.amount.to_string(),
    });
    if let Some(px) = req.price { body["px"] = json!(px.to_string()); }
    // OKX clOrdId: max 32 chars (alphanumeric + _ -)
    if let Some(ref clord_id) = req.client_order_id {
        let cid = if clord_id.len() > 32 { &clord_id[..32] } else { clord_id.as_str() };
        body["clOrdId"] = json!(cid);
    }

    let body_str = serde_json::to_string(&body).unwrap();
    let headers  = auth_headers(&account.api_key, &account.api_secret, passphrase, account.testnet, "POST", path, &body_str);

    let resp: Value = Client::new().post(&format!("{}{}", OKX_BASE, path))
        .headers(headers).body(body_str)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["code"].as_str() != Some("0") {
        return Ok(OrderResult { success: false, order: None, error: Some(resp["msg"].as_str().unwrap_or("OKX order failed").to_string()) });
    }

    let ord_id = resp["data"][0]["ordId"].as_str().unwrap_or("").to_string();
    Ok(OrderResult {
        success: true,
        order: Some(Order {
            order_id: ord_id,
            instrument_name: req.instrument_name.clone(),
            direction: req.side.clone(),
            order_type: req.order_type.clone(),
            order_state: "open".to_string(),
            price: req.price,
            amount: req.amount,
            filled_amount: 0.0,
            average_price: None,
            post_only: req.post_only.unwrap_or(false),
            time_in_force: req.time_in_force.clone().unwrap_or_else(|| "good_til_cancelled".to_string()),
            creation_timestamp: 0,
            last_update_timestamp: 0,
        }),
        error: None,
    })
}

pub async fn cancel_order(order_id: &str, instrument_name: Option<&str>, account: &Account) -> Result<bool, String> {
    let path = "/api/v5/trade/cancel-order";
    let passphrase = account.passphrase.as_deref().unwrap_or("");

    let mut body = json!({ "ordId": order_id });
    if let Some(inst) = instrument_name { body["instId"] = json!(inst); }

    let body_str = serde_json::to_string(&body).unwrap();
    let headers  = auth_headers(&account.api_key, &account.api_secret, passphrase, account.testnet, "POST", path, &body_str);

    let resp: Value = Client::new().post(&format!("{}{}", OKX_BASE, path))
        .headers(headers).body(body_str)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    eprintln!("[okx] cancel_order response: {}", resp);
    if resp["code"].as_str() != Some("0") {
        let msg = resp["msg"].as_str().unwrap_or("Unknown OKX error");
        let code = resp["code"].as_str().unwrap_or("?");
        return Err(format!("OKX cancel error ({}): {}", code, msg));
    }
    Ok(true)
}

pub async fn get_open_orders(instrument_name: &str, account: &Account) -> Result<Vec<Order>, String> {
    let path_q = format!("/api/v5/trade/orders-pending?instId={}", instrument_name);
    let passphrase = account.passphrase.as_deref().unwrap_or("");
    let headers = auth_headers(&account.api_key, &account.api_secret, passphrase, account.testnet, "GET", &path_q, "");

    let resp: Value = Client::new().get(&format!("{}{}", OKX_BASE, path_q))
        .headers(headers)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["code"].as_str() != Some("0") {
        return Err(resp["msg"].as_str().unwrap_or("OKX error").to_string());
    }

    let orders = resp["data"].as_array().map(|arr| arr.iter().map(|o| Order {
        order_id:            o["ordId"].as_str().unwrap_or("").to_string(),
        instrument_name:     o["instId"].as_str().unwrap_or("").to_string(),
        direction:           o["side"].as_str().unwrap_or("").to_string(),
        order_type:          o["ordType"].as_str().unwrap_or("").to_string(),
        order_state:         o["state"].as_str().unwrap_or("").to_string(),
        price:               str_to_f64(&o["px"]),
        amount:              parse_str_f64(&o["sz"], 0.0),
        filled_amount:       parse_str_f64(&o["accFillSz"], 0.0),
        average_price:       str_to_f64(&o["avgPx"]),
        post_only:           o["ordType"].as_str() == Some("post_only"),
        time_in_force:       tif_from_okx(o["timeInForce"].as_str().unwrap_or("GTC")),
        creation_timestamp:  o["cTime"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
        last_update_timestamp: o["uTime"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
    }).collect()).unwrap_or_default();

    Ok(orders)
}

/// Get ALL open orders for this account across OPTION, SWAP, FUTURES instrument types.
pub async fn get_all_open_orders(account: &Account) -> Result<Vec<Order>, String> {
    let passphrase = account.passphrase.as_deref().unwrap_or("");
    let mut all = Vec::new();
    for inst_type in &["OPTION", "SWAP", "FUTURES"] {
        let path_q = format!("/api/v5/trade/orders-pending?instType={}", inst_type);
        let headers = auth_headers(&account.api_key, &account.api_secret, passphrase, account.testnet, "GET", &path_q, "");
        let resp: Value = match Client::new().get(&format!("{}{}", OKX_BASE, path_q)).headers(headers).send().await {
            Ok(r) => r.json().await.unwrap_or(Value::Null),
            Err(_) => continue,
        };
        if resp["code"].as_str() != Some("0") { continue; }
        if let Some(arr) = resp["data"].as_array() {
            all.extend(arr.iter().map(|o| Order {
                order_id:             o["ordId"].as_str().unwrap_or("").to_string(),
                instrument_name:      o["instId"].as_str().unwrap_or("").to_string(),
                direction:            o["side"].as_str().unwrap_or("").to_string(),
                order_type:           o["ordType"].as_str().unwrap_or("").to_string(),
                order_state:          o["state"].as_str().unwrap_or("").to_string(),
                price:                str_to_f64(&o["px"]),
                amount:               parse_str_f64(&o["sz"], 0.0),
                filled_amount:        parse_str_f64(&o["accFillSz"], 0.0),
                average_price:        str_to_f64(&o["avgPx"]),
                post_only:            o["ordType"].as_str() == Some("post_only"),
                time_in_force:        tif_from_okx(o["timeInForce"].as_str().unwrap_or("GTC")),
                creation_timestamp:   o["cTime"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
                last_update_timestamp: o["uTime"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
            }));
        }
    }
    Ok(all)
}

/// Get trade fills history for this account.
pub async fn get_trade_history(account: &Account, start_ms: i64, end_ms: i64) -> Result<Vec<Trade>, String> {
    let passphrase = account.passphrase.as_deref().unwrap_or("");
    let mut all = Vec::new();
    for inst_type in &["OPTION", "SWAP", "FUTURES"] {
        let mut path_q = format!("/api/v5/trade/fills-history?instType={}&limit=100", inst_type);
        if start_ms > 0 { path_q.push_str(&format!("&begin={}", start_ms)); }
        if end_ms   > 0 { path_q.push_str(&format!("&end={}", end_ms)); }
        let headers = auth_headers(&account.api_key, &account.api_secret, passphrase, account.testnet, "GET", &path_q, "");
        let resp: Value = match Client::new().get(&format!("{}{}", OKX_BASE, path_q)).headers(headers).send().await {
            Ok(r) => r.json().await.unwrap_or(Value::Null),
            Err(_) => continue,
        };
        if resp["code"].as_str() != Some("0") { continue; }
        if let Some(arr) = resp["data"].as_array() {
            for t in arr {
                let fee_str = t["fee"].as_str().unwrap_or("0");
                let fee: f64 = fee_str.parse::<f64>().unwrap_or(0.0).abs();
                all.push(Trade {
                    trade_id:        t["tradeId"].as_str().unwrap_or("").to_string(),
                    account_id:      String::new(),
                    account_name:    String::new(),
                    exchange:        "okx".to_string(),
                    instrument_name: t["instId"].as_str().unwrap_or("").to_string(),
                    direction:       t["side"].as_str().unwrap_or("").to_string(),
                    amount:          t["fillSz"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    price:           t["fillPx"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    fee,
                    fee_currency:    t["feeCcy"].as_str().unwrap_or("").to_string(),
                    timestamp:       t["ts"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
                    order_id:        t["ordId"].as_str().unwrap_or("").to_string(),
                });
            }
        }
    }
    Ok(all)
}

pub async fn get_account_summary(currency: &str, account: &Account) -> Result<AccountSummary, String> {
    let path_q = format!("/api/v5/account/balance?ccy={}", currency);
    let passphrase = account.passphrase.as_deref().unwrap_or("");
    let headers = auth_headers(&account.api_key, &account.api_secret, passphrase, account.testnet, "GET", &path_q, "");

    let resp: Value = Client::new().get(&format!("{}{}", OKX_BASE, path_q))
        .headers(headers)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["code"].as_str() != Some("0") {
        return Err(resp["msg"].as_str().unwrap_or("OKX error").to_string());
    }

    let det = resp["data"][0]["details"].as_array()
        .and_then(|arr| arr.iter().find(|d| d["ccy"].as_str() == Some(currency)));

    Ok(AccountSummary {
        currency:          currency.to_string(),
        equity:            det.and_then(|d| str_to_f64(&d["eq"])).unwrap_or(0.0),
        available_funds:   det.and_then(|d| str_to_f64(&d["availBal"])).unwrap_or(0.0),
        initial_margin:    det.and_then(|d| str_to_f64(&d["frozenBal"])).unwrap_or(0.0),
        maintenance_margin: 0.0,
        unrealized_pl:     det.and_then(|d| str_to_f64(&d["upl"])).unwrap_or(0.0),
    })
}

/// Get open positions across OPTION, SWAP, and FUTURES.
pub async fn get_positions(currency: &str, account: &Account) -> Result<Vec<Position>, String> {
    let passphrase = account.passphrase.as_deref().unwrap_or("");
    let mut all: Vec<Position> = Vec::new();
    for inst_type in &["OPTION", "SWAP", "FUTURES"] {
        let path_q = format!("/api/v5/account/positions?instType={}&ccy={}", inst_type, currency);
        let headers = auth_headers(&account.api_key, &account.api_secret, passphrase, account.testnet, "GET", &path_q, "");
        let resp: Value = match Client::new()
            .get(&format!("{}{}", OKX_BASE, path_q))
            .headers(headers).send().await
        {
            Ok(r) => r.json().await.unwrap_or(Value::Null),
            Err(_) => continue,
        };
        if resp["code"].as_str() != Some("0") { continue; }
        if let Some(data) = resp["data"].as_array() {
            all.extend(data.iter().filter_map(|p| {
                let size: f64 = str_to_f64(&p["pos"]).unwrap_or(0.0);
                if size == 0.0 { return None; }
                let side = p["posSide"].as_str().unwrap_or(if size > 0.0 { "long" } else { "short" });
                Some(Position {
                    instrument_name: p["instId"].as_str().unwrap_or("").to_string(),
                    direction:       side.to_lowercase(),
                    size:            size.abs(),
                    average_price:   str_to_f64(&p["avgPx"]).unwrap_or(0.0),
                    mark_price:      str_to_f64(&p["markPx"]).unwrap_or(0.0),
                    mark_iv:         0.0,
                    unrealized_pnl:  str_to_f64(&p["upl"]).unwrap_or(0.0),
                    delta:           str_to_f64(&p["delta"]).unwrap_or(0.0),
                    gamma:           str_to_f64(&p["gamma"]).unwrap_or(0.0),
                    theta:           str_to_f64(&p["theta"]).unwrap_or(0.0),
                    vega:            str_to_f64(&p["vega"]).unwrap_or(0.0),
                })
            }));
        }
    }
    Ok(all)
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn str_to_f64(v: &Value) -> Option<f64> {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_f64())
}

fn parse_str_f64(v: &Value, default: f64) -> f64 {
    str_to_f64(v).unwrap_or(default)
}

fn non_empty_str(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(|s| s.to_string())
}

fn tif_from_okx(s: &str) -> String {
    match s {
        "IOC" => "immediate_or_cancel".to_string(),
        "FOK" => "fill_or_kill".to_string(),
        _     => "good_til_cancelled".to_string(),
    }
}

pub async fn fetch_orderbook(instrument_name: &str, depth: u32) -> Result<OrderbookSnapshot, String> {
    let client = Client::new();
    let url = format!(
        "{}/api/v5/market/books?instId={}&sz={}",
        OKX_BASE, instrument_name, depth
    );

    let resp: Value = client.get(&url).send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    if resp["code"].as_str() == Some("0") {
        let data = &resp["data"][0];
        let parse_levels = |arr: &Value| -> Vec<OrderbookLevel> {
            arr.as_array().map(|a| a.iter().filter_map(|item| {
                let arr = item.as_array()?;
                Some(OrderbookLevel {
                    price: arr.get(0)?.as_str()?.parse().ok()?,
                    size:  arr.get(1)?.as_str()?.parse().ok()?,
                })
            }).collect()).unwrap_or_default()
        };
        let ts_val = data["ts"].as_str().and_then(|s| s.parse::<i64>().ok()).unwrap_or(ts);
        Ok(OrderbookSnapshot {
            instrument_name: instrument_name.to_string(),
            bids: parse_levels(&data["bids"]),
            asks: parse_levels(&data["asks"]),
            timestamp: ts_val,
        })
    } else {
        Err(resp["msg"].as_str().unwrap_or("Unknown OKX error").to_string())
    }
}
