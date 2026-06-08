use crate::api::models::Account;
use crate::db::{crypto, DbPool};
use rusqlite::params;
use uuid::Uuid;

pub fn get_all(pool: &DbPool, key: &[u8; 32]) -> Result<Vec<Account>, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, exchange, api_key_enc, api_secret_enc, passphrase_enc,
                    testnet, default_tif, default_post_only, risk_limit, rate_tier,
                    rpc_url, chain_id
             FROM accounts ORDER BY name",
        )
        .map_err(|e| e.to_string())?;

    let rows: Vec<(String,String,String,String,String,Option<String>,bool,String,bool,f64,String,Option<String>,Option<i64>)> =
        stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get::<_, Option<String>>(10)?.unwrap_or_else(|| "tier1".to_string()),
                row.get(11)?,
                row.get(12)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e: rusqlite::Error| e.to_string())?;

    let mut accounts = Vec::new();
    for (id, name, exchange, ak_enc, as_enc, pp_enc, testnet, tif, post_only, risk_limit, rate_tier, rpc_url, chain_id) in rows {
        accounts.push(Account {
            id,
            name,
            exchange,
            api_key: crypto::decrypt(key, &ak_enc)?,
            api_secret: crypto::decrypt(key, &as_enc)?,
            passphrase: pp_enc.map(|e| crypto::decrypt(key, &e)).transpose()?,
            testnet,
            default_tif: tif,
            default_post_only: post_only,
            risk_limit,
            rate_tier,
            rpc_url,
            chain_id: chain_id.map(|v| v as u64),
        });
    }
    Ok(accounts)
}

pub fn upsert(pool: &DbPool, key: &[u8; 32], mut account: Account) -> Result<Account, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    if account.id.is_empty() {
        account.id = Uuid::new_v4().to_string();
    }

    let ak_enc = crypto::encrypt(key, &account.api_key)?;
    let as_enc = crypto::encrypt(key, &account.api_secret)?;
    let pp_enc = account
        .passphrase
        .as_deref()
        .filter(|p| !p.is_empty())
        .map(|p| crypto::encrypt(key, p))
        .transpose()?;

    conn.execute(
        "INSERT INTO accounts
             (id, name, exchange, api_key_enc, api_secret_enc, passphrase_enc,
              testnet, default_tif, default_post_only, risk_limit, rate_tier,
              rpc_url, chain_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(id) DO UPDATE SET
             name              = excluded.name,
             exchange          = excluded.exchange,
             api_key_enc       = excluded.api_key_enc,
             api_secret_enc    = excluded.api_secret_enc,
             passphrase_enc    = excluded.passphrase_enc,
             testnet           = excluded.testnet,
             default_tif       = excluded.default_tif,
             default_post_only = excluded.default_post_only,
             risk_limit        = excluded.risk_limit,
             rate_tier         = excluded.rate_tier,
             rpc_url           = excluded.rpc_url,
             chain_id          = excluded.chain_id",
        params![
            account.id,
            account.name,
            account.exchange,
            ak_enc,
            as_enc,
            pp_enc,
            account.testnet,
            account.default_tif,
            account.default_post_only,
            account.risk_limit,
            account.rate_tier,
            account.rpc_url,
            account.chain_id.map(|v| v as i64),
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(account)
}

pub fn delete(pool: &DbPool, id: &str) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM accounts WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn find_by_id(pool: &DbPool, key: &[u8; 32], id: &str) -> Result<Account, String> {
    get_all(pool, key)?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| format!("Account '{}' not found", id))
}
