use reqwest::{Client, multipart};
use serde_json::Value;
use std::path::Path;

use crate::api::models::{BroadcastPart, BroadcastSend, TelegramChat, TelegramResult};
use crate::db::DbPool;

const TG_BASE: &str = "https://api.telegram.org/bot";

fn api(token: &str, method: &str) -> String {
    format!("{}{}/{}", TG_BASE, token, method)
}

// ── Validate token / get bot info ──────────────────────────────────────────

/// Calls getMe — returns the bot's username on success.
pub async fn validate_token(token: &str) -> Result<String, String> {
    let resp: Value = Client::new()
        .get(&api(token, "getMe"))
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["ok"].as_bool() != Some(true) {
        return Err(resp["description"].as_str().unwrap_or("Invalid token").to_string());
    }
    Ok(resp["result"]["username"].as_str().unwrap_or("bot").to_string())
}

// ── Discover chats from recent updates ────────────────────────────────────

/// Returns unique chats seen in recent getUpdates (last 100).
/// NOTE: calls deleteWebhook first — getUpdates returns nothing while a webhook is set.
pub async fn get_recent_chats(token: &str) -> Result<Vec<TelegramChat>, String> {
    eprintln!("[telegram_sync] calling deleteWebhook to clear any active webhook");
    let dw: Value = Client::new()
        .post(&api(token, "deleteWebhook"))
        .json(&serde_json::json!({ "drop_pending_updates": false }))
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;
    eprintln!("[telegram_sync] deleteWebhook result: ok={}", dw["ok"]);

    eprintln!("[telegram_sync] calling getUpdates (limit=100)");
    let resp: Value = Client::new()
        .post(&api(token, "getUpdates"))
        .json(&serde_json::json!({
            "limit": 100,
            "allowed_updates": ["message", "channel_post", "my_chat_member"]
        }))
        .send().await.map_err(|e| { eprintln!("[telegram_sync] HTTP error: {e}"); e.to_string() })?
        .json().await.map_err(|e| { eprintln!("[telegram_sync] JSON parse error: {e}"); e.to_string() })?;

    eprintln!("[telegram_sync] getUpdates ok={} result_len={}",
        resp["ok"], resp["result"].as_array().map(|a| a.len()).unwrap_or(0));

    if resp["ok"].as_bool() != Some(true) {
        let desc = resp["description"].as_str().unwrap_or("getUpdates failed").to_string();
        eprintln!("[telegram_sync] getUpdates failed: {desc}");
        return Err(desc);
    }

    let updates = resp["result"].as_array().unwrap_or(&vec![]).clone();
    eprintln!("[telegram_sync] processing {} updates", updates.len());

    let mut seen = std::collections::HashMap::new();
    for (i, update) in updates.iter().enumerate() {
        eprintln!("[telegram_sync] update[{i}] keys: {}",
            update.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>().join(", ")).unwrap_or_default());
        // check message, channel_post, and my_chat_member (bot added to chat)
        for key in ["message", "channel_post"] {
            if let Some(chat) = update[key]["chat"].as_object() {
                let id = chat.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                let kind = chat.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let title = chat.get("title").and_then(|v| v.as_str()).map(|s| s.to_string());
                let username = chat.get("username").and_then(|v| v.as_str()).map(|s| s.to_string());
                eprintln!("[telegram_sync] found chat via '{key}': id={id} kind={kind} title={title:?} username={username:?}");
                seen.entry(id).or_insert_with(|| TelegramChat { id, kind, title, username });
            }
        }
        // my_chat_member fires when bot is added to a group/channel
        for key in ["my_chat_member", "chat_member"] {
            if let Some(chat) = update[key]["chat"].as_object() {
                let id = chat.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                let kind = chat.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let title = chat.get("title").and_then(|v| v.as_str()).map(|s| s.to_string());
                let username = chat.get("username").and_then(|v| v.as_str()).map(|s| s.to_string());
                eprintln!("[telegram_sync] found chat via '{key}': id={id} kind={kind} title={title:?} username={username:?}");
                seen.entry(id).or_insert_with(|| TelegramChat { id, kind, title, username });
            }
        }
    }
    let result: Vec<TelegramChat> = seen.into_values().collect();
    eprintln!("[telegram_sync] returning {} unique chats: {:?}",
        result.len(),
        result.iter().map(|c| format!("{}({})", c.title.as_deref().unwrap_or("?"), c.id)).collect::<Vec<_>>());
    Ok(result)
}

// ── Resolve @username or numeric ID → chat info ────────────────────────────

pub async fn resolve_chat(token: &str, chat_ref: &str) -> Result<TelegramChat, String> {
    let resp: Value = Client::new()
        .get(&api(token, "getChat"))
        .query(&[("chat_id", chat_ref)])
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp["ok"].as_bool() != Some(true) {
        return Err(resp["description"].as_str().unwrap_or("getChat failed").to_string());
    }
    let c = &resp["result"];
    Ok(TelegramChat {
        id:       c["id"].as_i64().unwrap_or(0),
        kind:     c["type"].as_str().unwrap_or("").to_string(),
        title:    c["title"].as_str().map(|s| s.to_string()),
        username: c["username"].as_str().map(|s| s.to_string()),
    })
}

// ── Send text message ──────────────────────────────────────────────────────

/// `parse_mode`: "HTML" | "MarkdownV2" | "" (plain)
pub async fn send_message(
    token: &str,
    chat_id: &str,
    text: &str,
    parse_mode: &str,
    disable_preview: bool,
) -> Result<TelegramResult, String> {
    let mut params = vec![
        ("chat_id", chat_id.to_string()),
        ("text", text.to_string()),
    ];
    if !parse_mode.is_empty() {
        params.push(("parse_mode", parse_mode.to_string()));
    }
    if disable_preview {
        params.push(("disable_web_page_preview", "true".to_string()));
    }

    let resp: Value = Client::new()
        .post(&api(token, "sendMessage"))
        .form(&params)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;
    eprintln!("sendMessage response: {:?}", resp);
    parse_result(&resp)
}

// ── Send photo ─────────────────────────────────────────────────────────────

pub async fn send_photo(
    token: &str,
    chat_id: &str,
    file_path: &str,
    caption: &str,
    parse_mode: &str,
) -> Result<TelegramResult, String> {
    let path = Path::new(file_path);
    let file_name = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("photo.jpg")
        .to_string();
    let bytes = std::fs::read(path).map_err(|e| format!("read file: {}", e))?;

    let file_part = multipart::Part::bytes(bytes)
        .file_name(file_name)
        .mime_str("image/jpeg").map_err(|e| e.to_string())?;

    let mut form = multipart::Form::new()
        .text("chat_id", chat_id.to_string())
        .part("photo", file_part);

    if !caption.is_empty() {
        form = form.text("caption", caption.to_string());
    }
    if !parse_mode.is_empty() {
        form = form.text("parse_mode", parse_mode.to_string());
    }

    let resp: Value = Client::new()
        .post(&api(token, "sendPhoto"))
        .multipart(form)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;
    eprintln!("sendPhoto response: {:?}", resp);
    parse_result(&resp)
}

// ── Send document (any file) ───────────────────────────────────────────────

pub async fn send_document(
    token: &str,
    chat_id: &str,
    file_path: &str,
    caption: &str,
    parse_mode: &str,
) -> Result<TelegramResult, String> {
    let path = Path::new(file_path);
    let file_name = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    let bytes = std::fs::read(path).map_err(|e| format!("read file: {}", e))?;

    // detect mime from extension
    let mime = mime_from_ext(path.extension().and_then(|e| e.to_str()).unwrap_or(""));
    let file_part = multipart::Part::bytes(bytes)
        .file_name(file_name)
        .mime_str(mime).map_err(|e| e.to_string())?;

    let mut form = multipart::Form::new()
        .text("chat_id", chat_id.to_string())
        .part("document", file_part);

    if !caption.is_empty() {
        form = form.text("caption", caption.to_string());
    }
    if !parse_mode.is_empty() {
        form = form.text("parse_mode", parse_mode.to_string());
    }

    let resp: Value = Client::new()
        .post(&api(token, "sendDocument"))
        .multipart(form)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    parse_result(&resp)
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn parse_result(resp: &Value) -> Result<TelegramResult, String> {
    if resp["ok"].as_bool() == Some(true) {
        Ok(TelegramResult {
            ok: true,
            message_id: resp["result"]["message_id"].as_i64(),
            error: None,
        })
    } else {
        Ok(TelegramResult {
            ok: false,
            message_id: None,
            error: Some(resp["description"].as_str().unwrap_or("Telegram error").to_string()),
        })
    }
}

fn mime_from_ext(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png"          => "image/png",
        "gif"          => "image/gif",
        "webp"         => "image/webp",
        "pdf"          => "application/pdf",
        "csv"          => "text/csv",
        "txt"          => "text/plain",
        "json"         => "application/json",
        "zip"          => "application/zip",
        _              => "application/octet-stream",
    }
}

// ── Broadcast: resolve recipients ──────────────────────────────────────────

/// Returns a list of (chat_id, display_name) pairs for a broadcast.
/// recip_type: "group" | "clients" | "tag"
/// recip_value: for group = chat_id string; clients = comma-sep client IDs; tag = tag_id
pub fn resolve_recipients(
    pool: &DbPool,
    recip_type: &str,
    recip_value: &str,
) -> Result<Vec<(i64, String)>, String> {
    match recip_type {
        "group" => {
            let chat_id: i64 = recip_value.parse().map_err(|_| "Invalid group chat_id".to_string())?;
            // Try to get a name from known_chats
            let conn = pool.get().map_err(|e| e.to_string())?;
            let name: String = conn.query_row(
                "SELECT COALESCE(NULLIF(title,''), NULLIF(username,''), CAST(id AS TEXT))
                 FROM telegram_known_chats WHERE id = ?1",
                rusqlite::params![chat_id],
                |r| r.get(0),
            ).unwrap_or_else(|_| format!("Group {}", chat_id));
            Ok(vec![(chat_id, name)])
        }
        "clients" => {
            let ids: Vec<&str> = recip_value.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
            let mut result = Vec::new();
            for client_id in ids {
                let chats = crate::db::clients::get_client_chats(pool, client_id)?;
                // Get client display name
                let conn = pool.get().map_err(|e| e.to_string())?;
                let name: String = conn.query_row(
                    "SELECT COALESCE(NULLIF(company_name,''), NULLIF(contact_name,''), id) FROM clients WHERE id = ?1",
                    rusqlite::params![client_id],
                    |r| r.get(0),
                ).unwrap_or_else(|_| client_id.to_string());
                for chat in chats {
                    let chat_id: i64 = chat.chat_id.parse().unwrap_or(0);
                    if chat_id != 0 {
                        let label = if chat.label.is_empty() { name.clone() } else { format!("{} ({})", name, chat.label) };
                        result.push((chat_id, label));
                    }
                }
            }
            Ok(result)
        }
        "tag" => {
            // Find all clients with this tag_id
            let conn = pool.get().map_err(|e| e.to_string())?;
            let mut stmt = conn.prepare(
                "SELECT id, COALESCE(NULLIF(company_name,''), NULLIF(contact_name,''), id)
                 FROM clients WHERE (',' || tag_ids || ',') LIKE ('%,' || ?1 || ',%')
                    OR tag_ids = ?1
                    OR tag_ids LIKE ?1 || ',%'
                    OR tag_ids LIKE '%,' || ?1"
            ).map_err(|e| e.to_string())?;
            let client_rows: Vec<(String, String)> = stmt.query_map(
                rusqlite::params![recip_value],
                |r| Ok((r.get(0)?, r.get(1)?))
            ).map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;

            let mut result = Vec::new();
            for (client_id, client_name) in client_rows {
                let chats = crate::db::clients::get_client_chats(pool, &client_id)?;
                for chat in chats {
                    let chat_id: i64 = chat.chat_id.parse().unwrap_or(0);
                    if chat_id != 0 {
                        let label = if chat.label.is_empty() { client_name.clone() } else { format!("{} ({})", client_name, chat.label) };
                        result.push((chat_id, label));
                    }
                }
            }
            Ok(result)
        }
        _ => Err(format!("Unknown recipient type: {}", recip_type)),
    }
}

// ── Broadcast: send a single part to a chat ────────────────────────────────

pub async fn send_part(token: &str, chat_id: i64, part: &BroadcastPart) -> Result<TelegramResult, String> {
    let chat_str = chat_id.to_string();
    match part.part_type.as_str() {
        "photo"    => send_photo(token, &chat_str, &part.file_path, &part.caption, "HTML").await,
        "document" => send_document(token, &chat_str, &part.file_path, &part.caption, "HTML").await,
        _          => send_message(token, &chat_str, &part.caption, "HTML", false).await,
    }
}

// ── Broadcast: execute all pending/failed sends ────────────────────────────

/// Sends all pending (or failed, for retry) sends for a broadcast.
/// Returns (sent_count, failed_count).
pub async fn execute_broadcast(
    pool: &DbPool,
    token: &str,
    broadcast_id: &str,
) -> Result<(usize, usize), String> {
    crate::db::broadcasts::update_broadcast_status(pool, broadcast_id, "sending")?;

    let parts = crate::db::broadcasts::get_parts(pool, broadcast_id)?;
    let part_map: std::collections::HashMap<String, BroadcastPart> =
        parts.into_iter().map(|p| (p.id.clone(), p)).collect();

    let pending = crate::db::broadcasts::get_pending_sends(pool, broadcast_id)?;
    let mut sent_count = 0usize;
    let mut fail_count = 0usize;

    for send in pending {
        let part = match part_map.get(&send.part_id) {
            Some(p) => p,
            None => {
                let _ = crate::db::broadcasts::update_send_status(pool, &send.id, "failed", "Part not found", None);
                fail_count += 1;
                continue;
            }
        };

        match send_part(token, send.chat_id, part).await {
            Ok(res) if res.ok => {
                let _ = crate::db::broadcasts::update_send_status(pool, &send.id, "sent", "", res.message_id);
                sent_count += 1;
            }
            Ok(res) => {
                let err = res.error.as_deref().unwrap_or("Telegram error");
                let _ = crate::db::broadcasts::update_send_status(pool, &send.id, "failed", err, None);
                fail_count += 1;
            }
            Err(e) => {
                let _ = crate::db::broadcasts::update_send_status(pool, &send.id, "failed", &e, None);
                fail_count += 1;
            }
        }

        // Small delay to respect Telegram rate limits (30 msg/sec per bot, 1/sec per chat)
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    // Update overall broadcast status
    let final_status = if fail_count == 0 { "done" } else { "partial_fail" };
    crate::db::broadcasts::update_broadcast_status(pool, broadcast_id, final_status)?;

    Ok((sent_count, fail_count))
}
