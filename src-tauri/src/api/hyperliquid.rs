/// Hyperliquid REST API client
///
/// Hyperliquid is a decentralized perpetuals exchange.
/// - Public read: POST https://api.hyperliquid.xyz/info
/// - Trading:     POST https://api.hyperliquid.xyz/exchange  (EIP-712 signed)
/// - Testnet:     https://api.hyperliquid-testnet.xyz

use std::time::{SystemTime, UNIX_EPOCH};
use reqwest::Client;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use k256::ecdsa::{SigningKey, signature::hazmat::PrehashSigner};
use hex;

use crate::api::models::{
    Account, AccountSummary, Instrument, Order, OrderResult, OrderbookLevel, OrderbookSnapshot,
    PlaceOrderRequest, Position, Ticker, TickerStats, Trade,
};

fn base_url(testnet: bool) -> &'static str {
    if testnet { "https://api.hyperliquid-testnet.xyz" }
    else       { "https://api.hyperliquid.xyz" }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

// ── EIP-712 style signing ──────────────────────────────────────────────────

/// Decode hex private key and return a SigningKey
fn signing_key(private_key_hex: &str) -> Result<SigningKey, String> {
    let hex_clean = private_key_hex.trim_start_matches("0x");
    let bytes = hex::decode(hex_clean).map_err(|e| format!("Invalid private key hex: {}", e))?;
    SigningKey::from_slice(&bytes).map_err(|e| format!("Invalid private key: {}", e))
}

/// Sign an action for Hyperliquid exchange API.
/// Returns (signature_hex_r, signature_hex_s, v)
fn sign_action(private_key_hex: &str, action: &Value, nonce: u64, vault_address: Option<&str>) -> Result<Value, String> {
    let signing = signing_key(private_key_hex)?;

    // Build connection ID for signing
    let connection_id = if let Some(vault) = vault_address {
        // Hash vault address into connection
        let mut h = Keccak256::new();
        h.update(vault.as_bytes());
        hex::encode(h.finalize())
    } else {
        "0x0000000000000000000000000000000000000000".to_string()
    };

    // Compute action hash: keccak256(abi.encode(action_bytes, nonce, vault_connection))
    let action_str = serde_json::to_string(action).map_err(|e| e.to_string())?;
    let mut hasher = Keccak256::new();
    hasher.update(b"\x19Hyperliquid signed action:\n");
    hasher.update(action_str.as_bytes());
    hasher.update(&nonce.to_be_bytes());
    hasher.update(connection_id.as_bytes());
    let hash: [u8; 32] = hasher.finalize().into();

    let (sig, recid) = signing.sign_prehash(&hash)
        .map_err(|e| format!("Signing failed: {}", e))?;
    let sig_bytes = sig.to_bytes();
    let r = hex::encode(&sig_bytes[..32]);
    let s = hex::encode(&sig_bytes[32..]);
    let v = recid.to_byte() as u64 + 27;

    Ok(json!({ "r": format!("0x{}", r), "s": format!("0x{}", s), "v": v }))
}

// ── Info queries (public) ───────────────────────────────────────────────────

async fn info_query(client: &Client, testnet: bool, body: Value) -> Result<Value, String> {
    let url = format!("{}/info", base_url(testnet));
    let resp = client.post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send().await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}: {}", resp.status(), resp.text().await.unwrap_or_default()));
    }
    resp.json::<Value>().await.map_err(|e| e.to_string())
}

async fn exchange_action(client: &Client, testnet: bool, private_key: &str, action: Value) -> Result<Value, String> {
    let nonce = now_ms();
    let signature = sign_action(private_key, &action, nonce, None)?;
    let payload = json!({ "action": action, "nonce": nonce, "signature": signature });
    let url = format!("{}/exchange", base_url(testnet));
    let resp = client.post(&url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send().await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}: {}", resp.status(), resp.text().await.unwrap_or_default()));
    }
    resp.json::<Value>().await.map_err(|e| e.to_string())
}

// ── Asset index lookup ─────────────────────────────────────────────────────

async fn get_asset_index(client: &Client, testnet: bool, symbol: &str) -> Result<u32, String> {
    let meta = info_query(client, testnet, json!({"type": "meta"})).await?;
    let universe = meta["universe"].as_array()
        .ok_or("No universe in meta")?;
    for (i, asset) in universe.iter().enumerate() {
        if asset["name"].as_str() == Some(symbol.trim_end_matches("-PERP")) {
            return Ok(i as u32);
        }
    }
    Err(format!("Asset '{}' not found", symbol))
}

// ── Public API ──────────────────────────────────────────────────────────────

pub async fn fetch_instruments(_currency: &str, _kind: &str) -> Result<Vec<Instrument>, String> {
    let client = Client::new();
    let meta = info_query(&client, false, json!({"type": "meta"})).await?;
    let universe = meta["universe"].as_array()
        .ok_or("No universe in meta response")?;

    Ok(universe.iter().enumerate().map(|(i, asset)| {
        let name = asset["name"].as_str().unwrap_or("UNKNOWN");
        Instrument {
            instrument_name: format!("{}-PERP", name),
            kind:            "perpetual".to_string(),
            base_currency:   name.to_string(),
            quote_currency:  "USD".to_string(),
            settlement_currency: "USDC".to_string(),
            is_active:       true,
            tick_size:       asset["szDecimals"].as_f64().map(|d| 10f64.powi(-(d as i32))).unwrap_or(0.001),
            min_trade_amount: 0.001,
            qty_step:        None,
            contract_size:   Some(1.0),
            option_type:     None,
            strike:          None,
            expiration_timestamp: None,
        }
    }).collect())
}

pub async fn fetch_ticker(instrument_name: &str) -> Result<Ticker, String> {
    let client = Client::new();
    let coin = instrument_name.trim_end_matches("-PERP");
    let resp = info_query(&client, false, json!({
        "type": "l2Book",
        "coin": coin,
        "nSigFigs": 5,
    })).await?;

    let levels = &resp["levels"];
    let bids = levels[0].as_array();
    let asks = levels[1].as_array();
    let best_bid = bids.and_then(|b| b.first()).and_then(|l| l["px"].as_str()).and_then(|s| s.parse().ok());
    let best_ask = asks.and_then(|a| a.first()).and_then(|l| l["px"].as_str()).and_then(|s| s.parse().ok());

    // Get mid/mark from allMids
    let mids = info_query(&client, false, json!({"type": "allMids"})).await.unwrap_or_default();
    let mark = mids[coin].as_str().and_then(|s| s.parse::<f64>().ok());

    Ok(Ticker {
        instrument_name: instrument_name.to_string(),
        best_bid_price:   best_bid,
        best_ask_price:   best_ask,
        best_bid_amount:  bids.and_then(|b| b.first()).and_then(|l| l["sz"].as_str()).and_then(|s| s.parse().ok()),
        best_ask_amount:  asks.and_then(|a| a.first()).and_then(|l| l["sz"].as_str()).and_then(|s| s.parse().ok()),
        last_price:       mark,
        mark_price:       mark,
        index_price:      mark,
        underlying_price: None,
        open_interest:    None,
        stats:            TickerStats { high: None, low: None, price_change: None, volume: None, volume_usd: None },
        mark_iv: None, bid_iv: None, ask_iv: None,
        delta: None, gamma: None, vega: None, theta: None,
    })
}

pub async fn place_order(req: &PlaceOrderRequest, account: &Account) -> Result<OrderResult, String> {
    let client = Client::new();
    let asset = get_asset_index(&client, account.testnet, &req.instrument_name).await?;
    let is_buy = req.side == "buy";
    let price = req.price.unwrap_or(0.0);
    let tif = match req.time_in_force.as_deref().unwrap_or("good_til_cancelled") {
        "fill_or_kill"         => "Ioc",
        "immediate_or_cancel"  => "Ioc",
        _                      => "Gtc",
    };

    let order_type = if req.order_type == "market" {
        json!({ "limit": { "tif": "Ioc" } })
    } else {
        json!({ "limit": { "tif": tif } })
    };

    // Use market price for market orders
    let px = if req.order_type == "market" {
        if is_buy { price * 1.05 } else { price * 0.95 }
    } else { price };

    let action = json!({
        "type": "order",
        "orders": [{
            "a": asset,
            "b": is_buy,
            "p": format!("{}", px),
            "s": format!("{}", req.amount),
            "r": false,
            "t": order_type,
            "c": req.client_order_id.as_deref().unwrap_or(""),
        }],
        "grouping": "na",
    });

    let resp = exchange_action(&client, account.testnet, &account.api_secret, action).await?;
    let status = &resp["status"];
    if status.as_str() == Some("ok") {
        let oid = resp["response"]["data"]["statuses"][0]["resting"]["oid"]
            .as_i64()
            .or_else(|| resp["response"]["data"]["statuses"][0]["filled"]["oid"].as_i64())
            .unwrap_or(0);
        Ok(OrderResult {
            success: true,
            order: Some(Order {
                order_id:            oid.to_string(),
                instrument_name:     req.instrument_name.clone(),
                direction:           req.side.clone(),
                order_type:          req.order_type.clone(),
                order_state:         "open".to_string(),
                price:               Some(px),
                amount:              req.amount,
                filled_amount:       0.0,
                average_price:       None,
                post_only:           req.post_only.unwrap_or(false),
                time_in_force:       req.time_in_force.clone().unwrap_or_else(|| "good_til_cancelled".to_string()),
                creation_timestamp:  now_ms() as i64,
                last_update_timestamp: now_ms() as i64,
            }),
            error: None,
        })
    } else {
        let err = resp["response"].as_str()
            .or_else(|| resp["response"]["data"]["statuses"][0]["error"].as_str())
            .unwrap_or("Unknown error").to_string();
        Ok(OrderResult { success: false, order: None, error: Some(err) })
    }
}

pub async fn cancel_order(order_id: &str, instrument_name: Option<&str>, account: &Account) -> Result<bool, String> {
    let client = Client::new();
    let coin = instrument_name.unwrap_or("").trim_end_matches("-PERP");
    let asset = if coin.is_empty() { 0 } else {
        get_asset_index(&client, account.testnet, instrument_name.unwrap_or("")).await.unwrap_or(0)
    };
    let oid: i64 = order_id.parse().map_err(|_| "Invalid order id".to_string())?;
    let action = json!({
        "type": "cancel",
        "cancels": [{ "a": asset, "o": oid }],
    });
    let resp = exchange_action(&client, account.testnet, &account.api_secret, action).await?;
    Ok(resp["status"].as_str() == Some("ok"))
}

pub async fn get_open_orders(instrument_name: &str, account: &Account) -> Result<Vec<Order>, String> {
    let client = Client::new();
    let resp = info_query(&client, account.testnet, json!({
        "type": "openOrders",
        "user": account.api_key,
    })).await?;

    let orders = resp.as_array().ok_or("Expected array")?;
    let coin_filter = instrument_name.trim_end_matches("-PERP");

    Ok(orders.iter().filter_map(|o| {
        let coin = o["coin"].as_str()?;
        if !instrument_name.is_empty() && coin != coin_filter { return None; }
        let side = o["side"].as_str()?;
        let oid = o["oid"].as_i64()?.to_string();
        Some(Order {
            order_id:            oid,
            instrument_name:     format!("{}-PERP", coin),
            direction:           if side == "B" { "buy" } else { "sell" }.to_string(),
            order_type:          "limit".to_string(),
            order_state:         "open".to_string(),
            price:               o["limitPx"].as_str().and_then(|s| s.parse().ok()),
            amount:              o["sz"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
            filled_amount:       0.0,
            average_price:       None,
            post_only:           false,
            time_in_force:       o["tif"].as_str().unwrap_or("Gtc").to_string(),
            creation_timestamp:  o["timestamp"].as_i64().unwrap_or(0),
            last_update_timestamp: o["timestamp"].as_i64().unwrap_or(0),
        })
    }).collect())
}

pub async fn get_all_open_orders(account: &Account) -> Result<Vec<Order>, String> {
    get_open_orders("", account).await
}

pub async fn get_account_summary(_currency: &str, account: &Account) -> Result<AccountSummary, String> {
    let client = Client::new();
    let resp = info_query(&client, account.testnet, json!({
        "type": "clearinghouseState",
        "user": account.api_key,
    })).await?;

    let margin = &resp["marginSummary"];
    let equity:     f64 = margin["accountValue"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let used_margin: f64 = margin["totalMarginUsed"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let unrealized: f64 = margin["totalUnrealizedPnl"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);

    Ok(AccountSummary {
        currency:           "USDC".to_string(),
        equity,
        available_funds:    equity - used_margin,
        initial_margin:     used_margin,
        maintenance_margin: used_margin * 0.5,
        unrealized_pl:      unrealized,
    })
}

pub async fn get_trade_history(account: &Account, start_ms: i64, _end_ms: i64) -> Result<Vec<Trade>, String> {
    let client = Client::new();
    let resp = info_query(&client, account.testnet, json!({
        "type": "userFills",
        "user": account.api_key,
        "startTime": start_ms,
    })).await?;

    let fills = resp.as_array().ok_or("Expected array")?;
    Ok(fills.iter().filter_map(|f| {
        let coin = f["coin"].as_str()?;
        let side = f["side"].as_str()?;
        let tid = f["tid"].as_i64()?.to_string();
        Some(Trade {
            trade_id:        tid.clone(),
            account_id:      account.id.clone(),
            account_name:    account.name.clone(),
            exchange:        "hyperliquid".to_string(),
            instrument_name: format!("{}-PERP", coin),
            direction:       if side == "B" { "buy" } else { "sell" }.to_string(),
            amount:          f["sz"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
            price:           f["px"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
            fee:             f["fee"].as_str().and_then(|s| s.parse::<f64>().ok()).map(|v| v.abs()).unwrap_or(0.0),
            fee_currency:    "USDC".to_string(),
            timestamp:       f["time"].as_i64().unwrap_or(0),
            order_id:        f["oid"].as_i64().map(|v| v.to_string()).unwrap_or_default(),
        })
    }).collect())
}

pub async fn get_positions(_currency: &str, account: &Account) -> Result<Vec<Position>, String> {
    let client = Client::new();
    let resp = info_query(&client, account.testnet, json!({
        "type": "clearinghouseState",
        "user": account.api_key,
    })).await?;

    let positions = resp["assetPositions"].as_array().ok_or("No assetPositions")?;
    Ok(positions.iter().filter_map(|ap| {
        let pos = &ap["position"];
        let coin = pos["coin"].as_str()?;
        let szi: f64 = pos["szi"].as_str().and_then(|s| s.parse().ok())?;
        if szi == 0.0 { return None; }
        let entry: f64 = pos["entryPx"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let unrealized: f64 = pos["unrealizedPnl"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        Some(Position {
            instrument_name: format!("{}-PERP", coin),
            direction:       if szi > 0.0 { "long" } else { "short" }.to_string(),
            size:            szi.abs(),
            average_price:   entry,
            mark_price:      entry,
            mark_iv:         0.0,
            unrealized_pnl:  unrealized,
            delta:           szi,
            gamma:           0.0,
            theta:           0.0,
            vega:            0.0,
        })
    }).collect())
}

pub async fn fetch_orderbook(instrument_name: &str, depth: u32) -> Result<OrderbookSnapshot, String> {
    let client = Client::new();
    let coin = instrument_name.trim_end_matches("-PERP");

    let resp = info_query(&client, false, json!({
        "type": "l2Book",
        "coin": coin,
        "nSigFigs": 5,
    })).await?;

    let ts = now_ms() as i64;

    let parse_levels = |arr: Option<&Vec<Value>>, lim: usize| -> Vec<OrderbookLevel> {
        arr.map(|levels| {
            levels.iter().take(lim).filter_map(|item| {
                Some(OrderbookLevel {
                    price: item["px"].as_str()?.parse().ok()?,
                    size:  item["sz"].as_str()?.parse().ok()?,
                })
            }).collect()
        }).unwrap_or_default()
    };

    let lim = depth as usize;
    // levels[0] = bids (descending), levels[1] = asks (ascending)
    let bids = parse_levels(resp["levels"][0].as_array(), lim);
    let asks = parse_levels(resp["levels"][1].as_array(), lim);

    Ok(OrderbookSnapshot {
        instrument_name: instrument_name.to_string(),
        bids,
        asks,
        timestamp: ts,
    })
}
