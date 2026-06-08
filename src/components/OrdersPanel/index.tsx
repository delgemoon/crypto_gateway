import { useState, useEffect, useCallback } from 'react';
import styled from 'styled-components';
import { invoke } from '@tauri-apps/api/core';
import { useAppSelector } from '../../hooks';

// ── Styled ─────────────────────────────────────────────────────────────────

const Wrap = styled.div`
  display: flex;
  flex-direction: column;
  height: 100%;
  background: #0d1117;
  color: #e6edf3;
  overflow: hidden;
`;

const Toolbar = styled.div`
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  background: #161b22;
  border-bottom: 1px solid #30363d;
  flex-shrink: 0;
`;

const Label = styled.span`
  font-size: 12px;
  color: #8b949e;
`;

const Select = styled.select`
  background: #21262d;
  color: #e6edf3;
  border: 1px solid #30363d;
  border-radius: 4px;
  padding: 5px 10px;
  font-size: 13px;
  cursor: pointer;
`;

const TabBar = styled.div`
  display: flex;
  gap: 2px;
  padding: 0 16px;
  background: #161b22;
  border-bottom: 1px solid #30363d;
  flex-shrink: 0;
`;

const Tab = styled.button<{ $active: boolean }>`
  padding: 8px 16px;
  background: none;
  border: none;
  border-bottom: 2px solid ${p => p.$active ? '#58a6ff' : 'transparent'};
  color: ${p => p.$active ? '#58a6ff' : '#8b949e'};
  font-size: 13px;
  cursor: pointer;
  &:hover { color: #e6edf3; }
`;

const Body = styled.div`
  flex: 1;
  overflow: auto;
  padding: 16px;
`;

const Table = styled.table`
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
`;

const Th = styled.th`
  text-align: left;
  padding: 8px 12px;
  background: #161b22;
  color: #8b949e;
  font-weight: 500;
  border-bottom: 1px solid #30363d;
  position: sticky;
  top: 0;
  z-index: 1;
`;

const Td = styled.td`
  padding: 7px 12px;
  border-bottom: 1px solid #21262d;
  color: #e6edf3;
`;

const CancelBtn = styled.button`
  background: #da3633;
  color: #fff;
  border: none;
  border-radius: 4px;
  padding: 3px 8px;
  font-size: 11px;
  cursor: pointer;
  &:hover { background: #f85149; }
`;

const RefreshBtn = styled.button`
  background: #21262d;
  color: #58a6ff;
  border: 1px solid #30363d;
  border-radius: 4px;
  padding: 5px 12px;
  font-size: 12px;
  cursor: pointer;
  margin-left: auto;
  &:hover { background: #30363d; }
`;

const LogPath = styled.div`
  font-size: 11px;
  color: #6e7681;
  padding: 8px 16px;
  background: #161b22;
  border-top: 1px solid #30363d;
  flex-shrink: 0;
`;

const Msg = styled.div`
  padding: 32px 16px;
  color: #6e7681;
  font-size: 14px;
  text-align: center;
`;

const SideTag = styled.span<{ $side: string }>`
  color: ${p => p.$side === 'buy' ? '#3fb950' : '#f85149'};
  font-weight: 600;
`;

const LiveBadge = styled.span<{ $status: string }>`
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  font-weight: 600;
  padding: 3px 10px;
  border-radius: 10px;
  background: ${p =>
    p.$status === 'connected'   ? 'rgba(63,185,80,0.12)' :
    p.$status === 'connecting' || p.$status === 'reconnecting' ? 'rgba(210,153,34,0.12)' :
    p.$status === 'error'       ? 'rgba(248,81,73,0.12)' :
    'rgba(139,148,158,0.10)'};
  color: ${p =>
    p.$status === 'connected'   ? '#3fb950' :
    p.$status === 'connecting' || p.$status === 'reconnecting' ? '#d29922' :
    p.$status === 'error'       ? '#f85149' :
    '#8b949e'};
  border: 1px solid ${p =>
    p.$status === 'connected'   ? 'rgba(63,185,80,0.25)' :
    p.$status === 'connecting' || p.$status === 'reconnecting' ? 'rgba(210,153,34,0.25)' :
    p.$status === 'error'       ? 'rgba(248,81,73,0.25)' :
    'rgba(139,148,158,0.15)'};

  &::before {
    content: '';
    display: inline-block;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: currentColor;
    animation: ${p =>
      (p.$status === 'connected' || p.$status === 'connecting' || p.$status === 'reconnecting')
        ? 'lbpulse 1.8s ease-in-out infinite' : 'none'};
  }
  @keyframes lbpulse {
    0%, 100% { opacity: 1; }
    50%       { opacity: 0.3; }
  }
`;

// ── Types ──────────────────────────────────────────────────────────────────

interface Order {
  order_id: string;
  instrument_name: string;
  direction: string;
  order_type: string;
  amount: number;
  price: number | null;
  filled_amount: number;
  order_state: string;
  time_in_force: string;
  creation_timestamp: number;
}

interface Trade {
  tradeId: string;
  orderId: string;
  instrumentName: string;
  direction: string;
  amount: number;
  price: number;
  fee: number;
  feeCurrency: string;
  timestamp: number;
}

// ── Component ──────────────────────────────────────────────────────────────

type SubTab = 'open' | 'history';

export default function OrdersPanel() {
  const accounts    = useAppSelector(s => s.settings.accounts);
  const wsConnections = useAppSelector(s => s.ws.connections);
  const liveOrders  = useAppSelector(s => s.ws.liveOrders);
  const liveTrades  = useAppSelector(s => s.ws.liveTrades);

  const [accountId, setAccountId] = useState('');
  const [subTab, setSubTab]       = useState<SubTab>('open');
  const [orders, setOrders]       = useState<Order[]>([]);
  const [trades, setTrades]       = useState<Trade[]>([]);
  const [loading, setLoading]     = useState(false);
  const [error, setError]         = useState('');
  const [logPath, setLogPath]     = useState('');

  // Pick first account as default once accounts load
  useEffect(() => {
    if (accounts.length > 0 && !accountId) {
      setAccountId(accounts[0].id);
    }
  }, [accounts]);

  // Load log path once
  useEffect(() => {
    invoke<string>('get_trade_log_path').then(setLogPath).catch(() => {});
  }, []);

  const loadData = useCallback(async () => {
    if (!accountId) return;
    setLoading(true);
    setError('');
    try {
      if (subTab === 'open') {
        const result = await invoke<Order[]>('get_all_open_orders', { accountId });
        setOrders(result);
      } else {
        const result = await invoke<Trade[]>('get_trade_history', { accountId });
        setTrades(result);
      }
    } catch (e: any) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [accountId, subTab]);

  useEffect(() => {
    loadData();
  }, [loadData]);

  const [cancelling, setCancelling] = useState<string | null>(null);

  const cancelOrder = async (orderId: string, instrumentName: string) => {
    if (!accountId) return;
    setCancelling(orderId);
    try {
      const ok = await invoke<boolean>('cancel_order', { accountId, orderId, instrumentName });
      if (!ok) {
        alert('Cancel was rejected by the exchange (order may already be filled/cancelled).');
      }
      await loadData();
    } catch (e: any) {
      alert('Cancel failed: ' + String(e));
    } finally {
      setCancelling(null);
    }
  };

  const fmt = (ts: number) => {
    if (!ts) return '-';
    return new Date(ts).toLocaleString();
  };

  const wsStatus = wsConnections[accountId] ?? 'disconnected';
  const isLive   = wsStatus === 'connected';

  // Merge REST orders with live WS orders for current account
  const displayOrders: Order[] = (() => {
    if (!isLive) return orders;
    const wsForAccount = Object.values(liveOrders).filter(o => o.accountId === accountId);
    if (wsForAccount.length === 0) return orders;
    // Build merged map: start from REST snapshot, apply WS updates
    const merged = new Map<string, Order>();
    orders.forEach(o => merged.set(o.order_id, o));
    wsForAccount.forEach(wo => {
      merged.set(wo.orderId, {
        order_id:           wo.orderId,
        instrument_name:    wo.instrumentName,
        direction:          wo.direction,
        order_type:         wo.orderType,
        amount:             wo.amount,
        price:              wo.price,
        filled_amount:      wo.filledAmount,
        order_state:        wo.orderState,
        time_in_force:      wo.timeInForce,
        creation_timestamp: wo.timestamp,
      });
    });
    return Array.from(merged.values()).filter(o =>
      o.order_state !== 'filled' && o.order_state !== 'cancelled' && o.order_state !== 'rejected'
    );
  })();

  // Merge REST trades with live WS trades for current account (history tab)
  const displayTrades: Trade[] = (() => {
    if (!isLive) return trades;
    const wsForAccount = liveTrades.filter(t => t.accountId === accountId);
    if (wsForAccount.length === 0) return trades;
    const existing = new Set(trades.map(t => t.tradeId));
    const newTrades: Trade[] = wsForAccount
      .filter(wt => !existing.has(wt.tradeId))
      .map(wt => ({
        tradeId:        wt.tradeId,
        orderId:        wt.orderId,
        instrumentName: wt.instrumentName,
        direction:      wt.direction,
        amount:         wt.amount,
        price:          wt.price,
        fee:            wt.fee,
        feeCurrency:    wt.feeCurrency,
        timestamp:      wt.timestamp,
      }));
    return [...newTrades, ...trades].slice(0, 500);
  })();

  const wsStatusLabel =
    wsStatus === 'connected'    ? 'WS Live' :
    wsStatus === 'connecting'   ? 'Connecting…' :
    wsStatus === 'reconnecting' ? 'Reconnecting…' :
    wsStatus === 'error'        ? 'WS Error' :
    'REST only';

  return (
    <Wrap>
      <Toolbar>
        <Label>Account</Label>
        <Select
          value={accountId}
          onChange={e => setAccountId(e.target.value)}
        >
          {accounts.length === 0
            ? <option value="">No accounts configured</option>
            : accounts.map(a => (
              <option key={a.id} value={a.id}>
                {a.name} ({a.exchange})
              </option>
            ))
          }
        </Select>
        <LiveBadge $status={wsStatus}>{wsStatusLabel}</LiveBadge>
        <RefreshBtn onClick={loadData} disabled={loading}>
          {loading ? '⟳ Loading…' : '⟳ Refresh'}
        </RefreshBtn>
      </Toolbar>

      <TabBar>
        <Tab $active={subTab === 'open'} onClick={() => setSubTab('open')}>
          Open Orders {subTab === 'open' && displayOrders.length > 0 ? `(${displayOrders.length})` : ''}
        </Tab>
        <Tab $active={subTab === 'history'} onClick={() => setSubTab('history')}>
          Trade History {subTab === 'history' && displayTrades.length > 0 ? `(${displayTrades.length})` : ''}
        </Tab>
      </TabBar>

      <Body>
        {accounts.length === 0 ? (
          <Msg>No accounts configured. Go to Settings → Exchange to add accounts.</Msg>
        ) : error ? (
          <Msg style={{ color: '#f85149' }}>Error: {error}</Msg>
        ) : loading ? (
          <Msg>Loading…</Msg>
        ) : subTab === 'open' ? (
          displayOrders.length === 0 ? (
            <Msg>No open orders</Msg>
          ) : (
            <Table>
              <thead>
                <tr>
                  <Th>Instrument</Th>
                  <Th>Side</Th>
                  <Th>Type</Th>
                  <Th>Amount</Th>
                  <Th>Price</Th>
                  <Th>Filled</Th>
                  <Th>Status</Th>
                  <Th>TIF</Th>
                  <Th>Created</Th>
                  <Th>Action</Th>
                </tr>
              </thead>
              <tbody>
                {displayOrders.map(o => (
                  <tr key={o.order_id}>
                    <Td>{o.instrument_name}</Td>
                    <Td><SideTag $side={o.direction}>{o.direction.toUpperCase()}</SideTag></Td>
                    <Td>{o.order_type}</Td>
                    <Td>{o.amount}</Td>
                    <Td>{o.price ?? '-'}</Td>
                    <Td>{o.filled_amount}</Td>
                    <Td>{o.order_state}</Td>
                    <Td>{o.time_in_force}</Td>
                    <Td>{fmt(o.creation_timestamp)}</Td>
                    <Td>
                      <CancelBtn
                        onClick={() => cancelOrder(o.order_id, o.instrument_name)}
                        disabled={cancelling === o.order_id}
                      >
                        {cancelling === o.order_id ? '…' : 'Cancel'}
                      </CancelBtn>
                    </Td>
                  </tr>
                ))}
              </tbody>
            </Table>
          )
        ) : (
          displayTrades.length === 0 ? (
            <Msg>No trade history</Msg>
          ) : (
            <Table>
              <thead>
                <tr>
                  <Th>Time</Th>
                  <Th>Instrument</Th>
                  <Th>Side</Th>
                  <Th>Amount</Th>
                  <Th>Price</Th>
                  <Th>Fee</Th>
                  <Th>Trade ID</Th>
                </tr>
              </thead>
              <tbody>
                {displayTrades.map(t => (
                  <tr key={t.tradeId}>
                    <Td>{fmt(t.timestamp)}</Td>
                    <Td>{t.instrumentName}</Td>
                    <Td><SideTag $side={t.direction}>{t.direction.toUpperCase()}</SideTag></Td>
                    <Td>{t.amount}</Td>
                    <Td>{t.price}</Td>
                    <Td>{t.fee} {t.feeCurrency}</Td>
                    <Td style={{ fontFamily: 'monospace', fontSize: 11 }}>{t.tradeId}</Td>
                  </tr>
                ))}
              </tbody>
            </Table>
          )
        )}
      </Body>

      {logPath && (
        <LogPath>
          📄 Trade log: {logPath}
        </LogPath>
      )}
    </Wrap>
  );
}
