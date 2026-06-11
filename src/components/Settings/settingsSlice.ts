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
  setGeneral, setClient, setTelegram,
} = settingsSlice.actions;

export const selectTags     = (state: RootState) => state.settings.tags;
export const selectClients  = (state: RootState) => state.settings.clients;

export const selectAccounts = (state: RootState) => state.settings.accounts;
export const selectActiveAccountId = (state: RootState) => state.settings.activeAccountId;
export const selectActiveAccount = (state: RootState) =>
  state.settings.accounts.find((a) => a.id === state.settings.activeAccountId) ?? null;
export const selectGeneral = (state: RootState) => state.settings.general;
export const selectClient = (state: RootState) => state.settings.client;
export const selectTelegram = (state: RootState) => state.settings.telegram;

export default settingsSlice.reducer;
