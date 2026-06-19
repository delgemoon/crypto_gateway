import { createSlice, PayloadAction } from '@reduxjs/toolkit';
import { RootState } from '../../store';

// ── Tag ────────────────────────────────────────────────────────────────────

export interface Tag {
  id: string;
  name: string;
  color: string;
}

// ── Client (multi-client) ──────────────────────────────────────────────────

export interface Client {
  id: string;
  companyName: string;
  contactName: string;
  phone: string;
  email: string;
  /** comma-separated tag ids */
  tagIds: string;
  notes: string;
}

export interface ClientTelegramChat {
  id: string;
  clientId: string;
  chatId: string;
  label: string;
}

// ── Account (Exchange) ─────────────────────────────────────────────────────

export interface Account {
  id: string;
  name: string;
  exchange: 'deribit' | 'okx' | 'bybit' | 'coincall' | 'binance' | 'mexc' | 'hyperliquid' | 'uniswap' | 'bullish';
  rpcUrl?: string;
  chainId?: number;
  apiKey: string;
  /** DeFi: private key; Uniswap Safe multisig supports comma-separated owner keys */
  apiSecret: string;
  /** Required for OKX */
  passphrase?: string;
  testnet: boolean;
  // Trading defaults per key
  defaultTif: 'good_til_cancelled' | 'immediate_or_cancel' | 'fill_or_kill';
  defaultPostOnly: boolean;
  riskLimit: number; // max notional per order (0 = no limit)
  /** Rate limit tier: tier1 | tier2 | vip1 … vip5 | market_maker */
  rateTier: string;
}

// ── RFQ / Pricer Settings ─────────────────────────────────────────────────

export interface RfqSettings {
  /** Annual risk-free rate as decimal (e.g. 0.05 = 5%) */
  riskFreeRate: number;
  /** Fallback implied vol when market data unavailable (e.g. 0.80 = 80%) */
  defaultVol: number;
  /** Exchange to pull both spot/index price and mark IV from */
  pricerExchange: 'deribit' | 'okx' | 'bybit' | 'coincall';
  /** Base half-spread around mid (e.g. 0.01 = 1% each side). Default 0.01. */
  baseSpread: number;
  /** How aggressively portfolio gamma skews the quote. Default 0.5. */
  gammaSensitivity: number;
  /** How aggressively portfolio vega skews the quote. Default 0.0005. */
  vegaSensitivity: number;
  /** Max Greek-based skew cap (e.g. 0.05 = ±5% of mid). Default 0.05. */
  maxSkew: number;
  /** Coin this account trades — filters incoming RFQs to only this coin. Default "BTC". */
  tradingCoin: string;
  /** Automatically price and submit a quote for every new incoming RFQ seek. Default false. */
  autoQuote: boolean;
  /** Seconds after which an auto-submitted quote is automatically cancelled. Default 30. */
  autoQuoteTimeoutSecs: number;
}

// ── General Settings ───────────────────────────────────────────────────────

export interface GeneralSettings {
  theme: 'dark' | 'light';
  defaultCurrency: string;
  numberLocale: string;
  priceDecimals: number;
  sizeDecimals: number;
  confirmOrders: boolean;
  /** Comma-separated coins to show in Account Summary (empty = show all) */
  watchedCoins: string;
  /** Bot/instance ID encoded into client order IDs (default = 1) */
  botId: number;
  /** How often the backend emits book/ticker events to the frontend (ms, default 80) */
  bookEmitIntervalMs: number;
  /** Maximum number of orderbook widgets on the trading dashboard (default 4) */
  maxDashboardWidgets: number;
}

// ── Client Info ────────────────────────────────────────────────────────────

export interface ClientInfo {
  companyName: string;
  contactName: string;
  phone: string;
  email: string;
  telegramHandle: string;
  tags: string; // comma-separated
  notes: string;
}

// ── Telegram Settings ──────────────────────────────────────────────────────

export interface TelegramSettings {
  botToken: string;
  defaultChatId: string;
}

export interface TelegramChat {
  id: number;
  kind: string;
  title?: string;
  username?: string;
}

// ── State ──────────────────────────────────────────────────────────────────

interface SettingsState {
  tags: Tag[];
  clients: Client[];
  accounts: Account[];
  activeAccountId: string | null;
  general: GeneralSettings;
  rfq: RfqSettings;
  client: ClientInfo;
  telegram: TelegramSettings;
}

const initialState: SettingsState = {
  tags: [],
  clients: [],
  accounts: [],
  activeAccountId: null,
  general: {
    theme: 'dark',
    defaultCurrency: 'BTC',
    numberLocale: 'en-US',
    priceDecimals: 2,
    sizeDecimals: 4,
    confirmOrders: true,
    watchedCoins: '',
    botId: 1,
    bookEmitIntervalMs: 80,
    maxDashboardWidgets: 4,
  },
  rfq: {
    riskFreeRate: 0.05,
    defaultVol: 0.80,
    pricerExchange: 'deribit',
    baseSpread: 0.01,
    gammaSensitivity: 0.5,
    vegaSensitivity: 0.0005,
    maxSkew: 0.05,
    tradingCoin: 'BTC',
    autoQuote: false,
    autoQuoteTimeoutSecs: 30,
  },
  client: {
    companyName: '',
    contactName: '',
    phone: '',
    email: '',
    telegramHandle: '',
    tags: '',
    notes: '',
  },
  telegram: {
    botToken: '',
    defaultChatId: '',
  },
};

export const settingsSlice = createSlice({
  name: 'settings',
  initialState,
  reducers: {
    // Tags
    setTags(state, { payload }: PayloadAction<Tag[]>) {
      state.tags = payload;
    },
    upsertTag(state, { payload }: PayloadAction<Tag>) {
      const idx = state.tags.findIndex(t => t.id === payload.id);
      if (idx >= 0) state.tags[idx] = payload; else state.tags.push(payload);
    },
    removeTag(state, { payload }: PayloadAction<string>) {
      state.tags = state.tags.filter(t => t.id !== payload);
    },
    // Clients
    setClients(state, { payload }: PayloadAction<Client[]>) {
      state.clients = payload;
    },
    upsertClient(state, { payload }: PayloadAction<Client>) {
      const idx = state.clients.findIndex(c => c.id === payload.id);
      if (idx >= 0) state.clients[idx] = payload; else state.clients.push(payload);
    },
    removeClient(state, { payload }: PayloadAction<string>) {
      state.clients = state.clients.filter(c => c.id !== payload);
    },
    // Exchange accounts
    setAccounts(state, { payload }: PayloadAction<Account[]>) {
      state.accounts = payload;
      if (!state.activeAccountId && payload.length > 0) {
        state.activeAccountId = payload[0].id;
      }
    },
    upsertAccount(state, { payload }: PayloadAction<Account>) {
      const idx = state.accounts.findIndex((a) => a.id === payload.id);
      if (idx >= 0) {
        state.accounts[idx] = payload;
      } else {
        state.accounts.push(payload);
      }
      if (!state.activeAccountId) {
        state.activeAccountId = payload.id;
      }
    },
    removeAccount(state, { payload }: PayloadAction<string>) {
      state.accounts = state.accounts.filter((a) => a.id !== payload);
      if (state.activeAccountId === payload) {
        state.activeAccountId = state.accounts[0]?.id ?? null;
      }
    },
    setActiveAccount(state, { payload }: PayloadAction<string>) {
      state.activeAccountId = payload;
    },
    // General settings
    setGeneral(state, { payload }: PayloadAction<Partial<GeneralSettings>>) {
      state.general = { ...state.general, ...payload };
    },
    // RFQ / pricer settings
    setRfqSettings(state, { payload }: PayloadAction<Partial<RfqSettings>>) {
      state.rfq = { ...state.rfq, ...payload };
    },
    // Client info
    setClient(state, { payload }: PayloadAction<Partial<ClientInfo>>) {
      state.client = { ...state.client, ...payload };
    },
    // Telegram settings
    setTelegram(state, { payload }: PayloadAction<Partial<TelegramSettings>>) {
      state.telegram = { ...state.telegram, ...payload };
    },
  },
});

export const {
  setTags, upsertTag, removeTag,
  setClients, upsertClient, removeClient,
  setAccounts, upsertAccount, removeAccount, setActiveAccount,
  setGeneral, setRfqSettings, setClient, setTelegram,
} = settingsSlice.actions;

export const selectTags     = (state: RootState) => state.settings.tags;
export const selectClients  = (state: RootState) => state.settings.clients;

export const selectAccounts = (state: RootState) => state.settings.accounts;
export const selectActiveAccountId = (state: RootState) => state.settings.activeAccountId;
export const selectActiveAccount = (state: RootState) =>
  state.settings.accounts.find((a) => a.id === state.settings.activeAccountId) ?? null;
export const selectGeneral = (state: RootState) => state.settings.general;
export const selectRfqSettings = (state: RootState) => state.settings.rfq;
export const selectClient = (state: RootState) => state.settings.client;
export const selectTelegram = (state: RootState) => state.settings.telegram;

export default settingsSlice.reducer;
