use crate::api::models::{Client, ClientTelegramChat, Tag};
use crate::db::DbPool;
use rusqlite::params;
use uuid::Uuid;

// ── Tags ───────────────────────────────────────────────────────────────────

pub fn get_tags(pool: &DbPool) -> Result<Vec<Tag>, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT id, name, color FROM tags ORDER BY name")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok(Tag { id: row.get(0)?, name: row.get(1)?, color: row.get(2)? })
    }).map_err(|e| e.to_string())?;
    rows.map(|r| r.map_err(|e| e.to_string())).collect()
}

pub fn upsert_tag(pool: &DbPool, tag: Tag) -> Result<Tag, String> {
    let id = if tag.id.is_empty() { Uuid::new_v4().to_string() } else { tag.id.clone() };
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO tags (id, name, color) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name, color=excluded.color",
        params![id, tag.name, tag.color],
    ).map_err(|e| e.to_string())?;
    Ok(Tag { id, ..tag })
}

pub fn delete_tag(pool: &DbPool, id: &str) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM tags WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Clients ────────────────────────────────────────────────────────────────

pub fn get_clients(pool: &DbPool) -> Result<Vec<Client>, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, company_name, contact_name, phone, email, tag_ids, notes
         FROM clients ORDER BY company_name, contact_name",
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok(Client {
            id:           row.get(0)?,
            company_name: row.get(1)?,
            contact_name: row.get(2)?,
            phone:        row.get(3)?,
            email:        row.get(4)?,
            tag_ids:      row.get(5)?,
            notes:        row.get(6)?,
        })
    }).map_err(|e| e.to_string())?;
    rows.map(|r| r.map_err(|e| e.to_string())).collect()
}

pub fn upsert_client(pool: &DbPool, client: Client) -> Result<Client, String> {
    let id = if client.id.is_empty() { Uuid::new_v4().to_string() } else { client.id.clone() };
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO clients (id, company_name, contact_name, phone, email, tag_ids, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
             company_name = excluded.company_name,
             contact_name = excluded.contact_name,
             phone        = excluded.phone,
             email        = excluded.email,
             tag_ids      = excluded.tag_ids,
             notes        = excluded.notes",
        params![id, client.company_name, client.contact_name, client.phone,
                client.email, client.tag_ids, client.notes],
    ).map_err(|e| e.to_string())?;
    Ok(Client { id, ..client })
}

pub fn delete_client(pool: &DbPool, id: &str) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM clients WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Client Telegram Chats ──────────────────────────────────────────────────

pub fn get_client_chats(pool: &DbPool, client_id: &str) -> Result<Vec<ClientTelegramChat>, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, client_id, chat_id, label FROM client_telegram_chats
         WHERE client_id = ?1 ORDER BY label",
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![client_id], |row| {
        Ok(ClientTelegramChat {
            id:        row.get(0)?,
            client_id: row.get(1)?,
            chat_id:   row.get(2)?,
            label:     row.get(3)?,
        })
    }).map_err(|e| e.to_string())?;
    rows.map(|r| r.map_err(|e| e.to_string())).collect()
}

pub fn upsert_client_chat(pool: &DbPool, chat: ClientTelegramChat) -> Result<ClientTelegramChat, String> {
    let id = if chat.id.is_empty() { Uuid::new_v4().to_string() } else { chat.id.clone() };
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO client_telegram_chats (id, client_id, chat_id, label)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET chat_id=excluded.chat_id, label=excluded.label",
        params![id, chat.client_id, chat.chat_id, chat.label],
    ).map_err(|e| e.to_string())?;
    Ok(ClientTelegramChat { id, ..chat })
}

pub fn delete_client_chat(pool: &DbPool, id: &str) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM client_telegram_chats WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Account ↔ Client links ─────────────────────────────────────────────────

pub fn get_account_clients(pool: &DbPool, account_id: &str) -> Result<Vec<String>, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT client_id FROM account_clients WHERE account_id = ?1 ORDER BY client_id",
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![account_id], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.map(|r| r.map_err(|e| e.to_string())).collect()
}

/// Replace the full set of linked clients for an account.
pub fn set_account_clients(pool: &DbPool, account_id: &str, client_ids: &[String]) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM account_clients WHERE account_id = ?1", params![account_id])
        .map_err(|e| e.to_string())?;
    for cid in client_ids {
        conn.execute(
            "INSERT OR IGNORE INTO account_clients (account_id, client_id) VALUES (?1, ?2)",
            params![account_id, cid],
        ).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Collect all telegram chat_ids for clients linked to an account.
pub fn get_notify_chats_for_account(pool: &DbPool, account_id: &str) -> Result<Vec<String>, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT ctc.chat_id
         FROM client_telegram_chats ctc
         INNER JOIN account_clients ac ON ac.client_id = ctc.client_id
         WHERE ac.account_id = ?1",
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![account_id], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.map(|r| r.map_err(|e| e.to_string())).collect()
}
