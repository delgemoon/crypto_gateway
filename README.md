# Order Dashboard

A cross-exchange crypto trading dashboard built with **Tauri 2 + React 19 + TypeScript + Rust**.

---

## Overview

Order Dashboard is a desktop trading application that connects to multiple crypto exchanges for order management, position tracking, and real-time market data. The Rust backend handles all network I/O (REST + WebSocket) while the React frontend provides a responsive trading interface.

---

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Desktop shell | Tauri 2 |
| Frontend | React 19, TypeScript 5.8, Vite 7 |
| State management | Redux Toolkit (RTK) |
| Styling | styled-components 6 |
| Backend | Rust (tokio, tokio-tungstenite, reqwest, serde_json) |
| Database | SQLite via rusqlite + r2d2 |
| Encryption | AES-GCM (aes-gcm 0.10) |

---

## Supported Exchanges

| Exchange | Spot | Futures | Options | WebSocket (private) | Market data WS |
|----------|------|---------|---------|----------------------|----------------|
| Deribit | — | ✅ | ✅ | ✅ | ✅ |
| OKX | ✅ | ✅ | ✅ | ✅ | ✅ |
| Bybit | ✅ | ✅ | ✅ | ✅ | ✅ |
| CoInCall | — | ✅ | ✅ | ✅ | ✅ |
| Binance | ✅ | ✅ | — | ✅ | — |
| MEXC | ✅ | ✅ | — | ✅ | — |
| Hyperliquid | — | ✅ | — | ✅ | — |
| Uniswap (EVM) | ✅ | — | — | — | — |

---

## Features

### Trading Dashboard
- Per-account exchange panels with live orderbook and ticker
- Custom div-based dropdowns (no native `<select>`) — immune to WebView2 dropdown-closure bug
- Instrument selectors: Base currency → Kind → Quote → Expiry → Strike / Call/Put
- Click any orderbook row to pre-fill price + side in the order form
- 100 ms-throttled WS updates isolated in a `React.memo` child — parent selectors never re-render from ticks

### Order Management
- Place limit/market orders with configurable TIF and post-only
- Cancel individual orders or all open orders
- Open orders table with real-time updates via private WebSocket
- Trade history export to CSV

### Positions & Account Summary
- Live position tracking with PnL, delta, gamma, vega, theta
- Account equity and margin summary per currency

### Reference Data (canonical instrument model)
- `ReferenceData` struct: single canonical instrument shared across exchanges
- Canonical symbol format (all uppercase):
  - Token: `BTC`
  - Spot: `BTC-USDT`
  - Perpetual: `BTC-USD-PERPETUAL`
  - Dated future: `BTC-USD-20240329`
  - Option: `BTC-USD-50000-20240329-C`
- `VenueRef` per exchange: exchange symbol, tick size, min trade amount, settlement currency
- Tauri command: `fetch_reference_data(exchange, currency, kind)` → `Vec<ReferenceData>`

### Backend Market Data (public WebSocket)
- `MarketDataManager` in `src-tauri/src/market/`: manages per-instrument WS tasks
- Emits Tauri events to frontend:
  - `market://book` → `MarketBookEvent { symbol, exchange, bids, asks, timestamp }`
  - `market://ticker` → `MarketTickerEvent { symbol, exchange, last, mark, delta, … }`
- Per-exchange implementations: Deribit (incremental book), OKX (books channel), Bybit (routed by kind), CoInCall (signed URL, dt codes)
- Tauri commands: `subscribe_market_data`, `unsubscribe_market_data`

### Aggregated Orderbook
- Multi-exchange aggregated book view
- Configurable per-instrument, per-account configs stored in SQLite

### RFQ (CoInCall)
- Create, cancel, and monitor RFQ requests
- Real-time quote streaming via CoInCall WebSocket

### Telegram Integration
- Send messages, photos, and documents to Telegram chats
- Broadcast system: multi-recipient blasts with attachments
- Client management with tag-based recipient groups

### Settings
- Exchange account management (API key/secret encrypted at rest)
- Venue-level fee/rate settings
- General settings: theme, default currency, number format, bot ID
- Rate limiter with per-exchange tier configuration

---


## Development Setup

### Prerequisites
- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) stable toolchain
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)

### Install dependencies
```bash
npm install          # or yarn
```

### Run in development mode
```bash
npm run tauri dev    # starts Vite + Tauri dev window
```

### Build for production
```bash
npm run tauri build
```

---

## Recommended IDE

- [VS Code](https://code.visualstudio.com/)
  - [Tauri extension](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode)
  - [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
  - [ESLint](https://marketplace.visualstudio.com/items?itemName=dbaeumer.vscode-eslint)
  - [Pylance](https://marketplace.visualstudio.com/items?itemName=ms-python.vscode-pylance) (optional)

---

## Architecture Notes

### WebView2 dropdown fix
Native `<select>` popups in Tauri on Windows use a Win32 child window. When open, Win32 focus leaves the WebView, making `document.activeElement` null inside the page. Any React state update (re-render) causes WebView2 to close the popup. All exchange panel dropdowns use `TinySelect` — a fully custom React div-based component rendered via a portal — which is immune to this.

### WS data throttle
High-frequency WebSocket ticks (50+ msg/sec) write to ref buffers (`pendingBidsRef`, etc.) with zero re-renders. A `setInterval(100 ms)` is the only thing that calls `setState`, capping renders at ≤10/sec. The book+ticker display is extracted into a `React.memo` child so WS re-renders never touch the parent's selector controls.

### Canonical symbols
The backend uses a unified `ReferenceData` model for all instruments. Each exchange's raw instrument is normalized at fetch time via `instrument_to_ref()` in `api/models.rs`. This gives the frontend a single symbol vocabulary regardless of which exchange is queried.

### Encryption
API secrets are encrypted at rest using AES-256-GCM with a per-installation key stored in the app data directory.

