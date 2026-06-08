use rusqlite::params;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::models::{Broadcast, BroadcastPart, BroadcastSend};
use crate::db::DbPool;

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

// ── Create ────────────────────────────────────────────────────────────────

pub fn create_broadcast(
    pool: &DbPool,
    id: &str,
    subject: &str,
    text_body: &str,
    parse_mode: &str,
    recip_type: &str,
    recip_value: &str,
) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO telegram_broadcasts
             (id, subject, text_body, parse_mode, recip_type, recip_value, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7)",
        params![id, subject, text_body, parse_mode, recip_type, recip_value, now_secs()],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn add_part(pool: &DbPool, part: &BroadcastPart) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO telegram_broadcast_parts
             (id, broadcast_id, part_type, file_path, caption, sort_order)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![part.id, part.broadcast_id, part.part_type, part.file_path, part.caption, part.sort_order],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn create_send(pool: &DbPool, send: &BroadcastSend) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO telegram_broadcast_sends
             (id, broadcast_id, part_id, chat_id, client_name, status, error_msg, attempt_count, last_attempt)
         VALUES (?1, ?2, ?3, ?4, ?5, 'pending', '', 0, 0)",
        params![send.id, send.broadcast_id, send.part_id, send.chat_id, send.client_name],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

// ── Read ─────────────────────────────────────────────────────────────────

pub fn get_broadcasts(pool: &DbPool) -> Result<Vec<Broadcast>, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT b.id, b.subject, b.text_body, b.parse_mode, b.recip_type, b.recip_value, b.status, b.created_at,
                COUNT(s.id) as total,
                SUM(CASE WHEN s.status = 'sent'   THEN 1 ELSE 0 END) as sent,
                SUM(CASE WHEN s.status = 'failed' THEN 1 ELSE 0 END) as failed
         FROM telegram_broadcasts b
         LEFT JOIN telegram_broadcast_sends s ON s.broadcast_id = b.id
         GROUP BY b.id
         ORDER BY b.created_at DESC"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok(Broadcast {
            id:          row.get(0)?,
            subject:     row.get(1)?,
            text_body:   row.get(2)?,
            parse_mode:  row.get(3)?,
            recip_type:  row.get(4)?,
            recip_value: row.get(5)?,
            status:      row.get(6)?,
            created_at:  row.get(7)?,
            total:       row.get::<_, i32>(8).unwrap_or(0),
            sent:        row.get::<_, i32>(9).unwrap_or(0),
            failed:      row.get::<_, i32>(10).unwrap_or(0),
        })
    }).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn get_parts(pool: &DbPool, broadcast_id: &str) -> Result<Vec<BroadcastPart>, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, broadcast_id, part_type, file_path, caption, sort_order
         FROM telegram_broadcast_parts WHERE broadcast_id = ?1 ORDER BY sort_order"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![broadcast_id], |row| {
        Ok(BroadcastPart {
            id:           row.get(0)?,
            broadcast_id: row.get(1)?,
            part_type:    row.get(2)?,
            file_path:    row.get(3)?,
            caption:      row.get(4)?,
            sort_order:   row.get(5)?,
        })
    }).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn get_sends(pool: &DbPool, broadcast_id: &str) -> Result<Vec<BroadcastSend>, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, broadcast_id, part_id, chat_id, client_name, status, error_msg,
                message_id, attempt_count, last_attempt
         FROM telegram_broadcast_sends WHERE broadcast_id = ?1
         ORDER BY client_name, part_id"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![broadcast_id], |row| {
        Ok(BroadcastSend {
            id:            row.get(0)?,
            broadcast_id:  row.get(1)?,
            part_id:       row.get(2)?,
            chat_id:       row.get(3)?,
            client_name:   row.get(4)?,
            status:        row.get(5)?,
            error_msg:     row.get(6)?,
            message_id:    row.get(7)?,
            attempt_count: row.get(8)?,
            last_attempt:  row.get(9)?,
        })
    }).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Returns sends with status = 'pending' OR 'failed' (for retry), ordered by sort_order of part.
pub fn get_pending_sends(pool: &DbPool, broadcast_id: &str) -> Result<Vec<BroadcastSend>, String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT s.id, s.broadcast_id, s.part_id, s.chat_id, s.client_name, s.status, s.error_msg,
                s.message_id, s.attempt_count, s.last_attempt
         FROM telegram_broadcast_sends s
         JOIN telegram_broadcast_parts p ON p.id = s.part_id
         WHERE s.broadcast_id = ?1 AND s.status != 'sent'
         ORDER BY s.client_name, p.sort_order"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![broadcast_id], |row| {
        Ok(BroadcastSend {
            id:            row.get(0)?,
            broadcast_id:  row.get(1)?,
            part_id:       row.get(2)?,
            chat_id:       row.get(3)?,
            client_name:   row.get(4)?,
            status:        row.get(5)?,
            error_msg:     row.get(6)?,
            message_id:    row.get(7)?,
            attempt_count: row.get(8)?,
            last_attempt:  row.get(9)?,
        })
    }).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

// ── Update ────────────────────────────────────────────────────────────────

pub fn update_send_status(
    pool: &DbPool,
    send_id: &str,
    status: &str,
    error_msg: &str,
    message_id: Option<i64>,
) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE telegram_broadcast_sends SET
             status        = ?2,
             error_msg     = ?3,
             message_id    = ?4,
             attempt_count = attempt_count + 1,
             last_attempt  = ?5
         WHERE id = ?1",
        params![send_id, status, error_msg, message_id, now_secs()],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn update_broadcast_status(pool: &DbPool, broadcast_id: &str, status: &str) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE telegram_broadcasts SET status = ?2 WHERE id = ?1",
        params![broadcast_id, status],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_broadcast(pool: &DbPool, broadcast_id: &str) -> Result<(), String> {
    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM telegram_broadcasts WHERE id = ?1", params![broadcast_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}
