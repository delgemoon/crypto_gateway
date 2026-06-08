import { createSlice, PayloadAction } from '@reduxjs/toolkit';
import { RootState } from '../../store';

export interface Instrument {
  instrument_name: string;
  kind: string;
  base_currency: string;
  quote_currency: string;
  settlement_currency: string;
  is_active: boolean;
  tick_size: number;
  min_trade_amount: number;
  contract_size?: number;
  option_type?: string;
  strike?: number;
  expiration_timestamp?: number;
}

export interface TickerStats {
  high?: number;
  low?: number;
  price_change?: number;
  volume?: number;
  volume_usd?: number;
}

export interface Ticker {
  instrument_name: string;
  best_bid_price?: number;
  best_ask_price?: number;
  best_bid_amount?: number;
  best_ask_amount?: number;
  last_price?: number;
  mark_price?: number;
  index_price?: number;
  open_interest?: number;
  stats: TickerStats;
  mark_iv?: number;
  bid_iv?: number;
  ask_iv?: number;
  delta?: number;
  gamma?: number;
  vega?: number;
  theta?: number;
}

interface InstrumentsState {
  currency: string;
  kind: string;
  instruments: Instrument[];
  selectedInstrument: string;
  ticker: Ticker | null;
  loading: boolean;
  error: string | null;
  priceFromBook: { price: number; side: 'buy' | 'sell' } | null;
}

const initialState: InstrumentsState = {
  currency: 'BTC',
  kind: 'future',
  instruments: [],
  selectedInstrument: 'BTC-PERPETUAL',
  ticker: null,
  loading: false,
  error: null,
  priceFromBook: null,
};

export const instrumentsSlice = createSlice({
  name: 'instruments',
  initialState,
  reducers: {
    setCurrency(state, { payload }: PayloadAction<string>) {
      state.currency = payload;
      state.instruments = [];
      state.selectedInstrument = '';
      state.ticker = null;
    },
    setKind(state, { payload }: PayloadAction<string>) {
      state.kind = payload;
      state.instruments = [];
      state.selectedInstrument = '';
      state.ticker = null;
    },
    setInstruments(state, { payload }: PayloadAction<Instrument[]>) {
      state.instruments = payload;
      if (payload.length > 0 && !payload.find((i) => i.instrument_name === state.selectedInstrument)) {
        state.selectedInstrument = payload[0].instrument_name;
      }
    },
    setSelectedInstrument(state, { payload }: PayloadAction<string>) {
      state.selectedInstrument = payload;
      state.ticker = null;
    },
    setTicker(state, { payload }: PayloadAction<Ticker>) {
      state.ticker = payload;
    },
    setLoading(state, { payload }: PayloadAction<boolean>) {
      state.loading = payload;
    },
    setError(state, { payload }: PayloadAction<string | null>) {
      state.error = payload;
    },
    setPriceFromBook(state, { payload }: PayloadAction<{ price: number; side: 'buy' | 'sell' } | null>) {
      state.priceFromBook = payload;
    },
  },
});

export const {
  setCurrency,
  setKind,
  setInstruments,
  setSelectedInstrument,
  setTicker,
  setLoading,
  setError,
  setPriceFromBook,
} = instrumentsSlice.actions;

export const selectCurrency = (s: RootState) => s.instruments.currency;
export const selectKind = (s: RootState) => s.instruments.kind;
export const selectInstruments = (s: RootState) => s.instruments.instruments;
export const selectSelectedInstrument = (s: RootState) => s.instruments.selectedInstrument;
export const selectTicker = (s: RootState) => s.instruments.ticker;
export const selectInstrumentsLoading = (s: RootState) => s.instruments.loading;
export const selectPriceFromBook = (s: RootState) => s.instruments.priceFromBook;

export default instrumentsSlice.reducer;
