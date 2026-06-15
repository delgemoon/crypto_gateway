use reqwest::Client;
use serde_json::{json, Value};

use crate::api::models::{
    AccountSummary, AuthToken, Instrument, Order, OrderResult, OrderbookLevel, OrderbookSnapshot,
    PlaceOrderRequest, Position, Ticker, TickerStats, Trade, TransactionLog,
};

const DERIBIT_BASE: &str = "https://www.deribit.com/api/v2";
const DERIBIT_TEST_BASE: &str = "https://test.deribit.com/api/v2";

fn base_url(testnet: bool) -> &'static str {
    if testnet {
        DERIBIT_TEST_BASE
    } else {
        DERIBIT_BASE
    }
}

/// Fetch all active instruments for a given currency and kind.
pub async fn fetch_instruments(
    currency: &str,
    kind: &str,
) -> Result<Vec<Instrument>, String> {
    let client = Client::new();
    let url = format!(
        "{}/public/get_instruments?currency={}&kind={}&expired=false",
        DERIBIT_BASE, currency, kind
    );

    let resp: Value = client
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(result) = resp.get("result") {
        let instruments: Vec<Instrument> =
            serde_json::from_value(result.clone()).map_err(|e| e.to_string())?;
        Ok(instruments)
    } else {
        let err = resp
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error")
            .to_string();
        Err(err)
    }
}

/// Fetch ticker for a specific instrument.
pub async fn fetch_ticker(instrument_name: &str) -> Result<Ticker, String> {
    let client = Client::new();
    let url = format!(
        "{}/public/ticker?instrument_name={}",
        DERIBIT_BASE, instrument_name
    );

    let resp: Value = client
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(result) = resp.get("result") {
        parse_ticker(result)
    } else {
        let err = resp
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error")
            .to_string();
        Err(err)
    }
}

fn parse_ticker(v: &Value) -> Result<Ticker, String> {
    let binding = serde_json::json!({});
    let stats = v.get("stats").unwrap_or(&binding);
    Ok(Ticker {
        instrument_name: v["instrument_name"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        best_bid_price: v["best_bid_price"].as_f64(),
        best_ask_price: v["best_ask_price"].as_f64(),
        best_bid_amount: v["best_bid_amount"].as_f64(),
        best_ask_amount: v["best_ask_amount"].as_f64(),
        last_price: v["last_price"].as_f64(),
        mark_price: v["mark_price"].as_f64(),
        index_price: v["index_price"].as_f64(),
        // Deribit provides underlying_price (forward) separately from index_price (spot)
        underlying_price: v["underlying_price"].as_f64(),
        open_interest: v["open_interest"].as_f64(),
        stats: TickerStats {
            high: stats["high"].as_f64(),
            low: stats["low"].as_f64(),
            price_change: stats["price_change"].as_f64(),
            volume: stats["volume"].as_f64(),
            volume_usd: stats["volume_usd"].as_f64(),
        },
        mark_iv: v["mark_iv"].as_f64(),
        bid_iv: v["bid_iv"].as_f64(),
        ask_iv: v["ask_iv"].as_f64(),
        delta: v.get("greeks").and_then(|g| g["delta"].as_f64()),
        gamma: v.get("greeks").and_then(|g| g["gamma"].as_f64()),
        vega: v.get("greeks").and_then(|g| g["vega"].as_f64()),
        theta: v.get("greeks").and_then(|g| g["theta"].as_f64()),
    })
}

/// Authenticate with Deribit and return a bearer token.
pub async fn authenticate(
    api_key: &str,
    api_secret: &str,
    testnet: bool,
) -> Result<AuthToken, String> {
    let client = Client::new();
    let url = format!("{}/public/auth", base_url(testnet));
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "public/auth",
        "params": {
            "grant_type": "client_credentials",
            "client_id": api_key,
            "client_secret": api_secret
        }
    });

    let resp: Value = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(result) = resp.get("result") {
        Ok(AuthToken {
            access_token: result["access_token"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            expires_in: result["expires_in"].as_i64().unwrap_or(0),
            scope: result["scope"].as_str().unwrap_or("").to_string(),
            token_type: result["token_type"].as_str().unwrap_or("").to_string(),
        })
    } else {
        let err = resp
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("Authentication failed")
            .to_string();
        Err(err)
    }
}

/// Place a buy or sell order on Deribit.
pub async fn place_order(
    req: &PlaceOrderRequest,
    api_key: &str,
    api_secret: &str,
    testnet: bool,
) -> Result<OrderResult, String> {
    let token = authenticate(api_key, api_secret, testnet).await?;
    let client = Client::new();

    let method = if req.side == "buy" {
        "private/buy"
    } else {
        "private/sell"
    };

    let url = format!("{}/{}", base_url(testnet), method);

    let mut params = json!({
        "instrument_name": req.instrument_name,
        "amount": req.amount,
        "type": req.order_type,
    });

    if let Some(price) = req.price {
        params["price"] = json!(price);
    }
    if let Some(ref tif) = req.time_in_force {
        params["time_in_force"] = json!(tif);
    }
    if let Some(po) = req.post_only {
        params["post_only"] = json!(po);
    }
    if let Some(ref label) = req.label {
        params["label"] = json!(label);
    } else if let Some(ref clord_id) = req.client_order_id {
        // Use client_order_id as label when no explicit label set
        params["label"] = json!(clord_id);
    }

    let body = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": method,
        "params": params
    });

    let resp: Value = client
        .post(&url)
        .bearer_auth(&token.access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(result) = resp.get("result") {
        let order_val = &result["order"];
        let order = parse_order(order_val).ok();
        Ok(OrderResult {
            success: true,
            order,
            error: None,
        })
    } else {
        let err = resp
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("Order failed")
            .to_string();
        Ok(OrderResult {
            success: false,
            order: None,
            error: Some(err),
        })
    }
}

/// Cancel an open order by order_id.
pub async fn cancel_order(
    order_id: &str,
    api_key: &str,
    api_secret: &str,
    testnet: bool,
) -> Result<bool, String> {
    let token = authenticate(api_key, api_secret, testnet).await?;
    let client = Client::new();
    let url = format!("{}/private/cancel", base_url(testnet));

    let body = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "private/cancel",
        "params": { "order_id": order_id }
    });

    let resp: Value = client
        .post(&url)
        .bearer_auth(&token.access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(err) = resp.get("error") {
        return Err(format!("Deribit cancel error: {}", err));
    }
    Ok(resp.get("result").is_some())
}

/// Get all open orders for an instrument.
pub async fn get_open_orders(
    instrument_name: &str,
    api_key: &str,
    api_secret: &str,
    testnet: bool,
) -> Result<Vec<Order>, String> {
    let token = authenticate(api_key, api_secret, testnet).await?;
    let client = Client::new();
    let url = format!("{}/private/get_open_orders_by_instrument", base_url(testnet));

    let body = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "private/get_open_orders_by_instrument",
        "params": { "instrument_name": instrument_name }
    });

    let resp: Value = client
        .post(&url)
        .bearer_auth(&token.access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(result) = resp.get("result") {
        if let Some(arr) = result.as_array() {
            let orders: Vec<Order> = arr.iter().filter_map(|o| parse_order(o).ok()).collect();
            Ok(orders)
        } else {
            Ok(vec![])
        }
    } else {
        let err = resp
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error")
            .to_string();
        Err(err)
    }
}

/// Get ALL open orders for this account (iterates BTC + ETH + SOL + USDC currencies).
pub async fn get_all_open_orders(api_key: &str, api_secret: &str, testnet: bool) -> Result<Vec<Order>, String> {
    let token = authenticate(api_key, api_secret, testnet).await?;
    let client = Client::new();
    let mut all = Vec::new();
    for currency in &["BTC", "ETH", "SOL", "USDC"] {
        let url = format!("{}/private/get_open_orders_by_currency", base_url(testnet));
        let body = json!({
            "jsonrpc": "2.0", "id": 10,
            "method": "private/get_open_orders_by_currency",
            "params": { "currency": currency }
        });
        let resp: Value = match client.post(&url).bearer_auth(&token.access_token)
            .json(&body).send().await {
            Ok(r) => r.json().await.unwrap_or(Value::Null),
            Err(_) => continue,
        };
        if let Some(arr) = resp["result"].as_array() {
            all.extend(arr.iter().filter_map(|o| parse_order(o).ok()));
        }
    }
    Ok(all)
}

/// Get trade history for this account (iterates BTC + ETH currencies).
pub async fn get_trade_history(api_key: &str, api_secret: &str, testnet: bool, start_ms: i64, end_ms: i64) -> Result<Vec<Trade>, String> {
    let token = authenticate(api_key, api_secret, testnet).await?;
    let client = Client::new();
    let mut all = Vec::new();
    for currency in &["BTC", "ETH", "SOL", "USDC"] {
        let url = format!("{}/private/get_user_trades_by_currency", base_url(testnet));
        let mut params = json!({ "currency": currency, "count": 200 });
        if start_ms > 0 { params["start_timestamp"] = json!(start_ms); }
        if end_ms > 0   { params["end_timestamp"]   = json!(end_ms); }
        let body = json!({
            "jsonrpc": "2.0", "id": 11,
            "method": "private/get_user_trades_by_currency",
            "params": params
        });
        let resp: Value = client.post(&url).bearer_auth(&token.access_token)
            .json(&body).send().await.map_err(|e| e.to_string())?
            .json().await.map_err(|e| e.to_string())?;
        if let Some(trades) = resp["result"]["trades"].as_array() {
            for t in trades {
                all.push(Trade {
                    trade_id:        t["trade_id"].as_str().unwrap_or("").to_string(),
                    account_id:      String::new(),
                    account_name:    String::new(),
                    exchange:        "deribit".to_string(),
                    instrument_name: t["instrument_name"].as_str().unwrap_or("").to_string(),
                    direction:       t["direction"].as_str().unwrap_or("").to_string(),
                    amount:          t["amount"].as_f64().unwrap_or(0.0),
                    price:           t["price"].as_f64().unwrap_or(0.0),
                    fee:             t["fee"].as_f64().unwrap_or(0.0),
                    fee_currency:    t["fee_currency"].as_str().unwrap_or("").to_string(),
                    timestamp:       t["timestamp"].as_i64().unwrap_or(0),
                    order_id:        t["order_id"].as_str().unwrap_or("").to_string(),
                });
            }
        }
    }
    Ok(all)
}

/// Get account summary for a currency.
pub async fn get_account_summary(
    currency: &str,
    api_key: &str,
    api_secret: &str,
    testnet: bool,
) -> Result<AccountSummary, String> {
    let token = authenticate(api_key, api_secret, testnet).await?;
    let client = Client::new();
    let url = format!("{}/private/get_account_summary", base_url(testnet));

    let body = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "private/get_account_summary",
        "params": { "currency": currency }
    });

    let resp: Value = client
        .post(&url)
        .bearer_auth(&token.access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(result) = resp.get("result") {
        Ok(AccountSummary {
            currency: result["currency"]
                .as_str()
                .unwrap_or(currency)
                .to_string(),
            equity: result["equity"].as_f64().unwrap_or(0.0),
            available_funds: result["available_funds"].as_f64().unwrap_or(0.0),
            initial_margin: result["initial_margin"].as_f64().unwrap_or(0.0),
            maintenance_margin: result["maintenance_margin"].as_f64().unwrap_or(0.0),
            unrealized_pl: result["session_upl"].as_f64().unwrap_or(0.0),
        })
    } else {
        let err = resp
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error")
            .to_string();
        Err(err)
    }
}

fn parse_order(v: &Value) -> Result<Order, String> {
    Ok(Order {
        order_id: v["order_id"]
            .as_str()
            .ok_or("missing order_id")?
            .to_string(),
        instrument_name: v["instrument_name"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        direction: v["direction"].as_str().unwrap_or("").to_string(),
        order_type: v["order_type"].as_str().unwrap_or("").to_string(),
        order_state: v["order_state"].as_str().unwrap_or("").to_string(),
        price: v["price"].as_f64(),
        amount: v["amount"].as_f64().unwrap_or(0.0),
        filled_amount: v["filled_amount"].as_f64().unwrap_or(0.0),
        average_price: v["average_price"].as_f64(),
        post_only: v["post_only"].as_bool().unwrap_or(false),
        time_in_force: v["time_in_force"].as_str().unwrap_or("good_til_cancelled").to_string(),
        creation_timestamp: v["creation_timestamp"].as_i64().unwrap_or(0),
        last_update_timestamp: v["last_update_timestamp"].as_i64().unwrap_or(0),
    })
}

/// Get open positions for a given currency (options + futures).
pub async fn get_positions(
    currency: &str,
    api_key: &str,
    api_secret: &str,
    testnet: bool,
) -> Result<Vec<Position>, String> {
    let token = authenticate(api_key, api_secret, testnet).await?;
    let client = Client::new();
    let url = format!("{}/private/get_positions", base_url(testnet));

    let body = json!({
        "jsonrpc": "2.0", "id": 10,
        "method": "private/get_positions",
        "params": { "currency": currency, "kind": "any" }
    });

    let resp: Value = client
        .post(&url)
        .bearer_auth(&token.access_token)
        .json(&body)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if let Some(err) = resp.get("error") {
        return Err(format!("Deribit positions error: {}", err));
    }

    let positions = resp["result"].as_array()
        .map(|arr| arr.iter().filter_map(|p| {
            let size = p["size"].as_f64().unwrap_or(0.0);
            if size == 0.0 { return None; }
            Some(Position {
                instrument_name: p["instrument_name"].as_str().unwrap_or("").to_string(),
                direction:       p["direction"].as_str().unwrap_or("").to_string(),
                size,
                average_price:   p["average_price"].as_f64().unwrap_or(0.0),
                mark_price:      p["mark_price"].as_f64().unwrap_or(0.0),
                mark_iv:         p["mark_iv"].as_f64().unwrap_or(0.0),
                unrealized_pnl:  p["floating_profit_loss"].as_f64().unwrap_or(0.0),
                delta:           p["delta"].as_f64().unwrap_or(0.0),
                gamma:           p["gamma"].as_f64().unwrap_or(0.0),
                theta:           p["theta"].as_f64().unwrap_or(0.0),
                vega:            p["vega"].as_f64().unwrap_or(0.0),
            })
        }).collect())
        .unwrap_or_default();
    Ok(positions)
}

pub async fn fetch_orderbook(instrument_name: &str, depth: u32) -> Result<OrderbookSnapshot, String> {
    let client = Client::new();
    let url = format!(
        "{}/public/get_order_book?instrument_name={}&depth={}",
        DERIBIT_BASE, instrument_name, depth
    );

    let resp: Value = client.get(&url).send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    if let Some(result) = resp.get("result") {
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
            timestamp: result["timestamp"].as_i64().unwrap_or(ts),
        })
    } else {
        let err = resp.get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error")
            .to_string();
        Err(err)
    }
}

/// Fetch transaction log for a single currency from Deribit.
/// Paginates automatically until all entries within the date range are fetched.
pub async fn get_transaction_log(
    api_key: &str,
    api_secret: &str,
    testnet: bool,
    currency: &str,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<TransactionLog>, String> {
    let token = authenticate(api_key, api_secret, testnet).await?;
    let client = Client::new();
    let url = format!("{}/private/get_transaction_log", base_url(testnet));

    let mut logs: Vec<TransactionLog> = Vec::new();
    let mut continuation: Option<i64> = None;

    loop {
        let mut params = serde_json::json!({
            "currency":        currency,
            "start_timestamp": start_ms,
            "end_timestamp":   end_ms,
            "count":           1000,
        });
        if let Some(cont) = continuation {
            params["continuation"] = serde_json::json!(cont);
        }

        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": 42,
            "method": "private/get_transaction_log",
            "params": params,
        });

        let resp: Value = client
            .post(&url)
            .bearer_auth(&token.access_token)
            .json(&body)
            .send().await.map_err(|e| e.to_string())?
            .json().await.map_err(|e| e.to_string())?;

        if let Some(err) = resp.get("error") {
            return Err(format!("Deribit transaction_log error: {}", err));
        }

        let result = &resp["result"];
        let entries = result["logs"].as_array()
            .ok_or_else(|| "Deribit: missing logs array".to_string())?;

        // Debug: print first entry so we can verify all field names at runtime
        if logs.is_empty() {
            if let Some(first) = entries.first() {
                eprintln!("[deribit] first tx_log entry (raw): {}", serde_json::to_string(first).unwrap_or_default());
            }
        }

        for e in entries {
            let info = if e["info"].is_object() {
                serde_json::to_string(&e["info"]).unwrap_or_default()
            } else {
                e["info"].as_str().unwrap_or("").to_string()
            };
            // Parse a JSON value that could be int, float, or string-encoded number
            let f64_val = |v: &Value| -> f64 {
                v.as_f64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                    .unwrap_or(0.0)
            };
            // Deribit timestamps can be: integer ms, float ms, or integer seconds.
            // Try "date" first, then "timestamp" as fallback field names.
            let parse_ts_value = |v: &Value| -> i64 {
                let raw: i64 = v.as_i64()
                    .or_else(|| v.as_f64().map(|f| f as i64))
                    .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
                    .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()).map(|f| f as i64))
                    .unwrap_or(0);
                // If the value looks like Unix seconds (10 digits ≤ 9999999999)
                // multiply by 1000 to get milliseconds.
                if raw > 0 && raw < 10_000_000_000 { raw * 1000 } else { raw }
            };
            let ts = {
                let v = &e["date"];
                let candidate = parse_ts_value(v);
                if candidate != 0 { candidate } else { parse_ts_value(&e["timestamp"]) }
            };
            eprintln!("[deribit] date={} timestamp={} -> ts={}", e["date"], e["timestamp"], ts);

            let id_val = {
                let v = &e["id"];
                v.as_i64().map(|i| i.to_string())
                    .or_else(|| v.as_f64().map(|f| (f as i64).to_string()))
                    .or_else(|| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default()
            };
            let type_raw = e["type"].as_str().unwrap_or("other");
            let tx_type = type_raw
                .replace("transfer_from", "transfer_in")
                .replace("transfer_to", "transfer_out");
            // NOTE: Deribit "profit_as_cashflow" is a boolean flag.
            // The actual realised PnL cash amount is in the "cashflow" field.
            // "equity" is provided directly by Deribit in this endpoint.
            logs.push(TransactionLog {
                id:                 id_val,
                timestamp:          ts,
                instrument_name:    e["instrument_name"].as_str().unwrap_or("").to_string(),
                transaction_type:   tx_type,
                side:               e["side"].as_str().unwrap_or("").to_string(),
                amount:             f64_val(&e["amount"]),
                price:              f64_val(&e["price"]),
                fee:                f64_val(&e["fees"]),   // Deribit uses "fees" (plural)
                fee_currency:       currency.to_string(),
                currency:           currency.to_string(),
                profit_as_cashflow: f64_val(&e["cashflow"]), // the actual PnL float
                balance:            f64_val(&e["balance"]),
                change:             f64_val(&e["change"]),
                trade_id:           e["trade_id"].as_str().unwrap_or("").to_string(),
                order_id:           e["order_id"].as_str().unwrap_or("").to_string(),
                info,
                mark_price:         f64_val(&e["mark_price"]),
                index_price:        f64_val(&e["index_price"]),
                equity:             f64_val(&e["equity"]),  // Deribit provides equity directly
                position:           f64_val(&e["position"]),
                base_currency:      e["base_currency"].as_str().unwrap_or("BTC").to_string(),
                quote_currency:     e["quote_currency"].as_str().unwrap_or("USD").to_string(),
            });
        }

        // Deribit returns a continuation token when more pages exist
        match result.get("continuation") {
            Some(c) if !c.is_null() => {
                continuation = c.as_i64();
                if continuation.is_none() { break; }
            }
            _ => break,
        }
    }

    logs.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(logs)
}
