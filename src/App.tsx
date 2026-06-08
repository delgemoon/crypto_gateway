import { useState, useEffect } from 'react';
import styled from 'styled-components';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import GlobalStyle from "./styles/global";
import Navigation from "./components/Navigation";
import Dashboard from "./components/Dashboard";
import Settings from "./components/Settings";
import TelegramPanel from "./components/TelegramPanel";
import RfqPanel from "./components/RfqPanel";
import AccountSummaryPanel from "./components/AccountSummaryPanel";
import OrdersPanel from "./components/OrdersPanel";
import AggBook from "./components/AggBook";
import { useAppDispatch } from './hooks';
import {
  setAccounts, setClients, setTags, setGeneral, setTelegram,
} from './components/Settings/settingsSlice';
import type { Account, Client, Tag, GeneralSettings, TelegramSettings } from './components/Settings/settingsSlice';
import {
  setConnectionStatus,
  upsertLiveOrder,
  addLiveTrade,
  upsertLivePosition,
} from './components/WsManager/wsSlice';
import type {
  WsConnectionEvent,
  WsOrderUpdate,
  WsTradeUpdate,
  WsPositionUpdate,
} from './components/WsManager/wsSlice';
import { setConfigs, setSnapshot } from './components/AggBook/aggBookSlice';
import type { AggBookConfig, AggBookSnapshot } from './components/AggBook/aggBookSlice';

type View = 'trading' | 'rfq' | 'portfolio' | 'orders' | 'telegram' | 'settings' | 'aggbook';

const WS_SUPPORTED = ['deribit', 'bybit', 'okx', 'coincall', 'binance', 'mexc', 'hyperliquid'];

const ContentArea = styled.div`
  flex: 1;
  overflow: hidden;
  display: flex;
  flex-direction: column;
`;

function App() {
  const dispatch = useAppDispatch();
  const [view, setView] = useState<View>('trading');

  useEffect(() => {
    let cancelled = false;
    const unlisteners: (() => void)[] = [];
    let pollTimer: ReturnType<typeof setInterval> | null = null;

    const setup = async () => {
      // ── 1. Register WS event listeners FIRST ────────────────────────────
      unlisteners.push(
        await listen<WsConnectionEvent>('ws://connection', e => {
          if (!cancelled) dispatch(setConnectionStatus(e.payload));
        })
      );
      unlisteners.push(
        await listen<WsOrderUpdate>('ws://order_update', e => {
          if (!cancelled) dispatch(upsertLiveOrder(e.payload));
        })
      );
      unlisteners.push(
        await listen<WsTradeUpdate>('ws://trade_update', e => {
          if (!cancelled) dispatch(addLiveTrade(e.payload));
        })
      );
      unlisteners.push(
        await listen<WsPositionUpdate>('ws://position_update', e => {
          if (!cancelled) dispatch(upsertLivePosition(e.payload));
        })
      );
      unlisteners.push(
        await listen<AggBookSnapshot>('agg_book_update', e => {
          if (!cancelled) dispatch(setSnapshot(e.payload));
        })
      );

      if (cancelled) return;

      // ── 2. Load settings ────────────────────────────────────────────────
      const [accs, clients, tags, general, telegram, aggConfigs] = await Promise.allSettled([
        invoke<Account[]>('get_accounts'),
        invoke<Client[]>('get_clients'),
        invoke<Tag[]>('get_tags'),
        invoke<GeneralSettings>('get_general_settings'),
        invoke<TelegramSettings>('get_telegram_settings'),
        invoke<AggBookConfig[]>('get_agg_book_configs'),
      ]);

      if (cancelled) return;

      if (accs.status === 'fulfilled') dispatch(setAccounts(accs.value));
      if (clients.status === 'fulfilled') dispatch(setClients(clients.value));
      if (tags.status === 'fulfilled') dispatch(setTags(tags.value));
      if (general.status === 'fulfilled') dispatch(setGeneral(general.value));
      if (telegram.status === 'fulfilled') dispatch(setTelegram(telegram.value));
      if (aggConfigs.status === 'fulfilled') dispatch(setConfigs(aggConfigs.value));

      // ── 3. Auto-connect WS for all supported accounts ────────────────────
      if (accs.status === 'fulfilled') {
        for (const acc of accs.value) {
          if (WS_SUPPORTED.includes(acc.exchange)) {
            invoke('ws_connect', { accountId: acc.id }).catch((e: any) =>
              console.warn(`[WS] auto-connect failed for ${acc.name}:`, e)
            );
          }
        }
      }

      // ── 4. Sync current backend WS status (covers reconnected sessions) ──
      syncWsStatus();

      // ── 5. Poll every 5s to keep status fresh ───────────────────────────
      pollTimer = setInterval(syncWsStatus, 5000);
    };

    const syncWsStatus = async () => {
      try {
        const snapshots = await invoke<{ accountId: string; exchange: string; status: string }[]>('ws_status');
        if (cancelled) return;
        for (const s of snapshots) {
          dispatch(setConnectionStatus({
            accountId: s.accountId,
            exchange:  s.exchange,
            status:    s.status,   // wsSlice now lowercases before mapping
            message:   null,
          }));
        }
      } catch { /* ignore */ }
    };

    setup().catch(console.error);

    return () => {
      cancelled = true;
      if (pollTimer) clearInterval(pollTimer);
      unlisteners.forEach(u => u());
    };
  }, []);

  return (
    <>
      <GlobalStyle />
      <Navigation currentView={view} onNavigate={setView} />
      <ContentArea>
        {view === 'trading' && <Dashboard />}
        {view === 'rfq'       && <RfqPanel />}
        {view === 'portfolio'  && <AccountSummaryPanel />}
        {view === 'orders'     && <OrdersPanel />}
        {view === 'telegram'  && <TelegramPanel />}
        {view === 'aggbook'   && <AggBook />}
        {view === 'settings'  && <Settings />}
      </ContentArea>
    </>
  );
}

export default App;
