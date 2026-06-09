pub mod accounts;
pub mod agg_book_settings;
pub mod broadcasts;
pub mod clients;
pub mod crypto;
pub mod settings;
pub mod venue_settings;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rand::RngCore;
use std::path::Path;

pub type DbPool = Pool<SqliteConnectionManager>;

const MIGRATIONS: &str = "
CREATE TABLE IF NOT EXISTS accounts (
    id                TEXT    PRIMARY KEY,
    name              TEXT    NOT NULL,
    exchange          TEXT    NOT NULL,
    api_key_enc       TEXT    NOT NULL,
    api_secret_enc    TEXT    NOT NULL,
    passphrase_enc    TEXT,
    testnet           INTEGER NOT NULL DEFAULT 0,
    default_tif       TEXT    NOT NULL DEFAULT 'good_til_cancelled',
    default_post_only INTEGER NOT NULL DEFAULT 0,
    risk_limit        REAL    NOT NULL DEFAULT 0.0,
    rate_tier         TEXT    NOT NULL DEFAULT 'tier1',
    rpc_url           TEXT,
    chain_id          INTEGER
);

CREATE TABLE IF NOT EXISTS general_settings (
    id               INTEGER PRIMARY KEY CHECK(id = 1),
    theme            TEXT    NOT NULL DEFAULT 'dark',
    default_currency TEXT    NOT NULL DEFAULT 'BTC',
    number_locale    TEXT    NOT NULL DEFAULT 'en-US',
    price_decimals   INTEGER NOT NULL DEFAULT 2,
    size_decimals    INTEGER NOT NULL DEFAULT 4,
    confirm_orders   INTEGER NOT NULL DEFAULT 1,
    watched_coins    TEXT    NOT NULL DEFAULT '',
    bot_id           INTEGER NOT NULL DEFAULT 1
);

-- Legacy single-row client table (kept for migration safety, no longer written)
CREATE TABLE IF NOT EXISTS client_info (
    id              INTEGER PRIMARY KEY CHECK(id = 1),
    company_name    TEXT    NOT NULL DEFAULT '',
    contact_name    TEXT    NOT NULL DEFAULT '',
    phone           TEXT    NOT NULL DEFAULT '',
    email           TEXT    NOT NULL DEFAULT '',
    telegram_handle TEXT    NOT NULL DEFAULT '',
    tags            TEXT    NOT NULL DEFAULT '',
    notes           TEXT    NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS telegram_settings (
    id              INTEGER PRIMARY KEY CHECK(id = 1),
    bot_token_enc   TEXT    NOT NULL DEFAULT '',
    default_chat_id TEXT    NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS telegram_known_chats (
    id      INTEGER PRIMARY KEY,   -- Telegram chat id (can be negative)
    kind    TEXT    NOT NULL DEFAULT '',
    title   TEXT    NOT NULL DEFAULT '',
    username TEXT   NOT NULL DEFAULT '',
    label   TEXT    NOT NULL DEFAULT ''  -- user-defined friendly name
);

-- Telegram broadcast jobs
CREATE TABLE IF NOT EXISTS telegram_broadcasts (
    id          TEXT    PRIMARY KEY,
    subject     TEXT    NOT NULL DEFAULT '',
    text_body   TEXT    NOT NULL DEFAULT '',
    parse_mode  TEXT    NOT NULL DEFAULT 'HTML',
    recip_type  TEXT    NOT NULL DEFAULT 'group',
    recip_value TEXT    NOT NULL DEFAULT '',
    status      TEXT    NOT NULL DEFAULT 'pending',
    created_at  INTEGER NOT NULL DEFAULT 0
);

-- Each part of a broadcast: text body + each attachment
CREATE TABLE IF NOT EXISTS telegram_broadcast_parts (
    id           TEXT    PRIMARY KEY,
    broadcast_id TEXT    NOT NULL REFERENCES telegram_broadcasts(id) ON DELETE CASCADE,
    part_type    TEXT    NOT NULL DEFAULT 'text',
    file_path    TEXT    NOT NULL DEFAULT '',
    caption      TEXT    NOT NULL DEFAULT '',
    sort_order   INTEGER NOT NULL DEFAULT 0
);

-- One send record per (part × recipient chat)
CREATE TABLE IF NOT EXISTS telegram_broadcast_sends (
    id            TEXT    PRIMARY KEY,
    broadcast_id  TEXT    NOT NULL REFERENCES telegram_broadcasts(id) ON DELETE CASCADE,
    part_id       TEXT    NOT NULL REFERENCES telegram_broadcast_parts(id) ON DELETE CASCADE,
    chat_id       INTEGER NOT NULL,
    client_name   TEXT    NOT NULL DEFAULT '',
    status        TEXT    NOT NULL DEFAULT 'pending',
    error_msg     TEXT    NOT NULL DEFAULT '',
    message_id    INTEGER,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_attempt  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS tags (
    id    TEXT PRIMARY KEY,
    name  TEXT NOT NULL UNIQUE,
    color TEXT NOT NULL DEFAULT '#5087f2'
);

-- Multiple named clients (replaces single client_info for new usage)
CREATE TABLE IF NOT EXISTS clients (
    id           TEXT PRIMARY KEY,
    company_name TEXT NOT NULL DEFAULT '',
    contact_name TEXT NOT NULL DEFAULT '',
    phone        TEXT NOT NULL DEFAULT '',
    email        TEXT NOT NULL DEFAULT '',
    tag_ids      TEXT NOT NULL DEFAULT '',   -- comma-separated tag ids
    notes        TEXT NOT NULL DEFAULT ''
);

-- Each client can have multiple Telegram chat destinations
CREATE TABLE IF NOT EXISTS client_telegram_chats (
    id        TEXT PRIMARY KEY,
    client_id TEXT NOT NULL REFERENCES clients(id) ON DELETE CASCADE,
    chat_id   TEXT NOT NULL,
    label     TEXT NOT NULL DEFAULT ''
);

-- Junction: which clients are linked to which API key (account)
CREATE TABLE IF NOT EXISTS account_clients (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    client_id  TEXT NOT NULL REFERENCES clients(id)  ON DELETE CASCADE,
    PRIMARY KEY (account_id, client_id)
);

-- Per-exchange venue capability settings
CREATE TABLE IF NOT EXISTS venue_settings (
    exchange         TEXT PRIMARY KEY,
    instrument_types TEXT NOT NULL DEFAULT '[]',
    market_feeds     TEXT NOT NULL DEFAULT '[]',
    order_feeds      TEXT NOT NULL DEFAULT '[]',
    notes            TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS agg_book_configs (
    id               TEXT    PRIMARY KEY,
    name             TEXT    NOT NULL,
    base_symbol      TEXT    NOT NULL,
    instrument_kind  TEXT    NOT NULL,
    account_ids      TEXT    NOT NULL DEFAULT '[]',
    unify_quote      INTEGER NOT NULL DEFAULT 0,
    max_levels       INTEGER NOT NULL DEFAULT 20,
    tick_size        REAL,
    poll_interval_ms INTEGER NOT NULL DEFAULT 500,
    active           INTEGER NOT NULL DEFAULT 1
);
";

pub fn init_db(db_path: &Path) -> Result<DbPool, String> {
    let manager = SqliteConnectionManager::file(db_path).with_init(|conn| {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(())
    });
    let pool = Pool::builder()
        .max_size(4)
        .build(manager)
        .map_err(|e| e.to_string())?;

    let conn = pool.get().map_err(|e| e.to_string())?;
    conn.execute_batch(MIGRATIONS).map_err(|e| e.to_string())?;

    // Idempotent column additions for existing databases (ALTER TABLE ignores if column exists error)
    let _ = conn.execute(
        "ALTER TABLE general_settings ADD COLUMN watched_coins TEXT NOT NULL DEFAULT ''",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE general_settings ADD COLUMN bot_id INTEGER NOT NULL DEFAULT 1",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE general_settings ADD COLUMN book_emit_interval_ms INTEGER NOT NULL DEFAULT 80",
        [],
    );
    // Add rate_tier column to accounts (idempotent)
    let _ = conn.execute(
        "ALTER TABLE accounts ADD COLUMN rate_tier TEXT NOT NULL DEFAULT 'tier1'",
        [],
    );
    // Add DeFi fields to accounts (idempotent)
    let _ = conn.execute("ALTER TABLE accounts ADD COLUMN rpc_url TEXT", []);
    let _ = conn.execute("ALTER TABLE accounts ADD COLUMN chain_id INTEGER", []);
    // telegram_known_chats is created by MIGRATIONS above if missing; no ALTER needed

    Ok(pool)
}

/// Read existing key or generate + persist a new 32-byte AES key.
pub fn load_enc_key(key_path: &Path) -> Result<[u8; 32], String> {
    if key_path.exists() {
        let hex_str = std::fs::read_to_string(key_path).map_err(|e| e.to_string())?;
        let bytes = hex::decode(hex_str.trim()).map_err(|e| e.to_string())?;
        if bytes.len() != 32 {
            return Err("encryption.key has wrong length (expected 32 bytes)".to_string());
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(key)
    } else {
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        std::fs::write(key_path, hex::encode(key)).map_err(|e| e.to_string())?;
        Ok(key)
    }
}
