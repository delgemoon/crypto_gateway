/// Uniswap V3 trading client via JSON-RPC
///
/// Connects to a user-configured RPC endpoint (Infura, Alchemy, etc.)
/// Uses Uniswap V3 contracts:
///   - Quoter V2:  0x61fFE014bA17989E743c5F6cB21bF9697530B21e (all chains)
///   - Router V2:  0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45 (all chains)
///   - Factory:    0x1F98431c8aD98523631AE4a59f267346ea31F984
///
/// Chains supported (chain_id):
///   1  = Ethereum mainnet
///   42161 = Arbitrum One
///   8453  = Base
///   10    = Optimism
///   137   = Polygon

use std::collections::HashMap;
use std::sync::OnceLock;
use reqwest::Client;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use k256::ecdsa::{SigningKey, signature::hazmat::PrehashSigner};
use k256::elliptic_curve::sec1::ToEncodedPoint;
use std::time::{SystemTime, UNIX_EPOCH};
use hex;

use crate::api::models::{
    Account, AccountSummary, Instrument, Order, OrderResult, OrderbookSnapshot,
    PlaceOrderRequest, Position, Ticker, TickerStats, Trade,
};

// ── Known token registry ────────────────────────────────────────────────────

#[derive(Clone)]
struct TokenInfo {
    address: &'static str,
    symbol:  &'static str,
    decimals: u8,
}

fn token_registry() -> &'static HashMap<&'static str, TokenInfo> {
    static REG: OnceLock<HashMap<&'static str, TokenInfo>> = OnceLock::new();
    REG.get_or_init(|| {
        let mut m = HashMap::new();
        // Ethereum mainnet + Arbitrum (most liquid pairs)
        m.insert("WETH",  TokenInfo { address: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", symbol: "WETH",  decimals: 18 });
        m.insert("USDC",  TokenInfo { address: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", symbol: "USDC",  decimals: 6  });
        m.insert("USDT",  TokenInfo { address: "0xdAC17F958D2ee523a2206206994597C13D831ec7", symbol: "USDT",  decimals: 6  });
        m.insert("WBTC",  TokenInfo { address: "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599", symbol: "WBTC",  decimals: 8  });
        m.insert("DAI",   TokenInfo { address: "0x6B175474E89094C44Da98b954EedeAC495271d0F", symbol: "DAI",   decimals: 18 });
        m.insert("UNI",   TokenInfo { address: "0x1f9840a85d5aF5bf1D1762F925BDADdC4201F984", symbol: "UNI",   decimals: 18 });
        m.insert("AAVE",  TokenInfo { address: "0x7Fc66500c84A76Ad7e9c93437bFc5Ac33E2DDaE9", symbol: "AAVE",  decimals: 18 });
        m
    })
}

const QUOTER_V2:  &str = "0x61fFE014bA17989E743c5F6cB21bF9697530B21e";
const ROUTER_V2:  &str = "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45";

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

// ── EVM signing utilities ───────────────────────────────────────────────────

fn signing_key(pk_hex: &str) -> Result<SigningKey, String> {
    let clean = pk_hex.trim_start_matches("0x");
    let bytes = hex::decode(clean).map_err(|e| format!("Bad private key: {}", e))?;
    SigningKey::from_slice(&bytes).map_err(|e| format!("Bad key: {}", e))
}

fn wallet_address(private_key_hex: &str) -> Result<String, String> {
    let key = signing_key(private_key_hex)?;
    let vk = key.verifying_key();
    let point = vk.to_encoded_point(false);
    let pubkey_bytes = &point.as_bytes()[1..]; // drop 0x04 prefix
    let mut hasher = Keccak256::new();
    hasher.update(pubkey_bytes);
    let hash = hasher.finalize();
    Ok(format!("0x{}", hex::encode(&hash[12..])))
}

fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(data);
    h.finalize().into()
}

/// Sign an EIP-1559 transaction
fn sign_tx(
    to: &str, data: &[u8], value: u64,
    gas_limit: u64, max_fee: u64, max_tip: u64,
    nonce: u64, chain_id: u64,
    private_key_hex: &str,
) -> Result<String, String> {
    // EIP-2718 type 0x02 transaction RLP encoding
    // rlp([chain_id, nonce, max_priority_fee, max_fee, gas_limit, to, value, data, access_list])
    let chain_id_bytes = u64_to_rlp_bytes(chain_id);
    let nonce_bytes    = u64_to_rlp_bytes(nonce);
    let max_tip_bytes  = u64_to_rlp_bytes(max_tip);
    let max_fee_bytes  = u64_to_rlp_bytes(max_fee);
    let gas_bytes      = u64_to_rlp_bytes(gas_limit);
    let to_bytes       = hex::decode(to.trim_start_matches("0x")).map_err(|e| e.to_string())?;
    let value_bytes    = u64_to_rlp_bytes(value);

    let payload = encode_list(&[
        &rlp_encode_bytes(&chain_id_bytes),
        &rlp_encode_bytes(&nonce_bytes),
        &rlp_encode_bytes(&max_tip_bytes),
        &rlp_encode_bytes(&max_fee_bytes),
        &rlp_encode_bytes(&gas_bytes),
        &rlp_encode_bytes(&to_bytes),
        &rlp_encode_bytes(&value_bytes),
        &rlp_encode_bytes(data),
        &rlp_encode_bytes(&[]), // empty access list
    ]);

    let signing_hash = {
        let mut pre = vec![0x02u8];
        pre.extend_from_slice(&payload);
        keccak256(&pre)
    };

    let key = signing_key(private_key_hex)?;
    let (sig, recid) = key.sign_prehash(&signing_hash)
        .map_err(|e| format!("Sign error: {}", e))?;
    let sig_bytes = sig.to_bytes();
    let v = recid.to_byte() as u64;

    // Build signed tx: 0x02 || rlp([...fields..., v, r, s])
    let v_bytes  = u64_to_rlp_bytes(v);
    let r_bytes  = sig_bytes[..32].to_vec();
    let s_bytes  = sig_bytes[32..].to_vec();

    let signed = encode_list(&[
        &rlp_encode_bytes(&chain_id_bytes),
        &rlp_encode_bytes(&nonce_bytes),
        &rlp_encode_bytes(&max_tip_bytes),
        &rlp_encode_bytes(&max_fee_bytes),
        &rlp_encode_bytes(&gas_bytes),
        &rlp_encode_bytes(&to_bytes),
        &rlp_encode_bytes(&value_bytes),
        &rlp_encode_bytes(data),
        &rlp_encode_bytes(&[]),
        &rlp_encode_bytes(&v_bytes),
        &rlp_encode_bytes(&r_bytes),
        &rlp_encode_bytes(&s_bytes),
    ]);

    let mut raw = vec![0x02u8];
    raw.extend_from_slice(&signed);
    Ok(format!("0x{}", hex::encode(raw)))
}

// Minimal RLP helpers
fn u64_to_rlp_bytes(n: u64) -> Vec<u8> {
    if n == 0 { return vec![]; }
    let bytes = n.to_be_bytes();
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(7);
    bytes[start..].to_vec()
}

fn rlp_encode_bytes(data: &[u8]) -> Vec<u8> {
    if data.is_empty() { return vec![0x80]; }
    if data.len() == 1 && data[0] < 0x80 { return data.to_vec(); }
    let mut out = rlp_length_prefix(0x80, data.len());
    out.extend_from_slice(data);
    out
}

fn rlp_length_prefix(offset: u8, len: usize) -> Vec<u8> {
    if len <= 55 {
        vec![offset + len as u8]
    } else {
        let len_bytes = usize_to_bytes(len);
        let mut out = vec![offset + 55 + len_bytes.len() as u8];
        out.extend_from_slice(&len_bytes);
        out
    }
}

fn usize_to_bytes(n: usize) -> Vec<u8> {
    let bytes = n.to_be_bytes();
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(7);
    bytes[start..].to_vec()
}

fn encode_list(items: &[&[u8]]) -> Vec<u8> {
    let content: Vec<u8> = items.iter().flat_map(|i| i.iter().copied()).collect();
    let mut out = rlp_length_prefix(0xc0, content.len());
    out.extend_from_slice(&content);
    out
}

// ── ABI encoding helpers ────────────────────────────────────────────────────

fn abi_encode_address(addr: &str) -> Vec<u8> {
    let clean = addr.trim_start_matches("0x");
    let bytes = hex::decode(clean).unwrap_or_default();
    let mut out = vec![0u8; 32];
    let start = 32 - bytes.len();
    out[start..].copy_from_slice(&bytes);
    out
}

fn abi_encode_u256(n: u128) -> Vec<u8> {
    let mut out = vec![0u8; 32];
    let bytes = n.to_be_bytes();
    out[16..].copy_from_slice(&bytes);
    out
}

fn abi_encode_u24(n: u32) -> Vec<u8> {
    let mut out = vec![0u8; 32];
    out[29..].copy_from_slice(&n.to_be_bytes()[1..]);
    out
}

// ── RPC helpers ────────────────────────────────────────────────────────────

async fn rpc_call(client: &Client, rpc_url: &str, method: &str, params: Value) -> Result<Value, String> {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    let resp = client.post(rpc_url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send().await
        .map_err(|e| e.to_string())?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    if let Some(err) = v.get("error") {
        return Err(format!("RPC error: {}", err));
    }
    Ok(v["result"].clone())
}

async fn eth_call(client: &Client, rpc_url: &str, to: &str, data: &str) -> Result<String, String> {
    let result = rpc_call(client, rpc_url, "eth_call", json!([
        { "to": to, "data": data },
        "latest"
    ])).await?;
    Ok(result.as_str().unwrap_or("0x").to_string())
}

async fn eth_get_nonce(client: &Client, rpc_url: &str, address: &str) -> Result<u64, String> {
    let result = rpc_call(client, rpc_url, "eth_getTransactionCount", json!([address, "pending"])).await?;
    let hex_str = result.as_str().unwrap_or("0x0");
    u64::from_str_radix(hex_str.trim_start_matches("0x"), 16).map_err(|e| e.to_string())
}

async fn eth_gas_price(client: &Client, rpc_url: &str) -> Result<(u64, u64), String> {
    let fee_data = rpc_call(client, rpc_url, "eth_feeHistory", json!([1, "latest", [50]])).await?;
    let base = fee_data["baseFeePerGas"].as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(20_000_000_000); // 20 gwei fallback
    let tip = 1_000_000_000u64; // 1 gwei tip
    let max_fee = base * 2 + tip;
    Ok((max_fee, tip))
}

async fn eth_send_raw(client: &Client, rpc_url: &str, raw_tx: &str) -> Result<String, String> {
    let result = rpc_call(client, rpc_url, "eth_sendRawTransaction", json!([raw_tx])).await?;
    Ok(result.as_str().unwrap_or("").to_string())
}

// ── Token/pair parsing ─────────────────────────────────────────────────────

/// Parse instrument name like "WETH/USDC" → (token_in, token_out)
fn parse_pair(instrument_name: &str) -> Option<(TokenInfo, TokenInfo)> {
    let reg = token_registry();
    let parts: Vec<&str> = instrument_name.split('/').collect();
    if parts.len() != 2 { return None; }
    let a = reg.get(parts[0].trim_end_matches("-SPOT"))?;
    let b = reg.get(parts[1].trim_end_matches("-SPOT"))?;
    Some((a.clone(), b.clone()))
}

// ── Public API ──────────────────────────────────────────────────────────────

pub async fn fetch_instruments(_currency: &str, _kind: &str) -> Result<Vec<crate::api::models::Instrument>, String> {
    let reg = token_registry();
    let tokens: Vec<&str> = reg.keys().cloned().collect();
    let mut instruments = Vec::new();
    let quote_tokens = ["USDC", "USDT", "DAI"];

    for &base in &tokens {
        for &quote in &quote_tokens {
            if base == quote { continue; }
            if reg.contains_key(base) && reg.contains_key(quote) {
                instruments.push(Instrument {
                    instrument_name: format!("{}/{}", base, quote),
                    kind:            "spot".to_string(),
                    base_currency:   base.to_string(),
                    quote_currency:  quote.to_string(),
                    settlement_currency: quote.to_string(),
                    is_active:       true,
                    tick_size:       0.01,
                    min_trade_amount: 0.001,
                    qty_step:        None,
                    contract_size:   None,
                    option_type:     None,
                    strike:          None,
                    expiration_timestamp: None,
                });
            }
        }
    }
    Ok(instruments)
}

pub async fn fetch_ticker(instrument_name: &str) -> Result<Ticker, String> {
    // Without a configured account we can't do on-chain calls
    // Return a placeholder that indicates quote-on-demand
    Ok(Ticker {
        instrument_name: instrument_name.to_string(),
        best_bid_price: None, best_ask_price: None,
        best_bid_amount: None, best_ask_amount: None,
        last_price: None, mark_price: None, index_price: None,
        open_interest: None,
        stats: TickerStats { high: None, low: None, price_change: None, volume: None, volume_usd: None },
        mark_iv: None, bid_iv: None, ask_iv: None,
        delta: None, gamma: None, vega: None, theta: None,
    })
}

pub async fn fetch_ticker_with_account(instrument_name: &str, account: &Account) -> Result<Ticker, String> {
    let rpc_url = account.rpc_url.as_deref().ok_or("No RPC URL configured for Uniswap account")?;
    let (token_in, token_out) = parse_pair(instrument_name).ok_or(format!("Unknown pair: {}", instrument_name))?;
    let client = Client::new();

    // Call quoteExactInputSingle on Quoter V2
    // function quoteExactInputSingle((address,address,uint256,uint24,uint160)) returns (uint256,uint160,uint32,uint256)
    let fn_selector = &keccak256(b"quoteExactInputSingle((address,address,uint256,uint24,uint160))")[..4];
    let amount_in = 10u128.pow(token_in.decimals as u32); // 1 unit

    let mut calldata = fn_selector.to_vec();
    // Encode struct as tuple
    calldata.extend_from_slice(&abi_encode_address(token_in.address));
    calldata.extend_from_slice(&abi_encode_address(token_out.address));
    calldata.extend_from_slice(&abi_encode_u256(amount_in));
    calldata.extend_from_slice(&abi_encode_u24(3000)); // 0.3% fee tier
    calldata.extend_from_slice(&abi_encode_u256(0)); // sqrtPriceLimitX96 = 0

    let data_hex = format!("0x{}", hex::encode(&calldata));
    let result = eth_call(&client, rpc_url, QUOTER_V2, &data_hex).await?;
    let result_bytes = hex::decode(result.trim_start_matches("0x")).map_err(|e| e.to_string())?;
    if result_bytes.len() < 32 { return Err("Invalid quoter response".to_string()); }
    let amount_out_bytes: [u8; 16] = result_bytes[16..32].try_into().map_err(|_| "slice error")?;
    let amount_out = u128::from_be_bytes(amount_out_bytes);
    let price = amount_out as f64 / 10f64.powi(token_out.decimals as i32);

    Ok(Ticker {
        instrument_name: instrument_name.to_string(),
        best_bid_price: Some(price), best_ask_price: Some(price),
        best_bid_amount: None, best_ask_amount: None,
        last_price: Some(price), mark_price: Some(price), index_price: Some(price),
        open_interest: None,
        stats: TickerStats { high: None, low: None, price_change: None, volume: None, volume_usd: None },
        mark_iv: None, bid_iv: None, ask_iv: None,
        delta: None, gamma: None, vega: None, theta: None,
    })
}

pub async fn place_order(req: &PlaceOrderRequest, account: &Account) -> Result<OrderResult, String> {
    let rpc_url = account.rpc_url.as_deref().ok_or("No RPC URL configured for Uniswap account")?;
    let chain_id = account.chain_id.unwrap_or(1);
    let (token_in, token_out) = parse_pair(&req.instrument_name).ok_or(format!("Unknown pair: {}", req.instrument_name))?;
    let client = Client::new();

    // Determine token direction based on side
    let (from_token, to_token) = if req.side == "buy" {
        (&token_out, &token_in)  // buying base = selling quote
    } else {
        (&token_in, &token_out)
    };

    let from_addr = wallet_address(&account.api_secret)?;
    let nonce = eth_get_nonce(&client, rpc_url, &from_addr).await?;
    let (max_fee, max_tip) = eth_gas_price(&client, rpc_url).await?;

    let amount_in_raw = (req.amount * 10f64.powi(from_token.decimals as i32)) as u128;
    let deadline = now_ms() / 1000 + 1800; // 30 min

    // Build exactInputSingle calldata
    // function exactInputSingle((address,address,uint24,address,uint256,uint256,uint160))
    let fn_selector = &keccak256(b"exactInputSingle((address,address,uint24,address,uint256,uint256,uint160))")[..4];
    let mut calldata = fn_selector.to_vec();
    calldata.extend_from_slice(&abi_encode_address(from_token.address));
    calldata.extend_from_slice(&abi_encode_address(to_token.address));
    calldata.extend_from_slice(&abi_encode_u24(3000));
    calldata.extend_from_slice(&abi_encode_address(&from_addr));
    calldata.extend_from_slice(&abi_encode_u256(deadline as u128));
    calldata.extend_from_slice(&abi_encode_u256(amount_in_raw));
    // amountOutMinimum = 0 (no slippage protection - could be configurable)
    calldata.extend_from_slice(&abi_encode_u256(0));
    calldata.extend_from_slice(&abi_encode_u256(0)); // sqrtPriceLimitX96

    let raw_tx = sign_tx(
        ROUTER_V2,
        &calldata,
        0, // value = 0 for ERC-20 swaps
        300_000, // gas limit
        max_fee,
        max_tip,
        nonce,
        chain_id,
        &account.api_secret,
    )?;

    let tx_hash = eth_send_raw(&client, rpc_url, &raw_tx).await?;
    let order_id = tx_hash.clone();

    Ok(OrderResult {
        success: true,
        order: Some(Order {
            order_id:            order_id.clone(),
            instrument_name:     req.instrument_name.clone(),
            direction:           req.side.clone(),
            order_type:          "market".to_string(),
            order_state:         "filled".to_string(),
            price:               req.price,
            amount:              req.amount,
            filled_amount:       req.amount,
            average_price:       req.price,
            post_only:           false,
            time_in_force:       "immediate_or_cancel".to_string(),
            creation_timestamp:  now_ms() as i64,
            last_update_timestamp: now_ms() as i64,
        }),
        error: None,
    })
}

pub async fn cancel_order(_order_id: &str, _instrument_name: Option<&str>, _account: &Account) -> Result<bool, String> {
    Err("Uniswap swaps are atomic on-chain transactions and cannot be cancelled".to_string())
}

pub async fn get_open_orders(_instrument_name: &str, _account: &Account) -> Result<Vec<Order>, String> {
    // Uniswap has no concept of open orders (AMM swaps are atomic)
    Ok(vec![])
}

pub async fn get_all_open_orders(_account: &Account) -> Result<Vec<Order>, String> {
    Ok(vec![])
}

pub async fn get_account_summary(currency: &str, account: &Account) -> Result<AccountSummary, String> {
    let rpc_url = account.rpc_url.as_deref().ok_or("No RPC URL configured")?;
    let wallet = if account.api_key.starts_with("0x") {
        account.api_key.clone()
    } else {
        wallet_address(&account.api_secret)?
    };
    let client = Client::new();

    // Look up token balance
    let token_addr = token_registry().get(currency)
        .map(|t| t.address)
        .unwrap_or("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"); // default USDC

    // balanceOf(address) ABI: selector + padded address
    let selector = &keccak256(b"balanceOf(address)")[..4];
    let mut calldata = selector.to_vec();
    calldata.extend_from_slice(&abi_encode_address(&wallet));
    let data_hex = format!("0x{}", hex::encode(&calldata));
    let result = eth_call(&client, rpc_url, token_addr, &data_hex).await?;
    let result_bytes = hex::decode(result.trim_start_matches("0x")).unwrap_or_default();
    let balance = if result_bytes.len() >= 32 {
        let bytes: [u8; 16] = result_bytes[16..32].try_into().unwrap_or([0u8; 16]);
        u128::from_be_bytes(bytes) as f64 / 10f64.powi(6) // assume 6 decimals (USDC/USDT)
    } else { 0.0 };

    Ok(AccountSummary {
        currency:           currency.to_string(),
        equity:             balance,
        available_funds:    balance,
        initial_margin:     0.0,
        maintenance_margin: 0.0,
        unrealized_pl:      0.0,
    })
}

pub async fn get_trade_history(_account: &Account, _start_ms: i64, _end_ms: i64) -> Result<Vec<Trade>, String> {
    // On-chain trade history requires event log indexing (The Graph, etc.)
    // Return empty for now — can integrate a subgraph API in future
    Ok(vec![])
}

pub async fn get_positions(_currency: &str, _account: &Account) -> Result<Vec<Position>, String> {
    // Uniswap spot has no margin positions
    Ok(vec![])
}

pub async fn fetch_orderbook(instrument_name: &str, _depth: u32) -> Result<OrderbookSnapshot, String> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    Ok(OrderbookSnapshot {
        instrument_name: instrument_name.to_string(),
        bids: vec![],
        asks: vec![],
        timestamp: ts,
    })
}
