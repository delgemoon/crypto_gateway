import { createSlice, PayloadAction } from '@reduxjs/toolkit';
import { RootState } from '../../store';

const STORAGE_KEY = 'dashboard_config_v2';

export interface DashboardWidget {
  /** Unique widget instance ID */
  id: string;
  exchange: string;
  /** Currently selected account ID for this widget */
  accountId: string;
}

interface DashboardState {
  widgets: DashboardWidget[];
  initialized: boolean;
}

function load(): Partial<DashboardState> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return JSON.parse(raw);
  } catch { /* ignore */ }
  return {};
}

const saved = load();

const initialState: DashboardState = {
  widgets:     saved.widgets ?? [],
  initialized: false,
};

function save(state: DashboardState) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ widgets: state.widgets }));
  } catch { /* ignore */ }
}

function makeId(): string {
  return `w_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`;
}

export const dashboardSlice = createSlice({
  name: 'dashboard',
  initialState,
  reducers: {
    /**
     * Called at startup once accounts are known.
     * Seeds one widget per unique exchange (up to maxWidgets) on first run.
     * Prunes widgets whose accountId no longer exists.
     */
    initWidgets(state, { payload }: PayloadAction<{
      accounts: { id: string; exchange: string }[];
      maxWidgets: number;
    }>) {
      const existingIds = new Set(payload.accounts.map(a => a.id));

      if (!state.initialized) {
        state.initialized = true;
        if (state.widgets.length === 0) {
          const seen = new Set<string>();
          for (const acc of payload.accounts) {
            if (state.widgets.length >= payload.maxWidgets) break;
            if (!seen.has(acc.exchange)) {
              seen.add(acc.exchange);
              state.widgets.push({ id: makeId(), exchange: acc.exchange, accountId: acc.id });
            }
          }
        }
      }

      // Always prune stale accountIds
      state.widgets = state.widgets.filter(w => existingIds.has(w.accountId));
      save(state);
    },

    /**
     * Add a widget for an exchange.
     * If adding exceeds maxWidgets, the last existing widget is replaced.
     */
    addWidget(state, { payload }: PayloadAction<{
      exchange: string;
      accountId: string;
      maxWidgets: number;
    }>) {
      const newWidget: DashboardWidget = {
        id: makeId(),
        exchange: payload.exchange,
        accountId: payload.accountId,
      };
      if (state.widgets.length >= payload.maxWidgets) {
        // Replace the last widget with the new one
        state.widgets[state.widgets.length - 1] = newWidget;
      } else {
        state.widgets.push(newWidget);
      }
      save(state);
    },

    removeWidget(state, { payload: widgetId }: PayloadAction<string>) {
      state.widgets = state.widgets.filter(w => w.id !== widgetId);
      save(state);
    },

    updateWidgetAccount(state, { payload }: PayloadAction<{ widgetId: string; accountId: string }>) {
      const w = state.widgets.find(w => w.id === payload.widgetId);
      if (w) {
        w.accountId = payload.accountId;
        save(state);
      }
    },
  },
});

export const { initWidgets, addWidget, removeWidget, updateWidgetAccount } = dashboardSlice.actions;

export const selectWidgets = (state: RootState) => state.dashboard.widgets;

export default dashboardSlice.reducer;
