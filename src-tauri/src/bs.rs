/// Black-Scholes option pricing, greeks, and implied volatility.
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

// ── Normal distribution helpers ───────────────────────────────────────────

fn norm_pdf(x: f64) -> f64 {
    (-0.5 * x * x).exp() / (2.0 * PI).sqrt()
}

/// Cumulative normal distribution via Abramowitz & Stegun rational approximation.
pub fn norm_cdf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let y = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let e = y * (-x * x / 2.0).exp();
    if x >= 0.0 { 1.0 - e } else { e }
}

// ── Core BS formula ───────────────────────────────────────────────────────

/// Black-Scholes price.
/// - S: spot/index price
/// - K: strike
/// - T: time to expiry in years
/// - r: risk-free rate (decimal, e.g. 0.05 for 5%)
/// - sigma: implied vol (decimal, e.g. 0.80 for 80%)
/// - is_call: true = call, false = put
pub fn bs_price(s: f64, k: f64, t: f64, r: f64, sigma: f64, is_call: bool) -> f64 {
    if t <= 0.0 || sigma <= 0.0 || s <= 0.0 || k <= 0.0 {
        return if is_call { (s - k).max(0.0) } else { (k - s).max(0.0) };
    }
    let sqrt_t = t.sqrt();
    let d1 = ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * sqrt_t);
    let d2 = d1 - sigma * sqrt_t;
    if is_call {
        s * norm_cdf(d1) - k * (-r * t).exp() * norm_cdf(d2)
    } else {
        k * (-r * t).exp() * norm_cdf(-d2) - s * norm_cdf(-d1)
    }
}

// ── Greeks ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Greeks {
    pub delta: f64,
    pub gamma: f64,
    /// Per 1% move in vol (divided by 100)
    pub vega: f64,
    /// Per calendar day (divided by 365)
    pub theta: f64,
    pub rho: f64,
}

pub fn bs_greeks(s: f64, k: f64, t: f64, r: f64, sigma: f64, is_call: bool) -> Option<Greeks> {
    if t <= 0.0 || sigma <= 0.0 || s <= 0.0 || k <= 0.0 {
        return None;
    }
    let sqrt_t = t.sqrt();
    let d1 = ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * sqrt_t);
    let d2 = d1 - sigma * sqrt_t;
    let n1 = norm_pdf(d1);
    let disc = (-r * t).exp();

    let delta = if is_call { norm_cdf(d1) } else { norm_cdf(d1) - 1.0 };
    let gamma = n1 / (s * sigma * sqrt_t);
    let vega  = s * n1 * sqrt_t / 100.0;  // per 1% vol move
    let theta = if is_call {
        (-s * n1 * sigma / (2.0 * sqrt_t) - r * k * disc * norm_cdf(d2)) / 365.0
    } else {
        (-s * n1 * sigma / (2.0 * sqrt_t) + r * k * disc * norm_cdf(-d2)) / 365.0
    };
    let rho = if is_call {
        k * t * disc * norm_cdf(d2) / 100.0
    } else {
        -k * t * disc * norm_cdf(-d2) / 100.0
    };

    Some(Greeks { delta, gamma, vega, theta, rho })
}

// ── Implied Volatility (bisection) ────────────────────────────────────────

/// Bisection search for implied volatility, 120 iterations ≈ 10^-7 precision.
pub fn implied_vol(price: f64, s: f64, k: f64, t: f64, r: f64, is_call: bool) -> Option<f64> {
    if t <= 0.0 || s <= 0.0 || k <= 0.0 || price <= 0.0 {
        return None;
    }
    let intrinsic = if is_call {
        (s - k * (-r * t).exp()).max(0.0)
    } else {
        (k * (-r * t).exp() - s).max(0.0)
    };
    if price < intrinsic - 1e-8 {
        return None;
    }
    let mut lo = 1e-6_f64;
    let mut hi = 20.0_f64;
    for _ in 0..120 {
        let mid = (lo + hi) / 2.0;
        let p = bs_price(s, k, t, r, mid, is_call);
        if (p - price).abs() < 1e-7 {
            return Some(mid);
        }
        if p < price { lo = mid; } else { hi = mid; }
    }
    Some((lo + hi) / 2.0)
}

// ── Instrument parser ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedOption {
    pub underlying: String,
    /// Unix timestamp (seconds) of expiry (08:00 UTC)
    pub expiry_ts: i64,
    pub strike: f64,
    pub is_call: bool,
}

fn month_num(s: &str) -> Option<u32> {
    match s.to_ascii_uppercase().as_str() {
        "JAN" => Some(0),  "FEB" => Some(1),  "MAR" => Some(2),
        "APR" => Some(3),  "MAY" => Some(4),  "JUN" => Some(5),
        "JUL" => Some(6),  "AUG" => Some(7),  "SEP" => Some(8),
        "OCT" => Some(9),  "NOV" => Some(10), "DEC" => Some(11),
        _ => None,
    }
}

/// Accepts formats:
/// - Deribit/Bybit: `BTC-27JUN25-100000-C`
/// - CoInCall:      `BTCUSD-27JUN25-100000-C`
pub fn parse_option(instrument_name: &str) -> Option<ParsedOption> {
    let parts: Vec<&str> = instrument_name.split('-').collect();
    if parts.len() < 4 {
        return None;
    }
    let type_str = parts[parts.len() - 1].to_ascii_uppercase();
    if type_str != "C" && type_str != "P" {
        return None;
    }
    let strike: f64 = parts[parts.len() - 2].parse().ok()?;
    let date_str = parts[parts.len() - 3].to_ascii_uppercase();
    if date_str.len() < 7 {
        return None;
    }
    let day: u32   = date_str[..2].parse().ok()?;
    let month: u32 = month_num(&date_str[2..5])?;
    let year: i32  = 2000 + date_str[5..7].parse::<i32>().ok()?;

    // underlying = everything before the date part, stripped of trailing "USD"/"USDT"
    let underlying_raw = parts[..parts.len() - 3].join("-");
    let underlying = strip_quote_suffix(&underlying_raw);

    // expiry = 08:00 UTC on expiry date
    let expiry_ts = ymd_to_unix(year, month, day, 8, 0, 0)?;

    Some(ParsedOption {
        underlying,
        expiry_ts,
        strike,
        is_call: type_str == "C",
    })
}

/// Strip common quote suffixes to get bare coin name: BTCUSD → BTC, ETHUSDT → ETH.
fn strip_quote_suffix(raw: &str) -> String {
    let upper = raw.to_ascii_uppercase();
    for suffix in &["USDT", "USD", "BUSD", "USDC"] {
        if upper.ends_with(suffix) && upper.len() > suffix.len() {
            return upper[..upper.len() - suffix.len()].to_string();
        }
    }
    upper
}

/// Approximate Unix timestamp for a date + hour (UTC). Good enough for T calculation.
fn ymd_to_unix(year: i32, month: u32, day: u32, h: u32, _m: u32, _s: u32) -> Option<i64> {
    // Days from epoch to Jan 1 of the year
    let y = year as i64;
    let m = month as i64;
    let d = day as i64;
    // Zeller-ish: count days from 1970-01-01
    let days_since_epoch = days_from_epoch(y, m + 1, d)?;
    Some(days_since_epoch * 86400 + h as i64 * 3600)
}

fn days_from_epoch(y: i64, m: i64, d: i64) -> Option<i64> {
    // Using the formula from https://en.wikipedia.org/wiki/Julian_day#Converting_Julian_or_Gregorian_calendar_date_to_Julian_day_number
    let a = (14 - m) / 12;
    let yy = y + 4800 - a;
    let mm = m + 12 * a - 3;
    let jdn = d + (153 * mm + 2) / 5 + 365 * yy + yy / 4 - yy / 100 + yy / 400 - 32045;
    // Unix epoch = JDN 2440588
    Some(jdn - 2440588)
}

/// Time to expiry in years from now.
pub fn time_to_expiry(expiry_ts: i64) -> f64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let seconds = (expiry_ts - now).max(0) as f64;
    seconds / (365.25 * 24.0 * 3600.0)
}

// ── Leg pricing ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegPriceInput {
    pub instrument_name: String,
    pub side: String,          // "BUY" | "SELL"
    pub quantity: f64,
    pub quoted_price: f64,
    /// Override spot price for this leg's underlying (0 = use spot_prices map)
    pub spot_override: Option<f64>,
    /// Override implied vol for this leg (0 = use default_vol)
    pub vol_override: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegPriceResult {
    pub instrument_name: String,
    pub is_option: bool,
    pub quoted_price: f64,
    pub fair_value: Option<f64>,
    pub diff: Option<f64>,   // quoted - fair (positive = expensive for buyer)
    pub iv: Option<f64>,     // decimal (e.g. 0.80 = 80%)
    pub spot_used: Option<f64>,
    pub greeks: Option<Greeks>,
}

pub fn price_leg(
    leg: &LegPriceInput,
    spot_prices: &std::collections::HashMap<String, f64>,
    risk_free_rate: f64,
    default_vol: f64,
) -> LegPriceResult {
    let parsed = parse_option(&leg.instrument_name);
    if parsed.is_none() {
        return LegPriceResult {
            instrument_name: leg.instrument_name.clone(),
            is_option: false,
            quoted_price: leg.quoted_price,
            fair_value: None,
            diff: None,
            iv: None,
            spot_used: None,
            greeks: None,
        };
    }
    let opt = parsed.unwrap();
    let s = leg.spot_override
        .unwrap_or_else(|| *spot_prices.get(&opt.underlying).unwrap_or(&0.0));
    let t = time_to_expiry(opt.expiry_ts);

    if s <= 0.0 || t <= 0.0 {
        return LegPriceResult {
            instrument_name: leg.instrument_name.clone(),
            is_option: true,
            quoted_price: leg.quoted_price,
            fair_value: None,
            diff: None,
            iv: None,
            spot_used: if s > 0.0 { Some(s) } else { None },
            greeks: None,
        };
    }

    // Compute IV: use vol_override if provided, else from quoted_price, else default_vol
    let iv = if let Some(vo) = leg.vol_override.filter(|&v| v > 0.0) {
        vo
    } else if leg.quoted_price > 0.0 {
        implied_vol(leg.quoted_price, s, opt.strike, t, risk_free_rate, opt.is_call)
            .unwrap_or(default_vol)
    } else {
        default_vol
    };

    let fair_value = bs_price(s, opt.strike, t, risk_free_rate, iv, opt.is_call);
    let diff = leg.quoted_price - fair_value;
    let greeks = bs_greeks(s, opt.strike, t, risk_free_rate, iv, opt.is_call);

    LegPriceResult {
        instrument_name: leg.instrument_name.clone(),
        is_option: true,
        quoted_price: leg.quoted_price,
        fair_value: Some(fair_value),
        diff: Some(diff),
        iv: Some(iv),
        spot_used: Some(s),
        greeks,
    }
}
