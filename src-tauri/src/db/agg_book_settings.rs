use crate::agg_book::AggBookConfig;
use crate::db::DbPool;
use rusqlite::params;

pub fn get_all(pool: &DbPool) -> Result<Vec<AggBookConfig>, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, name, base_symbol, instrument_kind, account_ids, unify_quote, \
         max_levels, tick_size, poll_interval_ms, active \
         FROM agg_book_configs ORDER BY name"
    ).map_err(|e| e.to_string())?;

    let rows = stmt.query_map([], |row| {
        let account_ids_json: String = row.get(4)?;
        let account_ids: Vec<String> = serde_json::from_str(&account_ids_json).unwrap_or_default();
        Ok(AggBookConfig {
            id:              row.get(0)?,
            name:            row.get(1)?,
            base_symbol:     row.get(2)?,
            instrument_kind: row.get(3)?,
            account_ids,
            unify_quote:     row.get::<_, i64>(5)? != 0,
            max_levels:      row.get::<_, i64>(6)? as u32,
            tick_size:       row.get(7)?,
            poll_interval_ms: row.get::<_, i64>(8)? as u64,
            active:          row.get::<_, i64>(9)? != 0,
        })
    }).map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn upsert(pool: &DbPool, config: &AggBookConfig) -> Result<AggBookConfig, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let account_ids_json = serde_json::to_string(&config.account_ids).map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO agg_book_configs \
         (id, name, base_symbol, instrument_kind, account_ids, unify_quote, max_levels, tick_size, poll_interval_ms, active) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
         ON CONFLICT(id) DO UPDATE SET \
           name = excluded.name, \
           base_symbol = excluded.base_symbol, \
           instrument_kind = excluded.instrument_kind, \
           account_ids = excluded.account_ids, \
           unify_quote = excluded.unify_quote, \
           max_levels = excluded.max_levels, \
           tick_size = excluded.tick_size, \
           poll_interval_ms = excluded.poll_interval_ms, \
           active = excluded.active",
        params![
            config.id,
            config.name,
            config.base_symbol,
            config.instrument_kind,
            account_ids_json,
            config.unify_quote as i64,
            config.max_levels as i64,
            config.tick_size,
            config.poll_interval_ms as i64,
            config.active as i64,
        ],
    ).map_err(|e| e.to_string())?;

    Ok(config.clone())
}

pub fn delete(pool: &DbPool, id: &str) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM agg_book_configs WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}
