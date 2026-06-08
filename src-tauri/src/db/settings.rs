use crate::api::models::{ClientInfo, GeneralSettings, TelegramChat, TelegramSettings};
use crate::db::{crypto, DbPool};
use rusqlite::params;

// ── General Settings ───────────────────────────────────────────────────────

pub fn get_general(pool: &DbPool) -> Result<GeneralSettings, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let result = conn.query_row(
        "SELECT theme, default_currency, number_locale,
                price_decimals, size_decimals, confirm_orders, watched_coins, bot_id
         FROM general_settings WHERE id = 1",
        [],
        |row| {
            Ok(GeneralSettings {
                theme: row.get(0)?,
                default_currency: row.get(1)?,
                number_locale: row.get(2)?,
                price_decimals: row.get(3)?,
                size_decimals: row.get(4)?,
                confirm_orders: row.get(5)?,
                watched_coins: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                bot_id: row.get::<_, Option<u16>>(7)?.unwrap_or(1),
            })
        },
    );
    match result {
        Ok(s) => Ok(s),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(GeneralSettings::default()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save_general(pool: &DbPool, s: &GeneralSettings) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO general_settings
             (id, theme, default_currency, number_locale,
              price_decimals, size_decimals, confirm_orders, watched_coins, bot_id)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
             theme            = excluded.theme,
             default_currency = excluded.default_currency,
             number_locale    = excluded.number_locale,
             price_decimals   = excluded.price_decimals,
             size_decimals    = excluded.size_decimals,
             confirm_orders   = excluded.confirm_orders,
             watched_coins    = excluded.watched_coins,
             bot_id           = excluded.bot_id",
        params![
            s.theme,
            s.default_currency,
            s.number_locale,
            s.price_decimals,
            s.size_decimals,
            s.confirm_orders,
            s.watched_coins,
            s.bot_id,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Telegram Settings ─────────────────────────────────────────────────────

pub fn get_telegram(pool: &DbPool, enc_key: &[u8; 32]) -> Result<TelegramSettings, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let result = conn.query_row(
        "SELECT bot_token_enc, default_chat_id FROM telegram_settings WHERE id = 1",
        [],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
            ))
        },
    );
    match result {
        Ok((token_enc, chat_id)) => {
            let bot_token = if token_enc.is_empty() {
                String::new()
            } else {
                crypto::decrypt(enc_key, &token_enc).unwrap_or_default()
            };
            Ok(TelegramSettings { bot_token, default_chat_id: chat_id })
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(TelegramSettings::default()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save_telegram(pool: &DbPool, enc_key: &[u8; 32], s: &TelegramSettings) -> Result<(), String> {
    let token_enc = if s.bot_token.is_empty() {
        String::new()
    } else {
        crypto::encrypt(enc_key, &s.bot_token)?
    };
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO telegram_settings (id, bot_token_enc, default_chat_id)
         VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET
             bot_token_enc   = excluded.bot_token_enc,
             default_chat_id = excluded.default_chat_id",
        params![token_enc, s.default_chat_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}


pub fn get_client(pool: &DbPool) -> Result<ClientInfo, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let result = conn.query_row(
        "SELECT company_name, contact_name, phone, email, telegram_handle, tags, notes
         FROM client_info WHERE id = 1",
        [],
        |row| {
            Ok(ClientInfo {
                company_name: row.get(0)?,
                contact_name: row.get(1)?,
                phone: row.get(2)?,
                email: row.get(3)?,
                telegram_handle: row.get(4)?,
                tags: row.get(5)?,
                notes: row.get(6)?,
            })
        },
    );
    match result {
        Ok(c) => Ok(c),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(ClientInfo::default()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save_client(pool: &DbPool, c: &ClientInfo) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO client_info
             (id, company_name, contact_name, phone, email, telegram_handle, tags, notes)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
             company_name    = excluded.company_name,
             contact_name    = excluded.contact_name,
             phone           = excluded.phone,
             email           = excluded.email,
             telegram_handle = excluded.telegram_handle,
             tags            = excluded.tags,
             notes           = excluded.notes",
        params![
            c.company_name,
            c.contact_name,
            c.phone,
            c.email,
            c.telegram_handle,
            c.tags,
            c.notes,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Known Telegram Chats ───────────────────────────────────────────────────

pub fn get_known_chats(pool: &DbPool) -> Result<Vec<TelegramChat>, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, kind, title, username FROM telegram_known_chats ORDER BY title, id"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok(TelegramChat {
            id:       row.get(0)?,
            kind:     row.get(1)?,
            title:    { let s: String = row.get(2)?; if s.is_empty() { None } else { Some(s) } },
            username: { let s: String = row.get(3)?; if s.is_empty() { None } else { Some(s) } },
        })
    }).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn upsert_known_chat(pool: &DbPool, chat: &TelegramChat) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO telegram_known_chats (id, kind, title, username)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET
             kind     = excluded.kind,
             title    = excluded.title,
             username = excluded.username",
        params![
            chat.id,
            chat.kind,
            chat.title.as_deref().unwrap_or(""),
            chat.username.as_deref().unwrap_or(""),
        ],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_known_chat(pool: &DbPool, chat_id: i64) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM telegram_known_chats WHERE id = ?1", params![chat_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}