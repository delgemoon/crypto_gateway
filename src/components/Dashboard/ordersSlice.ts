import { createSlice, PayloadAction } from '@reduxjs/toolkit';
import { RootState } from '../../store';

export interface Order {
  order_id: string;
  instrument_name: string;
  direction: string;
  order_type: string;
  order_state: string;
  price?: number;
  amount: number;
  filled_amount: number;
  average_price?: number;
  post_only: boolean;
  time_in_force: string;
  creation_timestamp: number;
  last_update_timestamp: number;
}

export interface OrderResult {
  success: boolean;
  order?: Order;
  error?: string;
}

interface OrdersState {
  openOrders: Order[];
  lastOrderResult: OrderResult | null;
  submitting: boolean;
}

const initialState: OrdersState = {
  openOrders: [],
  lastOrderResult: null,
  submitting: false,
};

export const ordersSlice = createSlice({
  name: 'orders',
  initialState,
  reducers: {
    setOpenOrders(state, { payload }: PayloadAction<Order[]>) {
      state.openOrders = payload;
    },
    setLastOrderResult(state, { payload }: PayloadAction<OrderResult | null>) {
      state.lastOrderResult = payload;
    },
    setSubmitting(state, { payload }: PayloadAction<boolean>) {
      state.submitting = payload;
    },
    removeOpenOrder(state, { payload }: PayloadAction<string>) {
      state.openOrders = state.openOrders.filter((o) => o.order_id !== payload);
    },
  },
});

export const { setOpenOrders, setLastOrderResult, setSubmitting, removeOpenOrder } = ordersSlice.actions;

export const selectOpenOrders = (s: RootState) => s.orders.openOrders;
export const selectLastOrderResult = (s: RootState) => s.orders.lastOrderResult;
export const selectSubmitting = (s: RootState) => s.orders.submitting;

export default ordersSlice.reducer;
