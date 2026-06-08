import { createSlice, PayloadAction } from '@reduxjs/toolkit';

export type WsConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'reconnecting' | 'error';

export interface WsOrderUpdate {
  accountId: string;
  exchange: string;
  orderId: string;
  instrumentName: string;
  direction: string;
  orderType: string;
  orderState: string;
  price: number | null;
  amount: number;
  filledAmount: number;
  timeInForce: string;
  label: string | null;
  clientOrderId: string | null;
  timestamp: number;
}

export interface WsTradeUpdate {
  accountId: string;
  exchange: string;
  tradeId: string;
  orderId: string;
  instrumentName: string;
  direction: string;
  amount: number;
  price: number;
  fee: number;
  feeCurrency: string;
  timestamp: number;
  clientOrderId: string | null;
}

export interface WsPositionUpdate {
  accountId: string;
  exchange: string;
  instrumentName: string;
  direction: string;
  size: number;
  averagePrice: number;
  markPrice: number;
  unrealizedPnl: number;
  delta: number;
  gamma: number;
  theta: number;
  vega: number;
}

export interface WsConnectionEvent {
  accountId: string;
  exchange: string;
  status: string;
  message: string | null;
}

interface WsState {
  connections: Record<string, WsConnectionStatus>;
  liveOrders: Record<string, WsOrderUpdate>;      // keyed by orderId
  liveTrades: WsTradeUpdate[];                     // recent fills, capped at 200
  livePositions: Record<string, WsPositionUpdate>; // keyed by instrumentName
}

const initialState: WsState = {
  connections: {},
  liveOrders: {},
  liveTrades: [],
  livePositions: {},
};

const wsSlice = createSlice({
  name: 'ws',
  initialState,
  reducers: {
    setConnectionStatus(state, action: PayloadAction<WsConnectionEvent>) {
      const { accountId, status } = action.payload;
      const s = status.toLowerCase();
      let mapped: WsConnectionStatus = 'disconnected';
      if (s.startsWith('connected'))   mapped = 'connected';
      else if (s.startsWith('connecting'))   mapped = 'connecting';
      else if (s.startsWith('reconnecting')) mapped = 'reconnecting';
      else if (s.startsWith('error'))        mapped = 'error';
      state.connections[accountId] = mapped;
    },
    upsertLiveOrder(state, action: PayloadAction<WsOrderUpdate>) {
      const o = action.payload;
      if (o.orderState === 'filled' || o.orderState === 'cancelled' || o.orderState === 'rejected') {
        delete state.liveOrders[o.orderId];
      } else {
        state.liveOrders[o.orderId] = o;
      }
    },
    addLiveTrade(state, action: PayloadAction<WsTradeUpdate>) {
      state.liveTrades.unshift(action.payload);
      if (state.liveTrades.length > 200) state.liveTrades.length = 200;
    },
    upsertLivePosition(state, action: PayloadAction<WsPositionUpdate>) {
      const p = action.payload;
      if (p.size === 0) {
        delete state.livePositions[p.instrumentName];
      } else {
        state.livePositions[p.instrumentName] = p;
      }
    },
    clearAccount(state, action: PayloadAction<string>) {
      const id = action.payload;
      delete state.connections[id];
      Object.keys(state.liveOrders).forEach(k => {
        if (state.liveOrders[k].accountId === id) delete state.liveOrders[k];
      });
      state.liveTrades = state.liveTrades.filter(t => t.accountId !== id);
      Object.keys(state.livePositions).forEach(k => {
        if (state.livePositions[k].accountId === id) delete state.livePositions[k];
      });
    },
  },
});

export const {
  setConnectionStatus,
  upsertLiveOrder,
  addLiveTrade,
  upsertLivePosition,
  clearAccount,
} = wsSlice.actions;

export default wsSlice.reducer;

// ── Selectors ────────────────────────────────────────────────────────────────

export const selectWsConnections = (s: any) => s.ws.connections as Record<string, WsConnectionStatus>;
export const selectWsStatus = (accountId: string) => (s: any): WsConnectionStatus =>
  (s.ws.connections[accountId] ?? 'disconnected') as WsConnectionStatus;
