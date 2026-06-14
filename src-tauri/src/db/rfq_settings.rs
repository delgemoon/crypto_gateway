use crate::api::models::RfqSettings;
use crate::db::DbPool;
use rusqlite::params;

pub fn get_rfq(pool: &DbPool) -> Result<RfqSettings, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let result = conn.query_row(
        "SELECT risk_free_rate, default_vol, spot_source, vol_source
         FROM rfq_settings WHERE id = 1",
        [],
        |row| {
            Ok(RfqSettings {
                risk_free_rate: row.get::<_, Option<f64>>(0)?.unwrap_or(0.05),
                default_vol:    row.get::<_, Option<f64>>(1)?.unwrap_or(0.80),
                spot_source:    row.get::<_, Option<String>>(2)?.unwrap_or_else(|| "deribit".to_string()),
                vol_source:     row.get::<_, Option<String>>(3)?.unwrap_or_else(|| "deribit".to_string()),
            })
        },
    );
    match result {
        Ok(s) => Ok(s),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(RfqSettings::default()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save_rfq(pool: &DbPool, s: &RfqSettings) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO rfq_settings (id, risk_free_rate, default_vol, spot_source, vol_source)
         VALUES (1, ?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET
             risk_free_rate = excluded.risk_free_rate,
             default_vol    = excluded.default_vol,
             spot_source    = excluded.spot_source,
             vol_source     = excluded.vol_source",
        params![s.risk_free_rate, s.default_vol, s.spot_source, s.vol_source],
    ).map_err(|e| e.to_string())?;
    Ok(())
}
