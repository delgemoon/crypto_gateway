import { configureStore } from '@reduxjs/toolkit';
import settingsReducer from './components/Settings/settingsSlice';
import instrumentsReducer from './components/Dashboard/instrumentsSlice';
import ordersReducer from './components/Dashboard/ordersSlice';
import wsReducer from './components/WsManager/wsSlice';
import aggBookReducer from './components/AggBook/aggBookSlice';

export const store = configureStore({
  reducer: {
    settings: settingsReducer,
    instruments: instrumentsReducer,
    orders: ordersReducer,
    ws: wsReducer,
    aggBook: aggBookReducer,
  },
});

export type AppDispatch = typeof store.dispatch;
export type RootState = ReturnType<typeof store.getState>;
