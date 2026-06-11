mod api;
mod agg_book;
mod db;
mod market;
mod order_id;
mod rate_limiter;
mod telegram;
mod ws;

use std::sync::Arc;
use tauri::{Manager, Emitter};
use api::dispatch;
use api::models::{
    Account, AccountSummary, Broadcast, BroadcastPart, BroadcastSend, Client, ClientInfo, ClientTelegramChat,
    CreateBroadcastRequest, GeneralSettings, Instrument, Order, OrderResult,
    PlaceOrderRequest, Position, ReferenceData, Tag, Ticker, TelegramSettings, TelegramChat, TelegramResult, Trade, TransactionLog,
};
use rate_limiter::{RateLimiter, RateLimiterStatus};
use ws::{WsManager, WsStatusSnapshot};
use market::MarketDataManager;

// ── App State ──────────────────────────────────────────────────────────────

pub struct AppState {
    pub pool: db::DbPool,
    pub enc_key: [u8; 32],
    pub rate_limiter: RateLimiter,
    pub ws_manager: Arc<WsManager>,
    pub agg_book_manager: Arc<agg_book::AggBookManager>,
    pub market_manager: Arc<MarketDataManager>,
}

// ── Account CRUD ───────────────────────────────────────────────────────────

#[tauri::command]
fn get_accounts(state: tauri::State<'_, AppState>) -> Result<Vec<Account>, String> {
    let accounts = db::accounts::get_all(&state.pool, &state.enc_key)?;
    // Register rate limit buckets for every loaded account
    for acc in &accounts {
        state.rate_limiter.configure_account(&acc.id, &acc.exchange, &acc.rate_tier);
    }
    Ok(accounts)
}

#[tauri::command]
fn save_account(state: tauri::State<'_, AppState>, account: Account) -> Result<Account, String> {
    // Update rate limiter tier whenever account is saved
    state.rate_limiter.configure_account(&account.id, &account.exchange, &account.rate_tier);
    db::accounts::upsert(&state.pool, &state.enc_key, account)
}

#[tauri::command]
fn delete_account(state: tauri::State<'_, AppState>, id: String) -> Result<bool, String> {
    state.rate_limiter.remove_account(&id);
    db::accounts::delete(&state.pool, &id)?;
    Ok(true)
}

// ── Settings ───────────────────────────────────────────────────────────────

#[tauri::command]
fn get_general_settings(state: tauri::State<'_, AppState>) -> Result<GeneralSettings, String> {
    db::settings::get_general(&state.pool)
}

#[tauri::command]
fn save_general_settings(
    state: tauri::State<'_, AppState>,
    settings: GeneralSettings,
) -> Result<(), String> {
    db::settings::save_general(&state.pool, &settings)
}

#[tauri::command]
fn get_client_info(state: tauri::State<'_, AppState>) -> Result<ClientInfo, String> {
    db::settings::get_client(&state.pool)
}

#[tauri::command]
fn save_client_info(
    state: tauri::State<'_, AppState>,
    info: ClientInfo,
) -> Result<(), String> {
    db::settings::save_client(&state.pool, &info)
}

// ── Public Market Data ─────────────────────────────────────────────────────

#[tauri::command]
async fn fetch_instruments(
    exchange: String,
    currency: String,
    kind: String,
    testnet: Option<bool>,
) -> Result<Vec<Instrument>, String> {
    dispatch::fetch_instruments(&exchange, &currency, &kind, testnet.unwrap_or(false)).await
}

#[tauri::command]
async fn fetch_ticker(
    exchange: String,
    instrument_name: String,
    testnet: Option<bool>,
) -> Result<Ticker, String> {
    dispatch::fetch_ticker(&exchange, &instrument_name, testnet.unwrap_or(false)).await
}

// ── Order Commands ─────────────────────────────────────────────────────────

#[tauri::command]
async fn place_order(
    state: tauri::State<'_, AppState>,
    mut req: PlaceOrderRequest,
) -> Result<OrderResult, String> {
    let account = db::accounts::find_by_id(&state.pool, &state.enc_key, &req.account_id)?;

    // Rate limit check: per-exchange group + per-account tier
    state.rate_limiter
        .check_account(&account.exchange, "private", "place_order", &req.account_id, 1.0)
        .map_err(|e| e.to_string())?;

    // Generate system order ID (UTC ms timestamp) and build client order ID
    let system_order_id = order_id::new_system_order_id();
    let general = db::settings::get_general(&state.pool).unwrap_or_default();
    let clord_id = order_id::build_client_order_id(system_order_id, general.bot_id);
    req.client_order_id = Some(clord_id);

    let result = dispatch::place_order(&account, &req).await?;

    // Fire Telegram notifications for all clients linked to this account
    if result.success {
        let tg_cfg = db::settings::get_telegram(&state.pool, &state.enc_key).unwrap_or_default();
        if !tg_cfg.bot_token.is_empty() {
            if let Ok(chats) = db::clients::get_notify_chats_for_account(&state.pool, &req.account_id) {
                if !chats.is_empty() {
                    let side_emoji = if req.side == "buy" { "🟢" } else { "🔴" };
                    let price_str = req.price.map(|p| format!(" @ <b>{}</b>", p)).unwrap_or_default();
                    let msg = format!(
                        "{} <b>{} {}</b>{}\n\
                        Qty: <b>{}</b>  ·  Type: <b>{}</b>\n\
                        Account: <i>{}</i>  ·  Exchange: <b>{}</b>{}",
                        side_emoji,
                        req.side.to_uppercase(), req.instrument_name, price_str,
                        req.amount, req.order_type,
                        account.name, account.exchange,
                        if account.testnet { "\n⚠️ <i>Testnet order</i>" } else { "" }
                    );
                    let token = tg_cfg.bot_token.clone();
                    tauri::async_runtime::spawn(async move {
                        for chat_id in chats {
                            let _ = telegram::send_message(&token, &chat_id, &msg, "HTML", true).await;
                        }
                    });
                }
            }
        }
    }

    Ok(result)
}

#[tauri::command]
async fn cancel_order(
    state: tauri::State<'_, AppState>,
    account_id: String,
    order_id: String,
    instrument_name: Option<String>,
) -> Result<bool, String> {
    let account = db::accounts::find_by_id(&state.pool, &state.enc_key, &account_id)?;
    state.rate_limiter
        .check_account(&account.exchange, "private", "cancel_order", &account_id, 1.0)
        .map_err(|e| e.to_string())?;
    dispatch::cancel_order(&account, &order_id, instrument_name.as_deref()).await
}

#[tauri::command]
async fn get_open_orders(
    state: tauri::State<'_, AppState>,
    account_id: String,
    instrument_name: String,
) -> Result<Vec<Order>, String> {
    let account = db::accounts::find_by_id(&state.pool, &state.enc_key, &account_id)?;
    dispatch::get_open_orders(&account, &instrument_name).await
}

#[tauri::command]
async fn get_account_summary(
    state: tauri::State<'_, AppState>,
    account_id: String,
    currency: String,
) -> Result<AccountSummary, String> {
    let account = db::accounts::find_by_id(&state.pool, &state.enc_key, &account_id)?;
    dispatch::get_account_summary(&account, &currency).await
}

// ── Open Orders & Trade History ───────────────────────────────────────────

#[tauri::command]
async fn get_positions(
    state: tauri::State<'_, AppState>,
    account_id: String,
    currency: String,
) -> Result<Vec<Position>, String> {
    let account = db::accounts::find_by_id(&state.pool, &state.enc_key, &account_id)?;
    dispatch::get_positions(&account, &currency).await
}

#[tauri::command]
async fn get_all_open_orders(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<Vec<Order>, String> {
    let account = db::accounts::find_by_id(&state.pool, &state.enc_key, &account_id)?;
    dispatch::get_all_open_orders(&account).await
}

#[tauri::command]
async fn get_trade_history(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    account_id: String,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
) -> Result<Vec<Trade>, String> {
    let account = db::accounts::find_by_id(&state.pool, &state.enc_key, &account_id)?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let s = start_ms.unwrap_or(0);
    let e = end_ms.unwrap_or(now_ms);
    let mut trades = dispatch::get_trade_history(&account, s, e).await?;
    // Stamp account info
    for t in &mut trades {
        t.account_id   = account.id.clone();
        t.account_name = account.name.clone();
    }
    // Append to CSV file
    if !trades.is_empty() {
        if let Ok(data_dir) = app.path().app_data_dir() {
            let _ = append_trades_to_csv(&data_dir, &trades);
        }
    }
    Ok(trades)
}

#[tauri::command]
async fn get_transaction_log(
    state: tauri::State<'_, AppState>,
    account_id: String,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
) -> Result<Vec<TransactionLog>, String> {
    let account = db::accounts::find_by_id(&state.pool, &state.enc_key, &account_id)?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let s = start_ms.unwrap_or(now_ms - 30 * 24 * 60 * 60 * 1000); // default: last 30 days
    let e = end_ms.unwrap_or(now_ms);
    dispatch::get_transaction_log(&account, s, e).await
}

#[tauri::command]
fn get_trade_log_path(app: tauri::AppHandle) -> Result<String, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(data_dir.join("trades.csv").to_string_lossy().to_string())
}

/// Append trades to a CSV file, creating it with headers if it doesn't exist.
fn append_trades_to_csv(dir: &std::path::Path, trades: &[Trade]) -> Result<(), String> {
    use std::io::Write;
    let path = dir.join("trades.csv");
    let need_header = !path.exists();
    let mut f = std::fs::OpenOptions::new()
        .create(true).append(true).open(&path)
        .map_err(|e| e.to_string())?;
    if need_header {
        writeln!(f, "timestamp,account_id,account_name,exchange,instrument,direction,amount,price,fee,fee_currency,trade_id,order_id")
            .map_err(|e| e.to_string())?;
    }
    for t in trades {
        writeln!(f, "{},{},{},{},{},{},{},{},{},{},{},{}",
            t.timestamp, t.account_id, t.account_name, t.exchange,
            t.instrument_name, t.direction, t.amount, t.price,
            t.fee, t.fee_currency, t.trade_id, t.order_id)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ── WebSocket Commands ─────────────────────────────────────────────────────

#[tauri::command]
async fn ws_connect(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<(), String> {
    let account = db::accounts::find_by_id(&state.pool, &state.enc_key, &account_id)?;

    if state.ws_manager.is_connected(&account_id) {
        return Ok(()); // already connected
    }

    match account.exchange.as_str() {
        "deribit" => {
            ws::deribit::spawn(
                app,
                state.ws_manager.clone(),
                account.id,
                account.api_key,
                account.api_secret,
                account.testnet,
            );
        }
        "bybit" => {
            ws::bybit::spawn(
                app,
                state.ws_manager.clone(),
                account.id,
                account.api_key,
                account.api_secret,
                account.testnet,
            );
        }
        "okx" => {
            let passphrase = account.passphrase.clone().unwrap_or_default();
            ws::okx::spawn(
                app,
                state.ws_manager.clone(),
                account.id,
                account.api_key,
                account.api_secret,
                passphrase,
                account.testnet,
            );
        }
        "coincall" => {
            ws::coincall::spawn(
                app,
                state.ws_manager.clone(),
                account.id,
                account.api_key,
                account.api_secret,
                account.testnet,
            );
        }
        "binance" => {
            ws::binance::spawn(
                app,
                state.ws_manager.clone(),
                account.id,
                account.api_key,
                account.api_secret,
                account.testnet,
            );
        }
        "mexc" => {
            ws::mexc::spawn(
                app,
                state.ws_manager.clone(),
                account.id,
                account.api_key,
                account.api_secret,
                account.testnet,
            );
        }
        "hyperliquid" => {
            let wallet = account.api_key.clone();
            let private_key = account.api_secret.clone();
            let testnet = account.testnet;
            let account_id = account.id.clone();
            let ws_mgr = state.ws_manager.clone();
            let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel::<ws::WsCommand>(8);
            let (status_tx, mut status_rx) = tokio::sync::mpsc::channel::<ws::WsStatus>(16);
            ws_mgr.register(ws::WsHandle {
                account_id: account_id.clone(),
                exchange: "hyperliquid".to_string(),
                status: ws::WsStatus::Connecting,
                cmd_tx,
            });
            let app_for_status = app.clone();
            let mgr_clone = ws_mgr.clone();
            let aid_clone = account_id.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(s) = status_rx.recv().await {
                    let status_str = match &s {
                        ws::WsStatus::Connected           => "connected",
                        ws::WsStatus::Disconnected        => "disconnected",
                        ws::WsStatus::Connecting          => "connecting",
                        ws::WsStatus::Reconnecting { .. } => "reconnecting",
                        ws::WsStatus::Error(_)            => "error",
                    }.to_string();
                    mgr_clone.set_status(&aid_clone, s);
                    let _ = app_for_status.emit("ws://connection", ws::WsConnectionEvent {
                        account_id: aid_clone.clone(),
                        exchange: "hyperliquid".to_string(),
                        status: status_str,
                        message: None,
                    });
                }
            });
            tauri::async_runtime::spawn(ws::hyperliquid::run(app, account_id, wallet, testnet, status_tx));
        }
        "bullish" => {
            let trading_account_id = account.passphrase.clone().unwrap_or_default();
            ws::bullish::spawn(
                app,
                state.ws_manager.clone(),
                account.id,
                account.api_key,
                account.api_secret,
                trading_account_id,
            );
        }
        ex => return Err(format!("WebSocket not yet supported for {}", ex)),
    }
    Ok(())
}

#[tauri::command]
async fn ws_disconnect(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<(), String> {
    state.ws_manager.disconnect(&account_id);
    Ok(())
}

#[tauri::command]
fn ws_status(state: tauri::State<'_, AppState>) -> Vec<WsStatusSnapshot> {
    state.ws_manager.status_all()
}

// ── Rate Limiter Status ────────────────────────────────────────────────────

#[tauri::command]
fn get_rate_limiter_status(state: tauri::State<'_, AppState>) -> RateLimiterStatus {
    state.rate_limiter.status()
}

// ── Venue Settings ─────────────────────────────────────────────────────────

#[tauri::command]
fn get_venue_settings(state: tauri::State<'_, AppState>) -> Result<Vec<db::venue_settings::VenueSettings>, String> {
    db::venue_settings::get_all(&state.pool)
}

#[tauri::command]
fn get_venue_settings_for(state: tauri::State<'_, AppState>, exchange: String) -> Result<db::venue_settings::VenueSettings, String> {
    db::venue_settings::get_one(&state.pool, &exchange)
}

#[tauri::command]
fn save_venue_settings(state: tauri::State<'_, AppState>, settings: db::venue_settings::VenueSettings) -> Result<(), String> {
    db::venue_settings::save(&state.pool, &settings)
}

#[tauri::command]
fn delete_venue_settings(state: tauri::State<'_, AppState>, exchange: String) -> Result<(), String> {
    db::venue_settings::delete(&state.pool, &exchange)
}

// ── Telegram Settings ──────────────────────────────────────────────────────

#[tauri::command]
fn get_telegram_settings(state: tauri::State<'_, AppState>) -> Result<TelegramSettings, String> {
    db::settings::get_telegram(&state.pool, &state.enc_key)
}

#[tauri::command]
fn save_telegram_settings(
    state: tauri::State<'_, AppState>,
    settings: TelegramSettings,
) -> Result<(), String> {
    db::settings::save_telegram(&state.pool, &state.enc_key, &settings)
}

// ── Telegram API Commands ──────────────────────────────────────────────────

#[tauri::command]
async fn telegram_validate(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let cfg = db::settings::get_telegram(&state.pool, &state.enc_key)?;
    if cfg.bot_token.is_empty() { return Err("No bot token configured".to_string()); }
    telegram::validate_token(&cfg.bot_token).await
}

/// Load persisted known chats from the DB (no network call).
#[tauri::command]
fn telegram_get_known_chats(state: tauri::State<'_, AppState>) -> Result<Vec<TelegramChat>, String> {
    db::settings::get_known_chats(&state.pool)
}

/// Poll getUpdates, merge new chats into the DB, and return the full persisted list.
#[tauri::command]
async fn telegram_sync_chats(state: tauri::State<'_, AppState>) -> Result<Vec<TelegramChat>, String> {
    eprintln!("[telegram_sync_chats] command invoked");
    let cfg = db::settings::get_telegram(&state.pool, &state.enc_key)?;
    if cfg.bot_token.is_empty() {
        eprintln!("[telegram_sync_chats] ERROR: no bot token configured");
        return Err("No bot token configured".to_string());
    }
    eprintln!("[telegram_sync_chats] bot token present, calling get_recent_chats");
    let fresh = telegram::get_recent_chats(&cfg.bot_token).await?;
    eprintln!("[telegram_sync_chats] got {} chats from Telegram, upserting to DB", fresh.len());
    for chat in &fresh {
        eprintln!("[telegram_sync_chats] upsert chat id={} title={:?} kind={}", chat.id, chat.title, chat.kind);
        let _ = db::settings::upsert_known_chat(&state.pool, chat);
    }
    let all = db::settings::get_known_chats(&state.pool)?;
    eprintln!("[telegram_sync_chats] returning {} total known chats from DB", all.len());
    Ok(all)
}

/// Resolve a @username or numeric ID, persist it, and return it.
#[tauri::command]
async fn telegram_resolve_chat(
    state: tauri::State<'_, AppState>,
    chat_ref: String,
) -> Result<TelegramChat, String> {
    let cfg = db::settings::get_telegram(&state.pool, &state.enc_key)?;
    if cfg.bot_token.is_empty() { return Err("No bot token configured".to_string()); }
    let chat = telegram::resolve_chat(&cfg.bot_token, &chat_ref).await?;
    let _ = db::settings::upsert_known_chat(&state.pool, &chat);
    Ok(chat)
}

/// Remove a chat from the known list.
#[tauri::command]
fn telegram_delete_known_chat(state: tauri::State<'_, AppState>, chat_id: i64) -> Result<(), String> {
    db::settings::delete_known_chat(&state.pool, chat_id)
}

#[tauri::command]
async fn telegram_get_chats(state: tauri::State<'_, AppState>) -> Result<Vec<TelegramChat>, String> {
    // Kept for backwards compatibility — same as sync
    let cfg = db::settings::get_telegram(&state.pool, &state.enc_key)?;
    if cfg.bot_token.is_empty() { return Err("No bot token configured".to_string()); }
    let fresh = telegram::get_recent_chats(&cfg.bot_token).await?;

    for chat in &fresh {
        let _ = db::settings::upsert_known_chat(&state.pool, chat);
    }
    db::settings::get_known_chats(&state.pool)
}

#[tauri::command]
async fn telegram_send_message(
    state: tauri::State<'_, AppState>,
    chat_id: String,
    text: String,
    parse_mode: String,
    disable_preview: bool,
) -> Result<TelegramResult, String> {
    let cfg = db::settings::get_telegram(&state.pool, &state.enc_key)?;
    if cfg.bot_token.is_empty() { return Err("No bot token configured".to_string()); }
    telegram::send_message(&cfg.bot_token, &chat_id, &text, &parse_mode, disable_preview).await
}

#[tauri::command]
async fn telegram_send_photo(
    state: tauri::State<'_, AppState>,
    chat_id: String,
    file_path: String,
    caption: String,
    parse_mode: String,
) -> Result<TelegramResult, String> {
    let cfg = db::settings::get_telegram(&state.pool, &state.enc_key)?;
    if cfg.bot_token.is_empty() { return Err("No bot token configured".to_string()); }
    telegram::send_photo(&cfg.bot_token, &chat_id, &file_path, &caption, &parse_mode).await
}

#[tauri::command]
async fn telegram_send_document(
    state: tauri::State<'_, AppState>,
    chat_id: String,
    file_path: String,
    caption: String,
    parse_mode: String,
) -> Result<TelegramResult, String> {
    let cfg = db::settings::get_telegram(&state.pool, &state.enc_key)?;
    if cfg.bot_token.is_empty() { return Err("No bot token configured".to_string()); }
    telegram::send_document(&cfg.bot_token, &chat_id, &file_path, &caption, &parse_mode).await
}

// ── Broadcasts ─────────────────────────────────────────────────────────────

#[tauri::command]
async fn telegram_create_broadcast(
    state: tauri::State<'_, AppState>,
    req: CreateBroadcastRequest,
) -> Result<Broadcast, String> {
    use uuid::Uuid;
    let broadcast_id = Uuid::new_v4().to_string();

    db::broadcasts::create_broadcast(
        &state.pool,
        &broadcast_id,
        &req.subject,
        &req.text_body,
        &req.parse_mode,
        &req.recip_type,
        &req.recip_value,
    )?;

    // Add text body as first part (if non-empty)
    let mut sort = 0i32;
    if !req.text_body.trim().is_empty() {
        let part = BroadcastPart {
            id: Uuid::new_v4().to_string(),
            broadcast_id: broadcast_id.clone(),
            part_type: "text".to_string(),
            file_path: String::new(),
            caption: req.text_body.clone(),
            sort_order: sort,
        };
        db::broadcasts::add_part(&state.pool, &part)?;
        sort += 1;
    }

    // Add attachments as subsequent parts
    for att in &req.attachments {
        let part = BroadcastPart {
            id: Uuid::new_v4().to_string(),
            broadcast_id: broadcast_id.clone(),
            part_type: att.kind.clone(),
            file_path: att.file_path.clone(),
            caption: att.caption.clone(),
            sort_order: sort,
        };
        db::broadcasts::add_part(&state.pool, &part)?;
        sort += 1;
    }

    // Resolve recipients and create send records
    let recipients = telegram::resolve_recipients(&state.pool, &req.recip_type, &req.recip_value)?;
    if recipients.is_empty() {
        return Err("No recipients found. Check client Telegram chats are configured.".to_string());
    }
    let parts = db::broadcasts::get_parts(&state.pool, &broadcast_id)?;
    for (chat_id, client_name) in &recipients {
        for part in &parts {
            let send = BroadcastSend {
                id:            Uuid::new_v4().to_string(),
                broadcast_id:  broadcast_id.clone(),
                part_id:       part.id.clone(),
                chat_id:       *chat_id,
                client_name:   client_name.clone(),
                status:        "pending".to_string(),
                error_msg:     String::new(),
                message_id:    None,
                attempt_count: 0,
                last_attempt:  0,
            };
            db::broadcasts::create_send(&state.pool, &send)?;
        }
    }

    // Return the newly created broadcast with counts
    let broadcasts = db::broadcasts::get_broadcasts(&state.pool)?;
    broadcasts.into_iter()
        .find(|b| b.id == broadcast_id)
        .ok_or_else(|| "Broadcast not found after creation".to_string())
}

#[tauri::command]
async fn telegram_send_broadcast(
    state: tauri::State<'_, AppState>,
    broadcast_id: String,
) -> Result<(usize, usize), String> {
    let cfg = db::settings::get_telegram(&state.pool, &state.enc_key)?;
    if cfg.bot_token.is_empty() { return Err("No bot token configured".to_string()); }
    telegram::execute_broadcast(&state.pool, &cfg.bot_token, &broadcast_id).await
}

#[tauri::command]
async fn telegram_retry_failed(
    state: tauri::State<'_, AppState>,
    broadcast_id: String,
) -> Result<(usize, usize), String> {
    let cfg = db::settings::get_telegram(&state.pool, &state.enc_key)?;
    if cfg.bot_token.is_empty() { return Err("No bot token configured".to_string()); }
    // Reset failed sends to pending before retrying
    {
        let conn = state.pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE telegram_broadcast_sends SET status='pending', error_msg='' WHERE broadcast_id=?1 AND status='failed'",
            rusqlite::params![broadcast_id],
        ).map_err(|e| e.to_string())?;
    }
    telegram::execute_broadcast(&state.pool, &cfg.bot_token, &broadcast_id).await
}

#[tauri::command]
fn telegram_get_broadcasts(state: tauri::State<'_, AppState>) -> Result<Vec<Broadcast>, String> {
    db::broadcasts::get_broadcasts(&state.pool)
}

#[tauri::command]
fn telegram_get_broadcast_detail(
    state: tauri::State<'_, AppState>,
    broadcast_id: String,
) -> Result<(Broadcast, Vec<BroadcastPart>, Vec<BroadcastSend>), String> {
    let broadcasts = db::broadcasts::get_broadcasts(&state.pool)?;
    let broadcast = broadcasts.into_iter().find(|b| b.id == broadcast_id)
        .ok_or_else(|| "Broadcast not found".to_string())?;
    let parts = db::broadcasts::get_parts(&state.pool, &broadcast_id)?;
    let sends = db::broadcasts::get_sends(&state.pool, &broadcast_id)?;
    Ok((broadcast, parts, sends))
}

#[tauri::command]
fn telegram_delete_broadcast(
    state: tauri::State<'_, AppState>,
    broadcast_id: String,
) -> Result<(), String> {
    db::broadcasts::delete_broadcast(&state.pool, &broadcast_id)
}

// ── Tags CRUD ──────────────────────────────────────────────────────────────

#[tauri::command]
fn get_tags(state: tauri::State<'_, AppState>) -> Result<Vec<Tag>, String> {
    db::clients::get_tags(&state.pool)
}

#[tauri::command]
fn save_tag(state: tauri::State<'_, AppState>, tag: Tag) -> Result<Tag, String> {
    db::clients::upsert_tag(&state.pool, tag)
}

#[tauri::command]
fn delete_tag(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    db::clients::delete_tag(&state.pool, &id)
}

// ── Clients CRUD ───────────────────────────────────────────────────────────

#[tauri::command]
fn get_clients(state: tauri::State<'_, AppState>) -> Result<Vec<Client>, String> {
    db::clients::get_clients(&state.pool)
}

#[tauri::command]
fn save_client(state: tauri::State<'_, AppState>, client: Client) -> Result<Client, String> {
    db::clients::upsert_client(&state.pool, client)
}

#[tauri::command]
fn delete_client(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    db::clients::delete_client(&state.pool, &id)
}

// ── Client Telegram Chats ──────────────────────────────────────────────────

#[tauri::command]
fn get_client_chats(state: tauri::State<'_, AppState>, client_id: String) -> Result<Vec<ClientTelegramChat>, String> {
    db::clients::get_client_chats(&state.pool, &client_id)
}

#[tauri::command]
fn save_client_chat(state: tauri::State<'_, AppState>, chat: ClientTelegramChat) -> Result<ClientTelegramChat, String> {
    db::clients::upsert_client_chat(&state.pool, chat)
}

#[tauri::command]
fn delete_client_chat(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    db::clients::delete_client_chat(&state.pool, &id)
}

// ── Account ↔ Client links ─────────────────────────────────────────────────

#[tauri::command]
fn get_account_clients(state: tauri::State<'_, AppState>, account_id: String) -> Result<Vec<String>, String> {
    db::clients::get_account_clients(&state.pool, &account_id)
}

#[tauri::command]
fn set_account_clients(
    state: tauri::State<'_, AppState>,
    account_id: String,
    client_ids: Vec<String>,
) -> Result<(), String> {
    db::clients::set_account_clients(&state.pool, &account_id, &client_ids)
}

// ── CoInCall RFQ ──────────────────────────────────────────────────────────

use api::coincall::{RfqLeg, RfqResponse};

#[tauri::command]
async fn get_coincall_ws_url(
    state: tauri::State<'_, AppState>,
    account_id: String,
    kind: String,
) -> Result<String, String> {
    let account = db::accounts::find_by_id(&state.pool, &state.enc_key, &account_id)?;
    if account.exchange != "coincall" {
        return Err("Not a CoInCall account".to_string());
    }
    api::coincall::get_ws_url(&account, &kind)
}

#[tauri::command]
async fn coincall_get_rfq_list(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<serde_json::Value, String> {
    let accounts = db::accounts::get_all(&state.pool, &state.enc_key)?;
    let account = accounts.into_iter().find(|a| a.id == account_id)
        .ok_or_else(|| "Account not found".to_string())?;
    api::coincall::get_rfq_list(&account).await
}

#[tauri::command]
async fn coincall_create_rfq(
    state: tauri::State<'_, AppState>,
    account_id: String,
    legs: Vec<RfqLeg>,
) -> Result<RfqResponse, String> {
    let accounts = db::accounts::get_all(&state.pool, &state.enc_key)?;
    let account = accounts.into_iter().find(|a| a.id == account_id)
        .ok_or_else(|| "Account not found".to_string())?;
    if account.exchange != "coincall" {
        return Err("RFQ is only supported for CoInCall accounts".to_string());
    }
    api::coincall::create_rfq(legs, &account).await
}

#[tauri::command]
async fn coincall_cancel_rfq(
    state: tauri::State<'_, AppState>,
    account_id: String,
    request_id: String,
) -> Result<bool, String> {
    let accounts = db::accounts::get_all(&state.pool, &state.enc_key)?;
    let account = accounts.into_iter().find(|a| a.id == account_id)
        .ok_or_else(|| "Account not found".to_string())?;
    api::coincall::cancel_rfq(&request_id, &account).await
}

#[tauri::command]
async fn coincall_get_rfq_quotes(
    state: tauri::State<'_, AppState>,
    account_id: String,
    request_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let accounts = db::accounts::get_all(&state.pool, &state.enc_key)?;
    let account = accounts.into_iter().find(|a| a.id == account_id)
        .ok_or_else(|| "Account not found".to_string())?;
    api::coincall::get_rfq_quotes(&account, request_id.as_deref()).await
}

// ── Aggregated Orderbook ───────────────────────────────────────────────────

#[tauri::command]
async fn get_agg_book_configs(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<agg_book::AggBookConfig>, String> {
    db::agg_book_settings::get_all(&state.pool)
}

#[tauri::command]
async fn save_agg_book_config(
    state: tauri::State<'_, AppState>,
    config: agg_book::AggBookConfig,
    app: tauri::AppHandle,
) -> Result<agg_book::AggBookConfig, String> {
    let saved = db::agg_book_settings::upsert(&state.pool, &config)?;

    if saved.active {
        let accounts = db::accounts::get_all(&state.pool, &state.enc_key).unwrap_or_default();
        state.agg_book_manager.start(saved.clone(), accounts, app).await;
    } else {
        state.agg_book_manager.stop(&saved.id).await;
    }

    Ok(saved)
}

#[tauri::command]
async fn delete_agg_book_config(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    state.agg_book_manager.stop(&id).await;
    db::agg_book_settings::delete(&state.pool, &id)
}

#[tauri::command]
async fn start_agg_book(
    state: tauri::State<'_, AppState>,
    config_id: String,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let configs = db::agg_book_settings::get_all(&state.pool)?;
    let config = configs
        .into_iter()
        .find(|c| c.id == config_id)
        .ok_or_else(|| format!("AggBook config '{}' not found", config_id))?;
    let accounts = db::accounts::get_all(&state.pool, &state.enc_key).unwrap_or_default();
    state.agg_book_manager.start(config, accounts, app).await;
    Ok(())
}

#[tauri::command]
async fn stop_agg_book(
    state: tauri::State<'_, AppState>,
    config_id: String,
) -> Result<(), String> {
    state.agg_book_manager.stop(&config_id).await;
    Ok(())
}

// ── Reference Data ─────────────────────────────────────────────────────────

#[tauri::command]
async fn fetch_reference_data(
    exchange: String,
    currency: String,
    kind: String,
    testnet: Option<bool>,
) -> Result<Vec<ReferenceData>, String> {
    dispatch::fetch_reference_data(&exchange, &currency, &kind, testnet.unwrap_or(false)).await
}

// ── Market Data Subscriptions ──────────────────────────────────────────────

/// Subscribe to orderbook + ticker for a single instrument on an exchange.
/// For CoInCall, an `account_id` must be provided to generate the signed WS URL.
#[tauri::command]
async fn subscribe_market_data(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    exchange: String,
    exchange_symbol: String,
    symbol: String,
    kind: String,
    account_id: Option<String>,
) -> Result<(), String> {
    let ws_url = if exchange == "coincall" {
        let aid = account_id.ok_or("CoInCall requires account_id for market data")?;
        let account = db::accounts::find_by_id(&state.pool, &state.enc_key, &aid)?;
        Some(api::coincall::get_ws_url(&account, &kind)?)
    } else {
        None
    };

    let emit_interval_ms = db::settings::get_general(&state.pool)
        .unwrap_or_default()
        .book_emit_interval_ms
        .max(10); // floor at 10ms to prevent accidental 0 freeze

    state.market_manager.subscribe(app, exchange, exchange_symbol, symbol, kind, ws_url, emit_interval_ms).await;
    Ok(())
}

/// Unsubscribe from orderbook + ticker for a single instrument.
#[tauri::command]
async fn unsubscribe_market_data(
    state: tauri::State<'_, AppState>,
    exchange: String,
    exchange_symbol: String,
) -> Result<(), String> {
    state.market_manager.unsubscribe(&exchange, &exchange_symbol).await;
    Ok(())
}

// ── App Entry ──────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to resolve app data directory");
            std::fs::create_dir_all(&data_dir)
                .expect("Failed to create app data directory");

            let db_path  = data_dir.join("dashboard.db");
            let key_path = data_dir.join("encryption.key");

            let pool    = db::init_db(&db_path).expect("Failed to initialise database");
            let enc_key = db::load_enc_key(&key_path).expect("Failed to load encryption key");

            let agg_book_manager = agg_book::AggBookManager::new();

            // Start active agg book configs in background
            {
                let pool2    = pool.clone();
                let enc_key2 = enc_key;
                let mgr      = agg_book_manager.clone();
                let app_h    = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Ok(configs) = db::agg_book_settings::get_all(&pool2) {
                        let accounts = db::accounts::get_all(&pool2, &enc_key2).unwrap_or_default();
                        for cfg in configs {
                            if cfg.active {
                                mgr.start(cfg, accounts.clone(), app_h.clone()).await;
                            }
                        }
                    }
                });
            }

            app.manage(AppState {
                pool,
                enc_key,
                rate_limiter: rate_limiter::default_rate_limiter(),
                ws_manager: WsManager::new(),
                agg_book_manager,
                market_manager: MarketDataManager::new(),
            });

            // Sync exchange server times to compensate for local clock drift.
            tauri::async_runtime::spawn(async {
                tokio::join!(
                    api::bybit::sync_server_time(),
                    api::coincall::sync_server_time(),
                );
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_accounts,
            save_account,
            delete_account,
            get_general_settings,
            save_general_settings,
            get_client_info,
            save_client_info,
            get_telegram_settings,
            save_telegram_settings,
            telegram_validate,
            telegram_get_chats,
            telegram_sync_chats,
            telegram_get_known_chats,
            telegram_resolve_chat,
            telegram_delete_known_chat,
            telegram_send_message,
            telegram_send_photo,
            telegram_send_document,
            telegram_create_broadcast,
            telegram_send_broadcast,
            telegram_retry_failed,
            telegram_get_broadcasts,
            telegram_get_broadcast_detail,
            telegram_delete_broadcast,
            fetch_instruments,
            fetch_ticker,
            place_order,
            cancel_order,
            get_open_orders,
            get_all_open_orders,
            get_trade_history,
            get_transaction_log,
            get_trade_log_path,
            get_account_summary,
            get_positions,
            get_tags,
            save_tag,
            delete_tag,
            get_clients,
            save_client,
            delete_client,
            get_client_chats,
            save_client_chat,
            delete_client_chat,
            get_account_clients,
            set_account_clients,
            coincall_create_rfq,
            coincall_cancel_rfq,
            coincall_get_rfq_quotes,
            coincall_get_rfq_list,
            get_coincall_ws_url,
            ws_connect,
            ws_disconnect,
            ws_status,
            get_rate_limiter_status,
            get_venue_settings,
            get_venue_settings_for,
            save_venue_settings,
            delete_venue_settings,
            get_agg_book_configs,
            save_agg_book_config,
            delete_agg_book_config,
            start_agg_book,
            stop_agg_book,
            fetch_reference_data,
            subscribe_market_data,
            unsubscribe_market_data,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

