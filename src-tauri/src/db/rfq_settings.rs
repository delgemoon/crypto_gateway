use crate::api::models::RfqSettings;
use crate::db::DbPool;
use rusqlite::params;

pub fn get_rfq(pool: &DbPool) -> Result<RfqSettings, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let result = conn.query_row(
        "SELECT risk_free_rate, default_vol, spot_source, vol_source,
                base_spread, gamma_sensitivity, vega_sensitivity, max_skew, trading_coin,
                auto_quote, auto_quote_timeout_secs
         FROM rfq_settings WHERE id = 1",
        [],
        |row| {
            Ok(RfqSettings {
                risk_free_rate:          row.get::<_, Option<f64>>(0)?.unwrap_or(0.05),
                default_vol:             row.get::<_, Option<f64>>(1)?.unwrap_or(0.80),
                spot_source:             row.get::<_, Option<String>>(2)?.unwrap_or_else(|| "deribit".to_string()),
                vol_source:              row.get::<_, Option<String>>(3)?.unwrap_or_else(|| "deribit".to_string()),
                base_spread:             row.get::<_, Option<f64>>(4)?.unwrap_or(0.01),
                gamma_sensitivity:       row.get::<_, Option<f64>>(5)?.unwrap_or(0.5),
                vega_sensitivity:        row.get::<_, Option<f64>>(6)?.unwrap_or(0.0005),
                max_skew:                row.get::<_, Option<f64>>(7)?.unwrap_or(0.05),
                trading_coin:            row.get::<_, Option<String>>(8)?.unwrap_or_else(|| "BTC".to_string()),
                auto_quote:              row.get::<_, Option<i64>>(9)?.unwrap_or(0) != 0,
                auto_quote_timeout_secs: row.get::<_, Option<i64>>(10)?.unwrap_or(30) as u32,
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
        "INSERT INTO rfq_settings (id, risk_free_rate, default_vol, spot_source, vol_source,
                                   base_spread, gamma_sensitivity, vega_sensitivity, max_skew, trading_coin,
                                   auto_quote, auto_quote_timeout_secs)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(id) DO UPDATE SET
             risk_free_rate          = excluded.risk_free_rate,
             default_vol             = excluded.default_vol,
             spot_source             = excluded.spot_source,
             vol_source              = excluded.vol_source,
             base_spread             = excluded.base_spread,
             gamma_sensitivity       = excluded.gamma_sensitivity,
             vega_sensitivity        = excluded.vega_sensitivity,
             max_skew                = excluded.max_skew,
             trading_coin            = excluded.trading_coin,
             auto_quote              = excluded.auto_quote,
             auto_quote_timeout_secs = excluded.auto_quote_timeout_secs",
        params![s.risk_free_rate, s.default_vol, s.spot_source, s.vol_source,
                s.base_spread, s.gamma_sensitivity, s.vega_sensitivity, s.max_skew, s.trading_coin,
                s.auto_quote as i64, s.auto_quote_timeout_secs as i64],
    ).map_err(|e| e.to_string())?;
    Ok(())
}
