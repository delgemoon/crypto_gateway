/// MEXC REST API (Spot + Futures)
///
/// Spot Base URL:    https://api.mexc.com
/// Futures Base URL: https://contract.mexc.com
/// Auth:             HMAC-SHA256 of query string, appended as `signature=`
/// Header:           X-MEXC-APIKEY: <api_key>
///
/// Instrument format: BTCUSDT (spot), BTC_USDT (futures)

use reqwest::{Client, header::{HeaderMap, HeaderValue, CONTENT_TYPE}};
use serde_json::{Value};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::models::{
    Account, AccountSummary, Instrument, Order, OrderResult, OrderbookLevel, OrderbookSnapshot,
    PlaceOrderRequest, Position, Ticker, TickerStats, Trade,
};

type HmacSha256 = Hmac<Sha256>;

const SPOT_BASE: &str    = "https://api.mexc.com";
const FUTURES_BASE: &str = "https://contract.mexc.com";

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn sign(msg: &str, secret: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(msg.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn api_key_header(api_key: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(
        "X-MEXC-APIKEY",
        HeaderValue::from_str(api_key).unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    h
}

fn signed_query(params: &str, secret: &str) -> String {
    let ts = now_ms();
    let base = if params.is_empty() {
        format!("timestamp={}", ts)
    } else {
        format!("{}&timestamp={}", params, ts)
    };
    let sig = sign(&base, secret);
    format!("{}&signature={}", base, sig)
}

// Determine if instrument is a futures contract (contains underscore like BTC_USDT)
fn is_futures(instrument: &str) -> bool {
    instrument.contains('_')
}

pub async fn fetch_orderbook(instrument_name: &str, depth: u32) -> Result<OrderbookSnapshot, String> {
    let client = Client::new();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let parse_str_levels = |arr: &Value| -> Vec<OrderbookLevel> {
        arr.as_array().map(|a| a.iter().filter_map(|item| {
            let arr = item.as_array()?;
            Some(OrderbookLevel {
                price: arr.get(0)?.as_str()?.parse().ok()?,
                size:  arr.get(1)?.as_str()?.parse().ok()?,
            })
        }).collect()).unwrap_or_default()
    };

    if is_futures(instrument_name) {
        // MEXC futures: GET /api/v1/contract/depth/{symbol}
        let url = format!("{}/api/v1/contract/depth/{}?limit={}", FUTURES_BASE, instrument_name, depth);
        let resp: Value = client.get(&url).send().await.map_err(|e| e.to_string())?
            .json().await.map_err(|e| e.to_string())?;
        if resp["success"].as_bool() == Some(true) {
            let data = &resp["data"];
            let parse_f_levels = |arr: &Value| -> Vec<OrderbookLevel> {
                arr.as_array().map(|a| a.iter().filter_map(|item| {
                    let arr = item.as_array()?;
                    Some(OrderbookLevel {
                        price: arr.get(0)?.as_f64()?,
                        size:  arr.get(1)?.as_f64()?,
                    })
                }).collect()).unwrap_or_default()
            };
            return Ok(OrderbookSnapshot {
                instrument_name: instrument_name.to_string(),
                bids: parse_f_levels(&data["bids"]),
                asks: parse_f_levels(&data["asks"]),
                timestamp: ts,
            });
        }
        return Err("MEXC futures depth error".to_string());
    }

    // Spot
    let url = format!("{}/api/v3/depth?symbol={}&limit={}", SPOT_BASE, instrument_name, depth);
    let resp: Value = client.get(&url).send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp.get("code").is_some() {
        return Err(resp["msg"].as_str().unwrap_or("MEXC error").to_string());
    }

    Ok(OrderbookSnapshot {
        instrument_name: instrument_name.to_string(),
        bids: parse_str_levels(&resp["bids"]),
        asks: parse_str_levels(&resp["asks"]),
        timestamp: ts,
    })
}

// ── Public Endpoints ───────────────────────────────────────────────────────

pub async fn fetch_instruments(currency: &str, kind: &str) -> Result<Vec<Instrument>, String> {
    let client = Client::new();
    let mut instruments = Vec::new();

    // Fetch spot instruments
    if kind.is_empty() || kind == "spot" {
        let url = format!("{}/api/v3/exchangeInfo", SPOT_BASE);
        let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
        let json: Value = resp.json().await.map_err(|e| e.to_string())?;

        if let Some(symbols) = json["symbols"].as_array() {
            for s in symbols {
                let base_asset = s["baseAsset"].as_str().unwrap_or("");
                let quote_asset = s["quoteAsset"].as_str().unwrap_or("");
                let status = s["status"].as_str().unwrap_or("");

                if !currency.is_empty() && !base_asset.to_uppercase().contains(&currency.to_uppercase()) {
                    continue;
                }
                if status != "1" && status != "ENABLED" { continue; }

                let symbol = s["symbol"].as_str().unwrap_or("").to_string();
                instruments.push(Instrument {
                    instrument_name:      symbol.clone(),
                    kind:                 "spot".to_string(),
                    base_currency:        base_asset.to_string(),
                    quote_currency:       quote_asset.to_string(),
                    settlement_currency:  quote_asset.to_string(),
                    is_active:            true,
                    tick_size:            s["quoteAssetPrecision"].as_i64()
                                            .map(|p| 10f64.powi(-(p as i32)))
                                            .unwrap_or(0.01),
                    min_trade_amount:     s["baseSizePrecision"].as_str()
                                            .and_then(|s| s.parse().ok())
                                            .unwrap_or(0.0001),
                    qty_step:             None,
                    contract_size:        None,
                    option_type:          None,
                    strike:               None,
                    expiration_timestamp: None,
                });
            }
        }
    }

    // Fetch perpetual futures
    if kind.is_empty() || kind == "future" || kind == "futures" || kind == "swap" {
        let url = format!("{}/api/v1/contract/detail", FUTURES_BASE);
        let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
        if let Ok(json) = resp.json::<Value>().await {
            if let Some(arr) = json["data"].as_array() {
                for s in arr {
                    let symbol = s["symbol"].as_str().unwrap_or("");
                    let base = symbol.split('_').next().unwrap_or("");
                    let quote = symbol.split('_').nth(1).unwrap_or("USDT");
                    let state = s["state"].as_i64().unwrap_or(0);

                    if !currency.is_empty() && !base.to_uppercase().contains(&currency.to_uppercase()) {
                        continue;
                    }
                    if state != 0 { continue; } // 0 = enabled

                    instruments.push(Instrument {
                        instrument_name:      symbol.to_string(),
                        kind:                 "future".to_string(),
                        base_currency:        base.to_string(),
                        quote_currency:       quote.to_string(),
                        settlement_currency:  quote.to_string(),
                        is_active:            true,
                        tick_size:            s["priceUnit"].as_f64().unwrap_or(0.01),
                        min_trade_amount:     s["minVol"].as_f64().unwrap_or(1.0),
                        qty_step:             None,
                        contract_size:        s["contractSize"].as_f64(),
                        option_type:          None,
                        strike:               None,
                        expiration_timestamp: None,
                    });
                }
            }
        }
    }

    Ok(instruments)
}

pub async fn fetch_ticker(instrument_name: &str) -> Result<Ticker, String> {
    let client = Client::new();

    if is_futures(instrument_name) {
        // Futures ticker
        let url = format!("{}/api/v1/contract/ticker?symbol={}", FUTURES_BASE, instrument_name);
        let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
        let json: Value = resp.json().await.map_err(|e| e.to_string())?;
        let d = &json["data"];

        Ok(Ticker {
            instrument_name: instrument_name.to_string(),
            best_bid_price:  d["bid1"].as_f64(),
            best_ask_price:  d["ask1"].as_f64(),
            best_bid_amount: None,
            best_ask_amount: None,
            last_price:      d["lastPrice"].as_f64(),
            mark_price:      d["fairPrice"].as_f64(),
            index_price:     d["indexPrice"].as_f64(),
            open_interest:   d["holdVol"].as_f64(),
            mark_iv:         None, bid_iv: None, ask_iv: None,
            delta: None, gamma: None, vega: None, theta: None,
            stats: TickerStats {
                high:         d["high24Price"].as_f64(),
                low:          d["low24Price"].as_f64(),
                price_change: d["riseFallRate"].as_f64(),
                volume:       d["volume24"].as_f64(),
                volume_usd:   None,
            },
        })
    } else {
        // Spot ticker
        let book_url  = format!("{}/api/v3/ticker/bookTicker?symbol={}", SPOT_BASE, instrument_name);
        let stats_url = format!("{}/api/v3/ticker/24hr?symbol={}", SPOT_BASE, instrument_name);

        let (book_resp, stats_resp) = tokio::join!(
            client.get(&book_url).send(),
            client.get(&stats_url).send(),
        );

        let book:  Value = book_resp.map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;
        let stats: Value = stats_resp.map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;

        Ok(Ticker {
            instrument_name: instrument_name.to_string(),
            best_bid_price:  book["bidPrice"].as_str().and_then(|s| s.parse().ok()),
            best_ask_price:  book["askPrice"].as_str().and_then(|s| s.parse().ok()),
            best_bid_amount: book["bidQty"].as_str().and_then(|s| s.parse().ok()),
            best_ask_amount: book["askQty"].as_str().and_then(|s| s.parse().ok()),
            last_price:      stats["lastPrice"].as_str().and_then(|s| s.parse().ok()),
            mark_price:      None,
            index_price:     None,
            open_interest:   None,
            mark_iv: None, bid_iv: None, ask_iv: None,
            delta: None, gamma: None, vega: None, theta: None,
            stats: TickerStats {
                high:         stats["highPrice"].as_str().and_then(|s| s.parse().ok()),
                low:          stats["lowPrice"].as_str().and_then(|s| s.parse().ok()),
                price_change: stats["priceChangePercent"].as_str().and_then(|s| s.parse().ok()),
                volume:       stats["volume"].as_str().and_then(|s| s.parse().ok()),
                volume_usd:   stats["quoteVolume"].as_str().and_then(|s| s.parse().ok()),
            },
        })
    }
}

// ── Private Endpoints ──────────────────────────────────────────────────────

pub async fn place_order(req: &PlaceOrderRequest, account: &Account) -> Result<OrderResult, String> {
    let client = Client::new();

    if is_futures(&req.instrument_name) {
        place_futures_order(req, account, &client).await
    } else {
        place_spot_order(req, account, &client).await
    }
}

async fn place_spot_order(req: &PlaceOrderRequest, account: &Account, client: &Client) -> Result<OrderResult, String> {
    let url = format!("{}/api/v3/order", SPOT_BASE);
    let side = if req.side == "buy" { "BUY" } else { "SELL" };
    let order_type = match req.order_type.as_str() {
        "market" => "MARKET",
        _        => "LIMIT_MAKER",
    };
    let tif = match req.time_in_force.as_deref().unwrap_or("good_til_cancelled") {
        "good_til_cancelled" | "gtc" => "GTC",
        "immediate_or_cancel" | "ioc" => "IOC",
        "fill_or_kill" | "fok"        => "FOK",
        _                             => "GTC",
    };

    let mut params = format!(
        "symbol={}&side={}&type={}&quantity={}&timeInForce={}",
        req.instrument_name, side, order_type, req.amount, tif
    );
    if let Some(price) = req.price {
        params.push_str(&format!("&price={}", price));
    }
    if let Some(ref clord) = req.client_order_id {
        let t = if clord.len() > 36 { &clord[..36] } else { clord.as_str() };
        params.push_str(&format!("&newClientOrderId={}", t));
    }

    let qs = signed_query(&params, &account.api_secret);
    let headers = api_key_header(&account.api_key);

    let resp = client.post(&url)
        .headers(headers)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(qs)
        .send().await.map_err(|e| e.to_string())?;

    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    if v["code"].as_i64().map(|c| c != 0).unwrap_or(false) {
        let msg = v["msg"].as_str().unwrap_or("unknown error").to_string();
        return Err(format!("MEXC order error ({}): {}", v["code"].as_i64().unwrap_or(0), msg));
    }

    Ok(OrderResult { success: true, order: Some(parse_spot_order(&v)), error: None })
}

async fn place_futures_order(req: &PlaceOrderRequest, account: &Account, client: &Client) -> Result<OrderResult, String> {
    let url = format!("{}/api/v1/private/order/submit", FUTURES_BASE);
    let side = if req.side == "buy" { 1 } else { 2 }; // 1=open long/close short, 2=open short/close long
    let order_type = match req.order_type.as_str() {
        "market" => 5, // market order
        _        => 1, // limit order
    };

    let body = serde_json::json!({
        "symbol": req.instrument_name,
        "side": side,
        "orderType": order_type,
        "vol": req.amount,
        "price": req.price,
        "externalOid": req.client_order_id,
        "openType": 1, // 1=isolated
        "positionType": if req.side == "buy" { 1 } else { 2 }, // 1=long, 2=short
    });

    let body_str = body.to_string();
    let ts = now_ms();
    let sign_str = format!("{}{}{}", account.api_key, ts, body_str);
    let sig = sign(&sign_str, &account.api_secret);

    let mut headers = api_key_header(&account.api_key);
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert("Request-Time", HeaderValue::from_str(&ts.to_string()).unwrap());
    headers.insert("Signature", HeaderValue::from_str(&sig).unwrap_or_else(|_| HeaderValue::from_static("")));

    let resp = client.post(&url)
        .headers(headers)
        .body(body_str)
        .send().await.map_err(|e| e.to_string())?;

    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    if v["success"].as_bool() != Some(true) {
        let msg = v["message"].as_str().unwrap_or("unknown error").to_string();
        return Err(format!("MEXC futures order error: {}", msg));
    }

    // Futures order submission returns just the order id
    let order_id = v["data"].as_str().unwrap_or("").to_string();
    let order = Order {
        order_id: order_id.clone(),
        instrument_name: req.instrument_name.clone(),
        direction: req.side.clone(),
        order_type: req.order_type.clone(),
        order_state: "open".to_string(),
        price: req.price,
        amount: req.amount,
        filled_amount: 0.0,
        average_price: None,
        post_only: false,
        time_in_force: "good_til_cancelled".to_string(),
        creation_timestamp: ts as i64,
        last_update_timestamp: ts as i64,
    };

    Ok(OrderResult { success: true, order: Some(order), error: None })
}

pub async fn cancel_order(order_id: &str, instrument_name: Option<&str>, account: &Account) -> Result<bool, String> {
    let client = Client::new();

    if instrument_name.map(is_futures).unwrap_or(false) {
        // Futures cancel
        let url = format!("{}/api/v1/private/order/cancel", FUTURES_BASE);
        let body = serde_json::json!({ "orderId": order_id });
        let body_str = body.to_string();
        let ts = now_ms();
        let sign_str = format!("{}{}{}", account.api_key, ts, body_str);
        let sig = sign(&sign_str, &account.api_secret);

        let mut headers = api_key_header(&account.api_key);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("Request-Time", HeaderValue::from_str(&ts.to_string()).unwrap());
        headers.insert("Signature", HeaderValue::from_str(&sig).unwrap_or_else(|_| HeaderValue::from_static("")));

        let resp = client.post(&url).headers(headers).body(body_str).send().await.map_err(|e| e.to_string())?;
        let v: Value = resp.json().await.map_err(|e| e.to_string())?;
        if v["success"].as_bool() != Some(true) {
            let msg = v["message"].as_str().unwrap_or("cancel error").to_string();
            return Err(format!("MEXC futures cancel error: {}", msg));
        }
    } else {
        // Spot cancel
        let symbol = instrument_name.unwrap_or("");
        let params = format!("symbol={}&orderId={}", symbol, order_id);
        let qs = signed_query(&params, &account.api_secret);
        let url = format!("{}/api/v3/order?{}", SPOT_BASE, qs);
        let headers = api_key_header(&account.api_key);

        let resp = client.delete(&url).headers(headers).send().await.map_err(|e| e.to_string())?;
        let v: Value = resp.json().await.map_err(|e| e.to_string())?;
        if v["code"].as_i64().map(|c| c != 0).unwrap_or(false) {
            let msg = v["msg"].as_str().unwrap_or("cancel error").to_string();
            return Err(format!("MEXC cancel error ({}): {}", v["code"].as_i64().unwrap_or(0), msg));
        }
    }
    Ok(true)
}

pub async fn get_open_orders(instrument_name: &str, account: &Account) -> Result<Vec<Order>, String> {
    let client = Client::new();

    if is_futures(instrument_name) {
        get_futures_open_orders(instrument_name, account, &client).await
    } else {
        get_spot_open_orders(instrument_name, account, &client).await
    }
}

async fn get_spot_open_orders(instrument_name: &str, account: &Account, client: &Client) -> Result<Vec<Order>, String> {
    let params = if instrument_name.is_empty() {
        signed_query("", &account.api_secret)
    } else {
        signed_query(&format!("symbol={}", instrument_name), &account.api_secret)
    };
    let url = format!("{}/api/v3/openOrders?{}", SPOT_BASE, params);
    let headers = api_key_header(&account.api_key);

    let resp = client.get(&url).headers(headers).send().await.map_err(|e| e.to_string())?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;

    Ok(v.as_array()
        .map(|arr| arr.iter().map(parse_spot_order).collect())
        .unwrap_or_default())
}

async fn get_futures_open_orders(instrument_name: &str, account: &Account, client: &Client) -> Result<Vec<Order>, String> {
    let url = format!("{}/api/v1/private/order/list/open_orders/{}", FUTURES_BASE, instrument_name);
    let ts = now_ms();
    let sign_str = format!("{}{}", account.api_key, ts);
    let sig = sign(&sign_str, &account.api_secret);

    let mut headers = api_key_header(&account.api_key);
    headers.insert("Request-Time", HeaderValue::from_str(&ts.to_string()).unwrap());
    headers.insert("Signature", HeaderValue::from_str(&sig).unwrap_or_else(|_| HeaderValue::from_static("")));

    let resp = client.get(&url).headers(headers).send().await.map_err(|e| e.to_string())?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;

    Ok(v["data"].as_array().map(|arr| arr.iter().map(parse_futures_order).collect()).unwrap_or_default())
}

pub async fn get_all_open_orders(account: &Account) -> Result<Vec<Order>, String> {
    let client = Client::new();
    // Spot open orders (all symbols)
    let mut orders = get_spot_open_orders("", account, &client).await.unwrap_or_default();
    // Futures open orders — we'd need a symbol list; for now return spot only unless extended
    let _ = client;
    Ok(orders)
}

pub async fn get_account_summary(currency: &str, account: &Account) -> Result<AccountSummary, String> {
    let client = Client::new();
    let qs = signed_query("", &account.api_secret);
    let url = format!("{}/api/v3/account?{}", SPOT_BASE, qs);
    let headers = api_key_header(&account.api_key);

    let resp = client.get(&url).headers(headers).send().await.map_err(|e| e.to_string())?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;

    // Find balance for requested currency
    let target = if currency.is_empty() { "USDT" } else { currency };
    let (free, locked) = v["balances"].as_array()
        .and_then(|arr| arr.iter().find(|b| b["asset"].as_str() == Some(target)))
        .map(|b| (
            b["free"].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0),
            b["locked"].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0),
        ))
        .unwrap_or((0.0, 0.0));

    Ok(AccountSummary {
        currency:           target.to_string(),
        equity:             free + locked,
        available_funds:    free,
        initial_margin:     locked,
        maintenance_margin: 0.0,
        unrealized_pl:      0.0,
    })
}

pub async fn get_trade_history(account: &Account, start_ms: i64, end_ms: i64) -> Result<Vec<Trade>, String> {
    let client = Client::new();
    let params = signed_query(
        &format!("startTime={}&endTime={}&limit=1000", start_ms, end_ms),
        &account.api_secret,
    );
    let url = format!("{}/api/v3/myTrades?{}", SPOT_BASE, params);
    let headers = api_key_header(&account.api_key);

    let resp = client.get(&url).headers(headers).send().await.map_err(|e| e.to_string())?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;

    Ok(v.as_array().map(|arr| {
        arr.iter().map(|t| Trade {
            trade_id:        t["id"].as_str().unwrap_or("").to_string(),
            account_id:      account.id.clone(),
            account_name:    account.name.clone(),
            exchange:        "mexc".to_string(),
            instrument_name: t["symbol"].as_str().unwrap_or("").to_string(),
            direction:       if t["isBuyer"].as_bool().unwrap_or(false) { "buy".to_string() } else { "sell".to_string() },
            amount:          t["qty"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
            price:           t["price"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
            fee:             t["commission"].as_str().and_then(|s| s.parse::<f64>().ok()).map(|f| f.abs()).unwrap_or(0.0),
            fee_currency:    t["commissionAsset"].as_str().unwrap_or("USDT").to_string(),
            timestamp:       t["time"].as_i64().unwrap_or(0),
            order_id:        t["orderId"].as_str().unwrap_or("").to_string(),
        }).collect()
    }).unwrap_or_default())
}

pub async fn get_positions(_currency: &str, account: &Account) -> Result<Vec<Position>, String> {
    let client = Client::new();
    let ts = now_ms();
    let sign_str = format!("{}{}", account.api_key, ts);
    let sig = sign(&sign_str, &account.api_secret);

    let mut headers = api_key_header(&account.api_key);
    headers.insert("Request-Time", HeaderValue::from_str(&ts.to_string()).unwrap());
    headers.insert("Signature", HeaderValue::from_str(&sig).unwrap_or_else(|_| HeaderValue::from_static("")));

    let url = format!("{}/api/v1/private/position/open_positions", FUTURES_BASE);
    let resp = client.get(&url).headers(headers).send().await.map_err(|e| e.to_string())?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;

    Ok(v["data"].as_array().map(|arr| {
        arr.iter().filter_map(|p| {
            let size: f64 = p["holdVol"].as_f64().unwrap_or(0.0);
            if size == 0.0 { return None; }
            let pos_type = p["positionType"].as_i64().unwrap_or(1);
            let direction = if pos_type == 1 { "long" } else { "short" };
            Some(Position {
                instrument_name: p["symbol"].as_str().unwrap_or("").to_string(),
                direction:       direction.to_string(),
                size,
                average_price:   p["openAvgPrice"].as_f64().unwrap_or(0.0),
                mark_price:      p["closeAvgPrice"].as_f64().unwrap_or(0.0),
                mark_iv:         0.0,
                unrealized_pnl:  p["unrealisedPnl"].as_f64().unwrap_or(0.0),
                delta:           if direction == "long" { size } else { -size },
                gamma:           0.0,
                theta:           0.0,
                vega:            0.0,
            })
        }).collect()
    }).unwrap_or_default())
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn parse_spot_order(o: &Value) -> Order {
    let state = match o["status"].as_str().unwrap_or("") {
        "NEW"              => "open",
        "PARTIALLY_FILLED" => "open",
        "FILLED"           => "filled",
        "CANCELED"         => "cancelled",
        "REJECTED"         => "rejected",
        other              => other,
    };
    let tif = match o["timeInForce"].as_str().unwrap_or("GTC") {
        "GTC" => "good_til_cancelled",
        "IOC" => "immediate_or_cancel",
        "FOK" => "fill_or_kill",
        other => other,
    };
    Order {
        order_id:              o["orderId"].to_string().trim_matches('"').to_string(),
        instrument_name:       o["symbol"].as_str().unwrap_or("").to_string(),
        direction:             o["side"].as_str().unwrap_or("").to_lowercase(),
        order_type:            o["type"].as_str().unwrap_or("").to_lowercase(),
        order_state:           state.to_string(),
        price:                 o["price"].as_str().and_then(|s| s.parse().ok()),
        amount:                o["origQty"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        filled_amount:         o["executedQty"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        average_price:         o["price"].as_str().and_then(|s| s.parse().ok()),
        post_only:             false,
        time_in_force:         tif.to_string(),
        creation_timestamp:    o["time"].as_i64().unwrap_or(0),
        last_update_timestamp: o["updateTime"].as_i64().unwrap_or(0),
    }
}

fn parse_futures_order(o: &Value) -> Order {
    let state = match o["state"].as_i64().unwrap_or(0) {
        1 => "open",      // Submitting
        2 => "open",      // Submitted
        3 => "open",      // Partially filled
        4 => "filled",    // Filled
        5 => "cancelled", // Canceled
        _ => "unknown",
    };
    let side = match o["side"].as_i64().unwrap_or(1) {
        1 => "buy",
        2 => "sell",
        _ => "buy",
    };
    Order {
        order_id:              o["orderId"].as_str().unwrap_or("").to_string(),
        instrument_name:       o["symbol"].as_str().unwrap_or("").to_string(),
        direction:             side.to_string(),
        order_type:            "limit".to_string(),
        order_state:           state.to_string(),
        price:                 o["price"].as_f64(),
        amount:                o["vol"].as_f64().unwrap_or(0.0),
        filled_amount:         o["dealVol"].as_f64().unwrap_or(0.0),
        average_price:         o["dealAvgPrice"].as_f64(),
        post_only:             false,
        time_in_force:         "good_til_cancelled".to_string(),
        creation_timestamp:    o["createTime"].as_i64().unwrap_or(0),
        last_update_timestamp: o["updateTime"].as_i64().unwrap_or(0),
    }
}
