/// Binance USDT-M Futures REST API
///
/// Base URL: https://fapi.binance.com  (testnet: https://testnet.binancefuture.com)
/// Auth:     HMAC-SHA256 of the full query string/body, appended as `signature=`
/// Header:   X-MBX-APIKEY: <api_key>
///
/// Instrument format: BTCUSDT, ETHUSDT, etc.

use reqwest::{Client, header::{HeaderMap, HeaderValue}};
use serde_json::{json, Value};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::models::{
    Account, AccountSummary, Instrument, Order, OrderResult, OrderbookLevel, OrderbookSnapshot,
    PlaceOrderRequest, Position, Ticker, TickerStats, Trade,
};

type HmacSha256 = Hmac<Sha256>;

const FAPI_BASE: &str      = "https://fapi.binance.com";
const FAPI_TEST: &str      = "https://testnet.binancefuture.com";
const RECV_WINDOW: u64     = 5000;

fn base(testnet: bool) -> &'static str {
    if testnet { FAPI_TEST } else { FAPI_BASE }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn sign(query: &str, secret: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(query.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn api_key_header(api_key: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(
        "X-MBX-APIKEY",
        HeaderValue::from_str(api_key).unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    h
}

/// Append timestamp + recvWindow + signature to a query string.
fn signed_query(params: &str, secret: &str) -> String {
    let ts = now_ms();
    let base = if params.is_empty() {
        format!("timestamp={}&recvWindow={}", ts, RECV_WINDOW)
    } else {
        format!("{}&timestamp={}&recvWindow={}", params, ts, RECV_WINDOW)
    };
    let sig = sign(&base, secret);
    format!("{}&signature={}", base, sig)
}

/// Append timestamp + recvWindow + signature to a JSON body by rebuilding as form params.
fn signed_body_params(mut pairs: Vec<(&str, String)>, secret: &str) -> String {
    let ts = now_ms();
    pairs.push(("timestamp", ts.to_string()));
    pairs.push(("recvWindow", RECV_WINDOW.to_string()));
    let body: String = pairs.iter()
        .map(|(k, v)| format!("{}={}", k, urlencoding_simple(v)))
        .collect::<Vec<_>>()
        .join("&");
    let sig = sign(&body, secret);
    format!("{}&signature={}", body, sig)
}

fn urlencoding_simple(s: &str) -> String {
    // Minimal percent-encoding for common characters in order params
    s.replace('%', "%25").replace('&', "%26").replace('+', "%2B").replace(' ', "%20")
}

// ── Public Endpoints ───────────────────────────────────────────────────────

pub async fn fetch_instruments(currency: &str, kind: &str) -> Result<Vec<Instrument>, String> {
    let client = Client::new();
    let url = format!("{}/fapi/v1/exchangeInfo", FAPI_BASE);
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let json: Value = resp.json().await.map_err(|e| e.to_string())?;

    let mut instruments = Vec::new();
    if let Some(symbols) = json["symbols"].as_array() {
        for s in symbols {
            let base_asset = s["baseAsset"].as_str().unwrap_or("");
            let quote_asset = s["quoteAsset"].as_str().unwrap_or("");
            let status = s["status"].as_str().unwrap_or("");
            let contract_type = s["contractType"].as_str().unwrap_or("");

            // Filter by currency if specified
            if !currency.is_empty() && !base_asset.to_uppercase().contains(&currency.to_uppercase()) {
                continue;
            }
            // Filter by kind
            let k = match kind {
                "future" | "futures" => "PERPETUAL",
                "spot"               => "SPOT",
                _                    => "", // all
            };
            if !k.is_empty() && contract_type != k {
                continue;
            }
            if status != "TRADING" { continue; }

            let symbol = s["symbol"].as_str().unwrap_or("").to_string();
            instruments.push(Instrument {
                instrument_name:       symbol.clone(),
                kind:                  contract_type.to_lowercase(),
                base_currency:         base_asset.to_string(),
                quote_currency:        quote_asset.to_string(),
                settlement_currency:   quote_asset.to_string(),
                is_active:             true,
                tick_size:             parse_filter_f64(s, "PRICE_FILTER", "tickSize"),
                min_trade_amount:      parse_filter_f64(s, "LOT_SIZE", "minQty"),
                qty_step:              Some(parse_filter_f64(s, "LOT_SIZE", "stepSize")).filter(|&v| v > 0.0),
                contract_size:         Some(1.0),
                option_type:           None,
                strike:                None,
                expiration_timestamp:  s["deliveryDate"].as_i64(),
            });
        }
    }
    Ok(instruments)
}

fn parse_filter_f64(s: &Value, filter_type: &str, field: &str) -> f64 {
    if let Some(filters) = s["filters"].as_array() {
        for f in filters {
            if f["filterType"].as_str() == Some(filter_type) {
                return f[field].as_str().and_then(|v| v.parse().ok()).unwrap_or(0.0);
            }
        }
    }
    0.0
}

pub async fn fetch_ticker(instrument_name: &str) -> Result<Ticker, String> {
    let client = Client::new();
    // Use book ticker + 24hr stats
    let book_url = format!("{}/fapi/v1/ticker/bookTicker?symbol={}", FAPI_BASE, instrument_name);
    let stats_url = format!("{}/fapi/v1/ticker/24hr?symbol={}", FAPI_BASE, instrument_name);

    let (book_resp, stats_resp) = tokio::join!(
        client.get(&book_url).send(),
        client.get(&stats_url).send(),
    );

    let book: Value = book_resp.map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;
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
        open_interest:   stats["openInterest"].as_str().and_then(|s| s.parse().ok()),
        mark_iv:         None,
        bid_iv:          None,
        ask_iv:          None,
        delta:           None,
        gamma:           None,
        vega:            None,
        theta:           None,
        stats: TickerStats {
            high:         stats["highPrice"].as_str().and_then(|s| s.parse().ok()),
            low:          stats["lowPrice"].as_str().and_then(|s| s.parse().ok()),
            price_change: stats["priceChangePercent"].as_str().and_then(|s| s.parse().ok()),
            volume:       stats["volume"].as_str().and_then(|s| s.parse().ok()),
            volume_usd:   stats["quoteVolume"].as_str().and_then(|s| s.parse().ok()),
        },
    })
}

// ── Private Endpoints ──────────────────────────────────────────────────────

pub async fn place_order(req: &PlaceOrderRequest, account: &Account) -> Result<OrderResult, String> {
    let client = Client::new();
    let url = format!("{}/fapi/v1/order", base(account.testnet));

    let side = if req.side == "buy" { "BUY" } else { "SELL" };
    let order_type = match req.order_type.as_str() {
        "market" => "MARKET",
        "limit"  => "LIMIT",
        _        => "LIMIT",
    };
    let tif = match req.time_in_force.as_deref().unwrap_or("good_til_cancelled") {
        "good_til_cancelled" | "gtc" => "GTC",
        "immediate_or_cancel" | "ioc" => "IOC",
        "fill_or_kill" | "fok"        => "FOK",
        _                             => "GTC",
    };

    let mut pairs: Vec<(&str, String)> = vec![
        ("symbol",      req.instrument_name.clone()),
        ("side",        side.to_string()),
        ("type",        order_type.to_string()),
        ("quantity",    req.amount.to_string()),
    ];
    if order_type == "LIMIT" {
        if let Some(price) = req.price {
            pairs.push(("price", price.to_string()));
        }
        pairs.push(("timeInForce", tif.to_string()));
    }
    if let Some(ref clord) = req.client_order_id {
        // Binance allows max 36 chars for newClientOrderId
        let truncated = if clord.len() > 36 { &clord[..36] } else { clord.as_str() };
        pairs.push(("newClientOrderId", truncated.to_string()));
    }

    let body = signed_body_params(pairs, &account.api_secret);
    let headers = api_key_header(&account.api_key);

    let resp = client.post(&url)
        .headers(headers)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send().await.map_err(|e| e.to_string())?;

    let v: Value = resp.json().await.map_err(|e| e.to_string())?;

    if v["orderId"].is_null() {
        let msg = v["msg"].as_str().unwrap_or("Unknown Binance error").to_string();
        return Err(format!("Binance order error ({}): {}", v["code"].as_i64().unwrap_or(0), msg));
    }

    Ok(OrderResult {
        success: true,
        order: Some(parse_order(&v)),
        error: None,
    })
}

pub async fn cancel_order(order_id: &str, instrument_name: Option<&str>, account: &Account) -> Result<bool, String> {
    let client = Client::new();
    let symbol = instrument_name.unwrap_or("");
    let qs = signed_query(
        &format!("symbol={}&orderId={}", symbol, order_id),
        &account.api_secret,
    );
    let url = format!("{}/fapi/v1/order?{}", base(account.testnet), qs);
    let headers = api_key_header(&account.api_key);

    let resp = client.delete(&url).headers(headers).send().await.map_err(|e| e.to_string())?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;

    if v["code"].as_i64().map(|c| c < 0).unwrap_or(false) {
        let msg = v["msg"].as_str().unwrap_or("cancel error").to_string();
        return Err(format!("Binance cancel error ({}): {}", v["code"].as_i64().unwrap_or(0), msg));
    }
    Ok(true)
}

pub async fn get_open_orders(instrument_name: &str, account: &Account) -> Result<Vec<Order>, String> {
    let client = Client::new();
    let params = if instrument_name.is_empty() {
        signed_query("", &account.api_secret)
    } else {
        signed_query(&format!("symbol={}", instrument_name), &account.api_secret)
    };
    let url = format!("{}/fapi/v1/openOrders?{}", base(account.testnet), params);
    let headers = api_key_header(&account.api_key);

    let resp = client.get(&url).headers(headers).send().await.map_err(|e| e.to_string())?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;

    Ok(v.as_array()
        .map(|arr| arr.iter().map(parse_order).collect())
        .unwrap_or_default())
}

pub async fn get_all_open_orders(account: &Account) -> Result<Vec<Order>, String> {
    get_open_orders("", account).await
}

pub async fn get_account_summary(currency: &str, account: &Account) -> Result<AccountSummary, String> {
    let client = Client::new();
    let qs = signed_query("", &account.api_secret);
    let url = format!("{}/fapi/v2/account?{}", base(account.testnet), qs);
    let headers = api_key_header(&account.api_key);

    let resp = client.get(&url).headers(headers).send().await.map_err(|e| e.to_string())?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;

    // Binance futures accounts are denominated in USDT
    let equity:    f64 = v["totalWalletBalance"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let unreal_pnl:f64 = v["totalUnrealizedProfit"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let init_margin:f64= v["totalInitialMargin"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let maint_margin:f64=v["totalMaintMargin"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let avail:     f64 = v["availableBalance"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);

    Ok(AccountSummary {
        currency: if currency.is_empty() { "USDT".to_string() } else { currency.to_string() },
        equity: equity + unreal_pnl,
        available_funds: avail,
        initial_margin: init_margin,
        maintenance_margin: maint_margin,
        unrealized_pl: unreal_pnl,
    })
}

pub async fn get_trade_history(account: &Account, start_ms: i64, end_ms: i64) -> Result<Vec<Trade>, String> {
    let client = Client::new();
    let params = signed_query(
        &format!("startTime={}&endTime={}&limit=1000", start_ms, end_ms),
        &account.api_secret,
    );
    let url = format!("{}/fapi/v1/userTrades?{}", base(account.testnet), params);
    let headers = api_key_header(&account.api_key);

    let resp = client.get(&url).headers(headers).send().await.map_err(|e| e.to_string())?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;

    Ok(v.as_array().map(|arr| {
        arr.iter().map(|t| Trade {
            trade_id:        t["id"].to_string().trim_matches('"').to_string(),
            account_id:      account.id.clone(),
            account_name:    account.name.clone(),
            exchange:        "binance".to_string(),
            instrument_name: t["symbol"].as_str().unwrap_or("").to_string(),
            direction:       if t["buyer"].as_bool().unwrap_or(false) { "buy".to_string() } else { "sell".to_string() },
            amount:          t["qty"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
            price:           t["price"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
            fee:             t["commission"].as_str().and_then(|s| s.parse::<f64>().ok()).map(|f| f.abs()).unwrap_or(0.0),
            fee_currency:    t["commissionAsset"].as_str().unwrap_or("USDT").to_string(),
            timestamp:       t["time"].as_i64().unwrap_or(0),
            order_id:        t["orderId"].to_string().trim_matches('"').to_string(),
        }).collect()
    }).unwrap_or_default())
}

pub async fn get_positions(_currency: &str, account: &Account) -> Result<Vec<Position>, String> {
    let client = Client::new();
    let qs = signed_query("", &account.api_secret);
    let url = format!("{}/fapi/v2/positionRisk?{}", base(account.testnet), qs);
    let headers = api_key_header(&account.api_key);

    let resp = client.get(&url).headers(headers).send().await.map_err(|e| e.to_string())?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;

    Ok(v.as_array().map(|arr| {
        arr.iter().filter_map(|p| {
            let size: f64 = p["positionAmt"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            if size == 0.0 { return None; }
            let direction = if size > 0.0 { "long" } else { "short" };
            Some(Position {
                instrument_name: p["symbol"].as_str().unwrap_or("").to_string(),
                direction:       direction.to_string(),
                size:            size.abs(),
                average_price:   p["entryPrice"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                mark_price:      p["markPrice"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                mark_iv:         0.0,
                unrealized_pnl:  p["unRealizedProfit"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                delta:           size.signum(), // simple delta approximation for futures
                gamma:           0.0,
                theta:           0.0,
                vega:            0.0,
            })
        }).collect()
    }).unwrap_or_default())
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn parse_order(o: &Value) -> Order {
    let state = match o["status"].as_str().unwrap_or("") {
        "NEW"              => "open",
        "PARTIALLY_FILLED" => "open",
        "FILLED"           => "filled",
        "CANCELED"         => "cancelled",
        "EXPIRED"          => "cancelled",
        "REJECTED"         => "rejected",
        other              => other,
    };
    let tif = match o["timeInForce"].as_str().unwrap_or("GTC") {
        "GTC" => "good_til_cancelled",
        "IOC" => "immediate_or_cancel",
        "FOK" => "fill_or_kill",
        other => other,
    };

    let order_id = if let Some(id) = o["orderId"].as_i64() {
        id.to_string()
    } else {
        o["orderId"].as_str().unwrap_or("").to_string()
    };

    Order {
        order_id,
        instrument_name:       o["symbol"].as_str().unwrap_or("").to_string(),
        direction:             o["side"].as_str().unwrap_or("").to_lowercase(),
        order_type:            o["type"].as_str().unwrap_or("").to_lowercase(),
        order_state:           state.to_string(),
        price:                 o["price"].as_str().and_then(|s| s.parse().ok()),
        amount:                o["origQty"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        filled_amount:         o["executedQty"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        average_price:         o["avgPrice"].as_str().and_then(|s| s.parse().ok()),
        post_only:             o["timeInForce"].as_str() == Some("GTX"),
        time_in_force:         tif.to_string(),
        creation_timestamp:    o["time"].as_i64().or_else(|| o["updateTime"].as_i64()).unwrap_or(0),
        last_update_timestamp: o["updateTime"].as_i64().unwrap_or(0),
    }
}

pub async fn fetch_orderbook(instrument_name: &str, depth: u32) -> Result<OrderbookSnapshot, String> {
    let client = Client::new();
    // Use spot endpoint for spot-like symbols (no futures suffix), fapi otherwise
    let is_spot = !instrument_name.ends_with("USDT")
        || instrument_name.contains('-')
        || instrument_name.to_uppercase() == instrument_name.to_uppercase();
    // Heuristic: if symbol is plain BTCUSDT-style without a delivery date, use fapi
    // For simplicity, use FAPI for all; many Binance users in this dashboard use futures
    let url = format!("{}/fapi/v1/depth?symbol={}&limit={}", FAPI_BASE, instrument_name, depth);

    let resp: Value = client.get(&url).send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    // FAPI returns bids/asks arrays or an error with code/msg
    if resp.get("code").is_some() {
        // Try spot API as fallback
        let spot_url = format!("https://api.binance.com/api/v3/depth?symbol={}&limit={}", instrument_name, depth);
        let spot_resp: Value = client.get(&spot_url).send().await.map_err(|e| e.to_string())?
            .json().await.map_err(|e| e.to_string())?;
        if spot_resp.get("code").is_some() {
            return Err(spot_resp["msg"].as_str().unwrap_or("Binance error").to_string());
        }
        let parse_levels = |arr: &Value| -> Vec<OrderbookLevel> {
            arr.as_array().map(|a| a.iter().filter_map(|item| {
                let arr = item.as_array()?;
                Some(OrderbookLevel {
                    price: arr.get(0)?.as_str()?.parse().ok()?,
                    size:  arr.get(1)?.as_str()?.parse().ok()?,
                })
            }).collect()).unwrap_or_default()
        };
        return Ok(OrderbookSnapshot {
            instrument_name: instrument_name.to_string(),
            bids: parse_levels(&spot_resp["bids"]),
            asks: parse_levels(&spot_resp["asks"]),
            timestamp: ts,
        });
    }

    let _ = is_spot; // suppress warning
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
        bids: parse_levels(&resp["bids"]),
        asks: parse_levels(&resp["asks"]),
        timestamp: ts,
    })
}
