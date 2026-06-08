use crate::db::DbPool;
use rusqlite::params;
use serde::{Deserialize, Serialize};

// ── Model ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VenueSettings {
    pub exchange:         String,
    pub instrument_types: Vec<String>,
    pub market_feeds:     Vec<String>,
    pub order_feeds:      Vec<String>,
    pub notes:            String,
}

// ── Defaults per exchange ──────────────────────────────────────────────────

pub fn default_for(exchange: &str) -> VenueSettings {
    let (inst, mkt, ord) = match exchange {
        "deribit" => (
            vec!["option", "future", "perpetual", "spot"],
            vec!["orderbook_l3", "market_trades", "reference_data", "top_of_book"],
            vec!["account_summary", "fund_update", "transfer_update",
                 "create_order", "modify_order", "cancel_order", "cancel_all_order",
                 "position_update"],
        ),
        "okx" => (
            vec!["option", "future", "perpetual", "spot"],
            vec!["orderbook_l3", "market_trades", "reference_data", "top_of_book"],
            vec!["account_summary", "fund_update", "transfer_update",
                 "create_order", "modify_order", "cancel_order", "cancel_all_order",
                 "position_update"],
        ),
        "bybit" => (
            vec!["future", "perpetual", "spot"],
            vec!["market_trades", "reference_data", "top_of_book"],
            vec!["account_summary", "fund_update",
                 "create_order", "modify_order", "cancel_order", "cancel_all_order",
                 "position_update"],
        ),
        "coincall" => (
            vec!["option", "future"],
            vec!["market_trades", "reference_data", "top_of_book"],
            vec!["account_summary",
                 "create_order", "cancel_order", "cancel_all_order",
                 "position_update"],
        ),
        "binance" => (
            vec!["future", "perpetual", "spot"],
            vec!["market_trades", "reference_data", "top_of_book"],
            vec!["account_summary", "fund_update",
                 "create_order", "modify_order", "cancel_order", "cancel_all_order",
                 "position_update"],
        ),
        "mexc" => (
            vec!["future", "perpetual", "spot"],
            vec!["market_trades", "reference_data", "top_of_book"],
            vec!["account_summary", "create_order", "cancel_order", "cancel_all_order",
                 "position_update"],
        ),
        "hyperliquid" => (
            vec!["perpetual"],
            vec!["market_trades", "reference_data", "top_of_book"],
            vec!["account_summary", "create_order", "cancel_order", "cancel_all_order",
                 "position_update", "trade_update", "order_update", "fund_update"],
        ),
        "uniswap" => (
            vec!["spot"],
            vec!["top_of_book"],
            vec!["create_order", "account_summary"],
        ),
        _ => (
            vec!["spot"],
            vec!["market_trades", "top_of_book"],
            vec!["account_summary", "create_order", "cancel_order"],
        ),
    };
    VenueSettings {
        exchange:         exchange.to_string(),
        instrument_types: inst.into_iter().map(|s| s.to_string()).collect(),
        market_feeds:     mkt.into_iter().map(|s| s.to_string()).collect(),
        order_feeds:      ord.into_iter().map(|s| s.to_string()).collect(),
        notes:            String::new(),
    }
}

// ── CRUD ───────────────────────────────────────────────────────────────────

pub fn get_all(pool: &DbPool) -> Result<Vec<VenueSettings>, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT exchange, instrument_types, market_feeds, order_feeds, notes
         FROM venue_settings ORDER BY exchange"
    ).map_err(|e| e.to_string())?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    }).map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for r in rows {
        let (exchange, inst_json, mkt_json, ord_json, notes) = r.map_err(|e| e.to_string())?;
        let instrument_types = serde_json::from_str(&inst_json).unwrap_or_default();
        let market_feeds     = serde_json::from_str(&mkt_json).unwrap_or_default();
        let order_feeds      = serde_json::from_str(&ord_json).unwrap_or_default();
        out.push(VenueSettings { exchange, instrument_types, market_feeds, order_feeds, notes });
    }
    Ok(out)
}

pub fn get_one(pool: &DbPool, exchange: &str) -> Result<VenueSettings, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let result = conn.query_row(
        "SELECT exchange, instrument_types, market_feeds, order_feeds, notes
         FROM venue_settings WHERE exchange = ?1",
        params![exchange],
        |row| Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        )),
    );
    match result {
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(default_for(exchange)),
        Err(e) => Err(e.to_string()),
        Ok((ex, inst_json, mkt_json, ord_json, notes)) => {
            Ok(VenueSettings {
                exchange: ex,
                instrument_types: serde_json::from_str(&inst_json).unwrap_or_default(),
                market_feeds:     serde_json::from_str(&mkt_json).unwrap_or_default(),
                order_feeds:      serde_json::from_str(&ord_json).unwrap_or_default(),
                notes,
            })
        }
    }
}

pub fn save(pool: &DbPool, vs: &VenueSettings) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let inst_json = serde_json::to_string(&vs.instrument_types).map_err(|e| e.to_string())?;
    let mkt_json  = serde_json::to_string(&vs.market_feeds).map_err(|e| e.to_string())?;
    let ord_json  = serde_json::to_string(&vs.order_feeds).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO venue_settings (exchange, instrument_types, market_feeds, order_feeds, notes)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(exchange) DO UPDATE SET
             instrument_types = excluded.instrument_types,
             market_feeds     = excluded.market_feeds,
             order_feeds      = excluded.order_feeds,
             notes            = excluded.notes",
        params![vs.exchange, inst_json, mkt_json, ord_json, vs.notes],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete(pool: &DbPool, exchange: &str) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM venue_settings WHERE exchange = ?1", params![exchange])
        .map_err(|e| e.to_string())?;
    Ok(())
}
