import { createSlice, PayloadAction } from '@reduxjs/toolkit';
import { RootState } from '../../store';

// ── Types ──────────────────────────────────────────────────────────────────

export interface AggBookConfig {
  id: string;
  name: string;
  baseSymbol: string;
  instrumentKind: string;
  accountIds: string[];
  unifyQuote: boolean;
  maxLevels: number;
  tickSize: number | null;
  pollIntervalMs: number;
  active: boolean;
}

export interface AggContribution {
  exchange: string;
  size: number;
  // accountId and instrumentName removed from IPC payload — use exchangeStatus for per-account info
}

export interface AggLevel {
  price: number;
  totalSize: number;
  contributions: AggContribution[];
}

export interface AggBookSnapshot {
  configId: string;
  name: string;
  baseSymbol: string;
  instrumentKind: string;
  bids: AggLevel[];
  asks: AggLevel[];
  exchangeStatus: Record<string, string>;
  timestamp: number;
}

// ── State ──────────────────────────────────────────────────────────────────

interface AggBookState {
  configs: AggBookConfig[];
  snapshots: Record<string, AggBookSnapshot>;
}

const initialState: AggBookState = {
  configs: [],
  snapshots: {},
};

// ── Slice ──────────────────────────────────────────────────────────────────

export const aggBookSlice = createSlice({
  name: 'aggBook',
  initialState,
  reducers: {
    setConfigs(state, { payload }: PayloadAction<AggBookConfig[]>) {
      state.configs = payload;
    },
    upsertConfig(state, { payload }: PayloadAction<AggBookConfig>) {
      const idx = state.configs.findIndex(c => c.id === payload.id);
      if (idx >= 0) state.configs[idx] = payload;
      else state.configs.push(payload);
    },
    removeConfig(state, { payload }: PayloadAction<string>) {
      state.configs = state.configs.filter(c => c.id !== payload);
      delete state.snapshots[payload];
    },
    setSnapshot(state, { payload }: PayloadAction<AggBookSnapshot>) {
      state.snapshots[payload.configId] = payload;
    },
  },
});

export const { setConfigs, upsertConfig, removeConfig, setSnapshot } = aggBookSlice.actions;

export const selectAggBookConfigs = (state: RootState) => state.aggBook.configs;
export const selectAggBookSnapshots = (state: RootState) => state.aggBook.snapshots;
export const selectAggBookSnapshot = (configId: string) => (state: RootState) =>
  state.aggBook.snapshots[configId];

export default aggBookSlice.reducer;
