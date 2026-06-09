import { FunctionComponent, useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useAppDispatch, useAppSelector } from '../../hooks';
import { selectSelectedInstrument, selectInstruments, selectExchangeSymbol, Instrument, ReferenceData, selectPriceFromBook } from './instrumentsSlice';
import { selectActiveAccountId, selectAccounts, setActiveAccount } from '../Settings/settingsSlice';
import { setLastOrderResult, setSubmitting, selectSubmitting, selectLastOrderResult, setOpenOrders } from './ordersSlice';
import { selectWsStatus, WsConnectionStatus } from '../WsManager/wsSlice';
import styled from 'styled-components';

// ── Styled components ──────────────────────────────────────────────────────

const FormContainer = styled.div`
  background: #141a28;
  border: 1px solid #1e2738;
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow-y: auto;
`;

const AccountBar = styled.div`
  padding: 0.45rem 0.65rem;
  border-bottom: 1px solid #1e2738;
  background: #0f1522;
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
`;

const AccountBarLabel = styled.span`
  color: #4a5568;
  font-size: 0.68rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
`;

const AccountSelectEl = styled.select`
  background: #141a28;
  border: 1px solid #29303e;
  color: #e8edf4;
  padding: 0.3rem 0.5rem;
  border-radius: 3px;
  font-size: 0.84rem;
  width: 100%;
  &:focus { border-color: #5087f2; outline: none; }
`;

const InstrumentTag = styled.div`
  font-size: 0.78rem;
  color: #7e8b99;
  padding: 0.15rem 0;
  span { color: #c9d4e0; font-weight: 600; }
`;

const ExchangeBadge = styled.span<{ $ex: string }>`
  display: inline-block;
  font-size: 0.65rem;
  font-weight: 700;
  padding: 0.1rem 0.35rem;
  border-radius: 3px;
  margin-right: 0.35rem;
  color: ${({ $ex }) => EX_COLORS[$ex] ?? '#7e8b99'};
  background: ${({ $ex }) => (EX_COLORS[$ex] ?? '#7e8b99') + '22'};
`;

const EX_COLORS: Record<string, string> = {
  deribit: '#5087f2', okx: '#e0b94a', bybit: '#f7a600',
  coincall: '#33b48f', binance: '#f0b90b', mexc: '#2aabee',
  hyperliquid: '#52ff70', uniswap: '#ff007a',
};

// ── WS Status badge ──────────────────────────────────────────────────────────

const WsBadge = styled.div<{ $status: WsConnectionStatus }>`
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  font-size: 0.72rem;
  font-weight: 600;
  letter-spacing: 0.03em;
  background: ${({ $status }) =>
    $status === 'connected'   ? 'rgba(63,185,80,0.12)' :
    $status === 'connecting'  ? 'rgba(210,153,34,0.12)' :
    $status === 'reconnecting'? 'rgba(210,153,34,0.12)' :
    $status === 'error'       ? 'rgba(248,81,73,0.12)' :
    'rgba(139,148,158,0.10)'};
  color: ${({ $status }) =>
    $status === 'connected'   ? '#3fb950' :
    $status === 'connecting'  ? '#d29922' :
    $status === 'reconnecting'? '#d29922' :
    $status === 'error'       ? '#f85149' :
    '#8b949e'};
  border: 1px solid ${({ $status }) =>
    $status === 'connected'   ? 'rgba(63,185,80,0.25)' :
    $status === 'connecting'  ? 'rgba(210,153,34,0.25)' :
    $status === 'reconnecting'? 'rgba(210,153,34,0.25)' :
    $status === 'error'       ? 'rgba(248,81,73,0.25)' :
    'rgba(139,148,158,0.15)'};
`;

const WsDot = styled.span<{ $status: WsConnectionStatus }>`
  width: 7px;
  height: 7px;
  border-radius: 50%;
  flex-shrink: 0;
  background: ${({ $status }) =>
    $status === 'connected'   ? '#3fb950' :
    $status === 'connecting'  ? '#d29922' :
    $status === 'reconnecting'? '#d29922' :
    $status === 'error'       ? '#f85149' :
    '#8b949e'};
  ${({ $status }) => ($status === 'connected' || $status === 'connecting' || $status === 'reconnecting') && `
    animation: ws-pulse 1.8s ease-in-out infinite;
  `}
  @keyframes ws-pulse {
    0%, 100% { opacity: 1; }
    50%       { opacity: 0.35; }
  }
`;

const TabRow = styled.div`
  display: flex;
  border-bottom: 1px solid #1e2738;
  flex-shrink: 0;
`;

const Tab = styled.button<{ $side: 'buy' | 'sell'; $active: boolean }>`
  flex: 1;
  padding: 0.6rem;
  border: none;
  cursor: pointer;
  font-size: 0.9rem;
  font-weight: 600;
  background: ${({ $active, $side }) =>
    !$active ? '#0f1522' : $side === 'buy' ? '#0f3320' : '#3a1010'};
  color: ${({ $active, $side }) =>
    !$active ? '#4a5568' : $side === 'buy' ? '#33b48f' : '#d0616e'};
  border-bottom: 2px solid ${({ $active, $side }) =>
    !$active ? 'transparent' : $side === 'buy' ? '#33b48f' : '#d0616e'};
  transition: all 0.15s;
`;

const Body = styled.div`
  padding: 0.75rem;
  display: flex;
  flex-direction: column;
  gap: 0.55rem;
  flex: 1;
`;

const Row = styled.div`
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
`;

const FieldLabel = styled.div`
  color: #4a5568;
  font-size: 0.72rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
`;

const FieldInput = styled.input`
  background: #0f1522;
  border: 1px solid #29303e;
  color: #e8edf4;
  padding: 0.4rem 0.6rem;
  border-radius: 3px;
  font-size: 0.88rem;
  width: 100%;
  &:focus { border-color: #5087f2; outline: none; }
  &:disabled { opacity: 0.5; }
`;

const FieldSelect = styled.select`
  background: #0f1522;
  border: 1px solid #29303e;
  color: #e8edf4;
  padding: 0.4rem 0.6rem;
  border-radius: 3px;
  font-size: 0.88rem;
  width: 100%;
  &:focus { border-color: #5087f2; outline: none; }
`;

const ConfigCard = styled.div`
  background: #0d1220;
  border: 1px solid #1e2738;
  border-radius: 4px;
  padding: 0.55rem 0.65rem;
  display: flex;
  flex-direction: column;
  gap: 0.45rem;
`;

const ConfigLabel = styled.div`
  color: #2aabee;
  font-size: 0.68rem;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.06em;
`;

const MiniGrid = styled.div`
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 0.4rem;
`;

const MiniLabel = styled.div`
  color: #4a5568;
  font-size: 0.7rem;
  text-transform: uppercase;
  letter-spacing: 0.03em;
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
`;

const CheckRow = styled.label`
  display: flex;
  align-items: center;
  gap: 0.4rem;
  color: #7e8b99;
  font-size: 0.82rem;
  cursor: pointer;
`;

const SubmitBtn = styled.button<{ $side: 'buy' | 'sell' }>`
  width: 100%;
  padding: 0.65rem;
  border: none;
  border-radius: 3px;
  font-size: 0.95rem;
  font-weight: 600;
  cursor: pointer;
  background: ${({ $side }) => $side === 'buy' ? '#1a5c3a' : '#5c1a1a'};
  color: ${({ $side }) => $side === 'buy' ? '#33b48f' : '#d0616e'};
  border: 1px solid ${({ $side }) => $side === 'buy' ? '#33b48f40' : '#d0616e40'};
  transition: opacity 0.15s;
  margin-top: auto;
  &:hover { opacity: 0.85; }
  &:disabled { opacity: 0.5; cursor: not-allowed; }
`;

const ResultMsg = styled.div<{ $success: boolean }>`
  font-size: 0.8rem;
  padding: 0.4rem 0.6rem;
  border-radius: 3px;
  background: ${({ $success }) => $success ? '#0f3320' : '#3a1010'};
  color: ${({ $success }) => $success ? '#33b48f' : '#d0616e'};
  border: 1px solid ${({ $success }) => $success ? '#33b48f30' : '#d0616e30'};
`;

const ExecLog = styled.div`
  background: #060b14;
  border: 1px solid #1e2738;
  border-radius: 3px;
  padding: 0.4rem 0.6rem;
  font-size: 0.75rem;
  font-family: monospace;
  color: #7e8b99;
  max-height: 110px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
`;

const TypeBadge = styled.span<{ $type: OrderType }>`
  display: inline-block;
  font-size: 0.65rem;
  font-weight: 700;
  padding: 0.1rem 0.35rem;
  border-radius: 3px;
  margin-left: 0.3rem;
  color: ${({ $type }) => TYPE_COLORS[$type] ?? '#7e8b99'};
  background: ${({ $type }) => (TYPE_COLORS[$type] ?? '#7e8b99') + '22'};
`;

const NoAccount = styled.div`
  padding: 1rem;
  color: #4a5568;
  font-size: 0.85rem;
  text-align: center;
`;

// ── Types ──────────────────────────────────────────────────────────────────

type OrderType =
  | 'limit' | 'limit_post' | 'market'
  | 'stop_limit' | 'stop_market'
  | 'smart_post' | 'hedge' | 'smart_hedge';

const TYPE_COLORS: Record<OrderType, string> = {
  limit: '#7e8b99', limit_post: '#5087f2', market: '#e0b94a',
  stop_limit: '#d0616e', stop_market: '#d0616e',
  smart_post: '#2aabee', hedge: '#33b48f', smart_hedge: '#a78bfa',
};

const TYPE_LABELS: Record<OrderType, string> = {
  limit: 'Limit', limit_post: 'Limit Post-Only', market: 'Market',
  stop_limit: 'Stop Limit', stop_market: 'Stop Market',
  smart_post: 'Smart Post', hedge: 'Hedge (Δ)', smart_hedge: 'Smart Hedge',
};

// ── Helpers ────────────────────────────────────────────────────────────────

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

// ── Component ──────────────────────────────────────────────────────────────

const OrderForm: FunctionComponent = () => {
  const dispatch = useAppDispatch();
  const instrument = useAppSelector(selectSelectedInstrument);
  const exchangeSymbol = useAppSelector(selectExchangeSymbol);
  const instruments = useAppSelector(selectInstruments) as ReferenceData[];
  const activeAccountId = useAppSelector(selectActiveAccountId);
  const accounts = useAppSelector(selectAccounts);
  const submitting = useAppSelector(selectSubmitting);
  const lastResult = useAppSelector(selectLastOrderResult);
  const priceFromBook = useAppSelector(selectPriceFromBook);

  // Basic order state
  const [side, setSide] = useState<'buy' | 'sell'>('buy');
  const [orderType, setOrderType] = useState<OrderType>('limit');
  const [price, setPrice] = useState('');
  const [amount, setAmount] = useState('');
  const [tif, setTif] = useState('good_til_cancelled');

  // Smart Post config
  const [spChase, setSpChase] = useState(true);
  const [spIntervalMs, setSpIntervalMs] = useState(1000);
  const [spMaxChases, setSpMaxChases] = useState(5);

  // Hedge config
  const [hedgeInstruments, setHedgeInstruments] = useState<Instrument[]>([]);
  const [hedgeInstrument, setHedgeInstrument] = useState('');
  const [hedgeDelta, setHedgeDelta] = useState('');
  const [fetchingDelta, setFetchingDelta] = useState(false);

  // Smart Hedge config
  const [shMaxTries, setShMaxTries] = useState(3);
  const [shTryIntervalSec, setShTryIntervalSec] = useState(10);
  const [shChunkPct, setShChunkPct] = useState(25);

  // Execution log
  const [executing, setExecuting] = useState(false);
  const [execLog, setExecLog] = useState<string[]>([]);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => { mountedRef.current = false; };
  }, []);

  // When user clicks a book row in ExchangePanel, auto-fill price and set side
  useEffect(() => {
    if (!priceFromBook) return;
    setPrice(String(priceFromBook.price));
    setSide(priceFromBook.side);
    // Switch to limit order type if currently market (market has no price field)
    setOrderType(prev => prev === 'market' || prev === 'stop_market' ? 'limit' : prev);
  }, [priceFromBook]);

  const addLog = (msg: string) => {
    if (mountedRef.current) {
      setExecLog((prev) => [...prev.slice(-19), `[${new Date().toLocaleTimeString()}] ${msg}`]);
    }
  };

  // Fetch hedge instruments when hedge type selected
  const currentRef = instruments.find((r) => r.symbol === instrument) ?? null;
  const baseCurrency = currentRef?.base ?? 'BTC';

  useEffect(() => {
    if ((orderType === 'hedge' || orderType === 'smart_hedge') && activeAccountId && activeAccount) {
      invoke<Instrument[]>('fetch_instruments', {
        exchange: activeAccount.exchange,
        currency: baseCurrency,
        kind: 'future',
      })
        .then((list) => {
          setHedgeInstruments(list);
          if (list.length > 0 && !hedgeInstrument) {
            const perp = list.find(
              (i) => i.instrument_name.includes('PERP') || i.kind === 'perpetual'
            );
            setHedgeInstrument(perp?.instrument_name ?? list[0].instrument_name);
          }
        })
        .catch(console.error);
    }
  }, [orderType, activeAccountId, baseCurrency]);

  const activeAccount = accounts.find((a) => a.id === activeAccountId) ?? null;
  const isOptionsExchange = ['deribit', 'coincall'].includes(activeAccount?.exchange ?? '');
  const wsStatus = useAppSelector(selectWsStatus(activeAccountId ?? ''));
  const wsLabel =
    wsStatus === 'connected'    ? 'WS Live' :
    wsStatus === 'connecting'   ? 'Connecting…' :
    wsStatus === 'reconnecting' ? 'Reconnecting…' :
    wsStatus === 'error'        ? 'WS Error' :
    'WS Offline';
  const minAmount = currentRef?.venues.find(v => v.exchange === (activeAccount?.exchange ?? ''))?.minTradeAmount ?? 1;
  const showPrice = ['limit', 'limit_post', 'stop_limit', 'stop_market', 'smart_post', 'smart_hedge'].includes(orderType);
  const showTif   = ['limit', 'limit_post', 'stop_limit'].includes(orderType);
  const isSmartType = ['smart_post', 'hedge', 'smart_hedge'].includes(orderType);
  const busy = submitting || executing;

  // Reset to 'limit' if a hedge/smart_hedge type is selected on a non-options exchange
  useEffect(() => {
    if (!isOptionsExchange && (orderType === 'hedge' || orderType === 'smart_hedge')) {
      setOrderType('limit');
      setExecLog([]);
    }
  }, [isOptionsExchange, orderType]);

  // Fetch net delta from account summary
  const handleFetchDelta = async () => {
    if (!activeAccountId) return;
    setFetchingDelta(true);
    try {
      const summary = await invoke<any>('get_account_summary', {
        accountId: activeAccountId,
        currency: baseCurrency,
      });
      const delta: number = summary?.delta_total ?? summary?.delta ?? 0;
      setHedgeDelta(Math.abs(delta).toFixed(4));
      if (delta > 0) setSide('sell');
      else if (delta < 0) setSide('buy');
    } catch (e) {
      addLog(`Fetch delta failed: ${e}`);
    } finally {
      setFetchingDelta(false);
    }
  };

  // Place a single exchange order
  const placeOne = (opts: {
    instrumentName: string;
    side: 'buy' | 'sell';
    orderType: string;
    amount: number;
    price?: number | null;
    postOnly?: boolean;
    tif?: string;
  }) =>
    invoke<any>('place_order', {
      req: {
        account_id: activeAccountId,
        instrument_name: opts.instrumentName,
        side: opts.side,
        order_type: opts.orderType,
        amount: opts.amount,
        price: opts.price ?? null,
        time_in_force: opts.tif ?? 'good_til_cancelled',
        post_only: opts.postOnly ?? false,
        label: null,
      },
    });

  // Poll open orders to find a specific order (null = filled/gone)
  const getOpenOrder = async (instrumentName: string, orderId: string) => {
    const orders = await invoke<any[]>('get_open_orders', {
      accountId: activeAccountId,
      instrumentName,
    }).catch(() => [] as any[]);
    return orders.find((o: any) => o.order_id === orderId) ?? null;
  };

  // Get best bid (buy) or best ask (sell) from ticker
  const getTopPrice = async (instrName: string): Promise<number | null> => {
    const ticker = await invoke<any>('fetch_ticker', {
      exchange: activeAccount?.exchange ?? '',
      instrumentName: instrName,
    }).catch(() => null);
    if (!ticker) return null;
    return side === 'buy'
      ? (ticker.best_bid_price ?? ticker.mark_price ?? null)
      : (ticker.best_ask_price ?? ticker.mark_price ?? null);
  };

  // Refresh open orders panel
  const refreshOpenOrders = () => {
    if (!activeAccountId || !exchangeSymbol) return;
    invoke<any[]>('get_open_orders', { accountId: activeAccountId, instrumentName: exchangeSymbol })
      .then((orders) => dispatch(setOpenOrders(orders)))
      .catch(() => {});
  };

  // Smart Post: place limit at top-of-book, optionally chase
  const executeSmartPost = async () => {
    const qty = parseFloat(amount);
    if (!qty || !exchangeSymbol) return;
    setExecuting(true);
    setExecLog([]);
    addLog(`Smart Post: ${side} ${qty} ${exchangeSymbol}`);

    let chaseCount = 0;
    let remaining = qty;

    while (remaining > 0 && mountedRef.current) {
      const topPrice = await getTopPrice(exchangeSymbol);
      if (topPrice == null) { addLog('Cannot fetch top-of-book price'); break; }
      addLog(`[${chaseCount}] Top-of-book = ${topPrice}`);

      const result = await placeOne({
        instrumentName: exchangeSymbol,
        side,
        orderType: 'limit',
        amount: remaining,
        price: topPrice,
        postOnly: true,
      }).catch((e: any) => ({ success: false, error: String(e) }));

      if (!result.success) { addLog(`Place failed: ${result.error}`); break; }

      const orderId: string = result.order?.order_id ?? '';
      addLog(`Placed ${orderId}`);

      if (!spChase) {
        dispatch(setLastOrderResult(result));
        break;
      }

      await sleep(spIntervalMs);
      if (!mountedRef.current) break;

      const open = await getOpenOrder(exchangeSymbol, orderId);
      if (!open) {
        addLog('✓ Filled!');
        dispatch(setLastOrderResult(result));
        break;
      }

      remaining = (open.amount ?? remaining) - (open.filled_amount ?? 0);
      if (remaining <= 0) { addLog('✓ Filled!'); dispatch(setLastOrderResult(result)); break; }

      await invoke('cancel_order', { accountId: activeAccountId, orderId }).catch(() => {});
      chaseCount++;

      if (chaseCount > spMaxChases) {
        addLog(`Max chases (${spMaxChases}) reached.`);
        break;
      }
      addLog(`Chase ${chaseCount}/${spMaxChases}: remaining ${remaining}`);
    }

    setExecuting(false);
    refreshOpenOrders();
  };

  // Hedge: market order on configured instrument for net delta amount
  const executeHedge = async () => {
    const qty = parseFloat(hedgeDelta || amount);
    if (!qty || !hedgeInstrument) {
      addLog('Enter delta amount and select hedge instrument');
      return;
    }
    setExecuting(true);
    setExecLog([]);
    addLog(`Hedge: ${side} ${qty} on ${hedgeInstrument} (market)`);

    const result = await placeOne({
      instrumentName: hedgeInstrument,
      side,
      orderType: 'market',
      amount: qty,
    }).catch((e: any) => ({ success: false, error: String(e) }));

    dispatch(setLastOrderResult(result));
    if (result.success) addLog(`✓ Hedge placed: ${result.order?.order_id}`);
    else addLog(`✗ Hedge failed: ${result.error}`);

    setExecuting(false);
    refreshOpenOrders();
  };

  // Smart Hedge: limit+chase loop, then break into market chunks
  const executeSmartHedge = async () => {
    const hedgeInst = hedgeInstrument || exchangeSymbol;
    let remaining = parseFloat(amount);
    if (!remaining || !hedgeInst) return;
    setExecuting(true);
    setExecLog([]);
    addLog(`Smart Hedge: ${side} ${remaining} ${hedgeInst}`);

    while (remaining > 0 && mountedRef.current) {
      // Limit order chase phase
      let tryCount = 0;

      while (tryCount < shMaxTries && remaining > 0 && mountedRef.current) {
        const topPrice = await getTopPrice(hedgeInst);
        if (topPrice == null) { addLog('Cannot fetch price'); break; }
        addLog(`[try ${tryCount + 1}/${shMaxTries}] Limit @ ${topPrice}, qty ${remaining}`);

        const result = await placeOne({
          instrumentName: hedgeInst,
          side,
          orderType: 'limit',
          amount: remaining,
          price: topPrice,
          tif: 'good_til_cancelled',
        }).catch((e: any) => ({ success: false, error: String(e) }));

        if (!result.success) { addLog(`Place failed: ${result.error}`); tryCount++; continue; }

        const orderId: string = result.order?.order_id ?? '';
        addLog(`Limit order ${orderId}, waiting ${shTryIntervalSec}s…`);

        await sleep(shTryIntervalSec * 1000);
        if (!mountedRef.current) break;

        const open = await getOpenOrder(hedgeInst, orderId);
        if (!open) { addLog('✓ Fully filled by limit'); remaining = 0; break; }

        const filled = open.filled_amount ?? 0;
        remaining = (open.amount ?? remaining) - filled;
        if (remaining <= 0) { addLog('✓ Fully filled'); break; }

        await invoke('cancel_order', { accountId: activeAccountId, orderId }).catch(() => {});
        addLog(`Partial fill: ${filled} filled, ${remaining} left`);
        tryCount++;
      }

      if (remaining <= 0 || !mountedRef.current) break;

      // Market chunk phase
      const chunkQty = Math.min(
        Math.max(minAmount, remaining * (shChunkPct / 100)),
        remaining
      );
      addLog(`Chunk: ${chunkQty} (${shChunkPct}%) via market`);

      const mktResult = await placeOne({
        instrumentName: hedgeInst,
        side,
        orderType: 'market',
        amount: chunkQty,
      }).catch((e: any) => ({ success: false, error: String(e) }));

      if (mktResult.success) {
        addLog(`✓ Market chunk filled: ${chunkQty}`);
        remaining -= chunkQty;
        dispatch(setLastOrderResult(mktResult));
      } else {
        addLog(`✗ Market chunk failed: ${mktResult.error}`);
        break;
      }
    }

    if (remaining <= 0) addLog('✓ Smart Hedge complete');
    setExecuting(false);
    refreshOpenOrders();
  };

  // Submit handler — routes to executor or standard place_order
  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!activeAccountId || busy) return;

    if (orderType === 'smart_post')  { executeSmartPost();  return; }
    if (orderType === 'hedge')       { executeHedge();      return; }
    if (orderType === 'smart_hedge') { executeSmartHedge(); return; }

    dispatch(setSubmitting(true));
    dispatch(setLastOrderResult(null));
    try {
      const isPost = orderType === 'limit_post';
      const result = await invoke<any>('place_order', {
        req: {
          account_id: activeAccountId,
          instrument_name: exchangeSymbol,
          side,
          order_type: isPost ? 'limit' : orderType,
          amount: parseFloat(amount),
          price: showPrice && price ? parseFloat(price) : null,
          time_in_force: tif,
          post_only: isPost,
          label: null,
        },
      });
      dispatch(setLastOrderResult(result));
      if (result.success) { refreshOpenOrders(); setAmount(''); setPrice(''); }
    } catch (err: any) {
      dispatch(setLastOrderResult({ success: false, error: String(err) }));
    } finally {
      dispatch(setSubmitting(false));
    }
  };

  // ── Render ───────────────────────────────────────────────────────────────

  if (accounts.length === 0) {
    return (
      <FormContainer>
        <NoAccount>No accounts configured.<br />Go to Settings to add a trading account.</NoAccount>
      </FormContainer>
    );
  }

  const submitLabel = () => {
    if (busy) return executing ? 'Executing…' : 'Submitting…';
    const dirLabel = side === 'buy' ? 'Buy' : 'Sell';
    if (orderType === 'smart_post')  return `Smart Post ${dirLabel} ${exchangeSymbol || '—'}`;
    if (orderType === 'hedge')       return `Hedge ${dirLabel} on ${hedgeInstrument || '—'}`;
    if (orderType === 'smart_hedge') return `Smart Hedge ${dirLabel} ${hedgeInstrument || exchangeSymbol || '—'}`;
    return `${dirLabel} ${exchangeSymbol || '—'}`;
  };

  return (
    <FormContainer>
      {/* ── Account selector ── */}
      <AccountBar>
        <AccountBarLabel>Account</AccountBarLabel>
        <AccountSelectEl
          value={activeAccountId ?? ''}
          onChange={(e) => dispatch(setActiveAccount(e.target.value))}
        >
          {accounts.map((a) => (
            <option key={a.id} value={a.id}>[{a.exchange.toUpperCase()}] {a.name || a.id}</option>
          ))}
        </AccountSelectEl>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '0.4rem' }}>
          {activeAccount && (
            <InstrumentTag>
              <ExchangeBadge $ex={activeAccount.exchange}>{activeAccount.exchange}</ExchangeBadge>
              {exchangeSymbol
                ? <><span>{exchangeSymbol}</span>{activeAccount.testnet && ' · testnet'}</>
                : <span style={{ color: '#4a5568' }}>no instrument selected</span>
              }
            </InstrumentTag>
          )}
          <WsBadge $status={wsStatus} title={wsStatus === 'error' ? 'WebSocket connection error' : undefined}>
            <WsDot $status={wsStatus} />
            {wsLabel}
          </WsBadge>
        </div>
      </AccountBar>

      {/* ── Buy / Sell tabs ── */}
      <TabRow>
        <Tab $side="buy"  $active={side === 'buy'}  onClick={() => setSide('buy')}>Buy / Long</Tab>
        <Tab $side="sell" $active={side === 'sell'} onClick={() => setSide('sell')}>Sell / Short</Tab>
      </TabRow>

      <Body>
        <form onSubmit={handleSubmit} style={{ display: 'flex', flexDirection: 'column', gap: '0.55rem', flex: 1 }}>

          {/* ── Order Type dropdown ── */}
          <Row>
            <FieldLabel>
              Order Type
              <TypeBadge $type={orderType}>{TYPE_LABELS[orderType]}</TypeBadge>
            </FieldLabel>
            <FieldSelect
              value={orderType}
              onChange={(e) => { setOrderType(e.target.value as OrderType); setExecLog([]); }}
            >
              <optgroup label="Standard">
                <option value="limit">Limit</option>
                <option value="limit_post">Limit Post-Only (Maker)</option>
                <option value="market">Market</option>
                <option value="stop_limit">Stop Limit</option>
                <option value="stop_market">Stop Market</option>
              </optgroup>
              <optgroup label="Smart Orders">
                <option value="smart_post">Smart Post — Top of Book + Chase</option>
                {isOptionsExchange && (
                  <option value="hedge">Hedge — Net Δ Market Order</option>
                )}
                {isOptionsExchange && (
                  <option value="smart_hedge">Smart Hedge — Limit + Chase + Chunk</option>
                )}
              </optgroup>
            </FieldSelect>
          </Row>

          {/* ── Price (standard types) ── */}
          {showPrice && orderType !== 'smart_post' && orderType !== 'smart_hedge' && (
            <Row>
              <FieldLabel>Price (USD)</FieldLabel>
              <FieldInput
                type="number" step="any" placeholder="0.00"
                value={price} onChange={(e) => setPrice(e.target.value)}
                required={showPrice}
              />
            </Row>
          )}

          {/* ── Amount (non-hedge types) ── */}
          {orderType !== 'hedge' && (
            <Row>
              <FieldLabel>Amount (min {minAmount})</FieldLabel>
              <FieldInput
                type="number" step="any" min={minAmount}
                placeholder={String(minAmount)} value={amount}
                onChange={(e) => setAmount(e.target.value)} required
              />
            </Row>
          )}

          {/* ── TIF ── */}
          {showTif && (
            <Row>
              <FieldLabel>Time In Force</FieldLabel>
              <FieldSelect value={tif} onChange={(e) => setTif(e.target.value)}>
                <option value="good_til_cancelled">GTC — Good Till Cancelled</option>
                <option value="fill_or_kill">FOK — Fill or Kill</option>
                <option value="immediate_or_cancel">IOC — Immediate or Cancel</option>
              </FieldSelect>
            </Row>
          )}

          {/* ── Smart Post config ── */}
          {orderType === 'smart_post' && (
            <ConfigCard>
              <ConfigLabel>⚡ Smart Post Config</ConfigLabel>
              <div style={{ fontSize: '0.75rem', color: '#4a5568' }}>
                Places limit at top-of-book; chases price if not filled within interval.
              </div>
              <CheckRow>
                <input type="checkbox" checked={spChase} onChange={(e) => setSpChase(e.target.checked)} />
                Chase price if unfilled
              </CheckRow>
              {spChase && (
                <MiniGrid>
                  <MiniLabel>
                    Chase Interval (ms)
                    <FieldInput type="number" min={100} step={100} value={spIntervalMs}
                      onChange={(e) => setSpIntervalMs(Number(e.target.value))} />
                  </MiniLabel>
                  <MiniLabel>
                    Max Chases
                    <FieldInput type="number" min={1} max={50} value={spMaxChases}
                      onChange={(e) => setSpMaxChases(Number(e.target.value))} />
                  </MiniLabel>
                </MiniGrid>
              )}
            </ConfigCard>
          )}

          {/* ── Hedge config ── */}
          {orderType === 'hedge' && (
            <ConfigCard>
              <ConfigLabel>🛡 Hedge Config</ConfigLabel>
              <div style={{ fontSize: '0.75rem', color: '#4a5568' }}>
                Places a market order to offset net options delta.
              </div>
              <Row>
                <FieldLabel>Hedge Instrument (Perp / Future)</FieldLabel>
                <FieldSelect value={hedgeInstrument} onChange={(e) => setHedgeInstrument(e.target.value)}>
                  {hedgeInstruments.length === 0 && <option value="">Loading…</option>}
                  {hedgeInstruments.map((i) => (
                    <option key={i.instrument_name} value={i.instrument_name}>
                      {i.instrument_name} ({i.kind})
                    </option>
                  ))}
                </FieldSelect>
              </Row>
              <Row>
                <FieldLabel>Delta Amount ({baseCurrency})</FieldLabel>
                <div style={{ display: 'flex', gap: '0.4rem' }}>
                  <FieldInput
                    type="number" step="any" placeholder="0.0000"
                    value={hedgeDelta} onChange={(e) => setHedgeDelta(e.target.value)}
                    style={{ flex: 1 }} required
                  />
                  <button
                    type="button" disabled={fetchingDelta}
                    onClick={handleFetchDelta}
                    style={{
                      background: '#0d1220', border: '1px solid #29303e', color: '#2aabee',
                      borderRadius: '3px', padding: '0 0.7rem', cursor: 'pointer',
                      fontSize: '0.78rem', whiteSpace: 'nowrap',
                    }}
                  >
                    {fetchingDelta ? '…' : 'Fetch Δ'}
                  </button>
                </div>
                <div style={{ fontSize: '0.72rem', color: '#4a5568', marginTop: '0.15rem' }}>
                  Side auto-flips on Fetch Δ — positive Δ (long) → sell to hedge.
                </div>
              </Row>
            </ConfigCard>
          )}

          {/* ── Smart Hedge config ── */}
          {orderType === 'smart_hedge' && (
            <ConfigCard>
              <ConfigLabel>🧠 Smart Hedge Config</ConfigLabel>
              <div style={{ fontSize: '0.75rem', color: '#4a5568' }}>
                Chases with limit orders; after max tries breaks into market chunks.
              </div>
              <Row>
                <FieldLabel>Hedge Instrument (blank = current)</FieldLabel>
                <FieldSelect value={hedgeInstrument} onChange={(e) => setHedgeInstrument(e.target.value)}>
                  <option value="">{instrument || '(current instrument)'}</option>
                  {hedgeInstruments.map((i) => (
                    <option key={i.instrument_name} value={i.instrument_name}>
                      {i.instrument_name} ({i.kind})
                    </option>
                  ))}
                </FieldSelect>
              </Row>
              <MiniGrid>
                <MiniLabel>
                  Max Tries (limit phase)
                  <FieldInput type="number" min={1} max={20} value={shMaxTries}
                    onChange={(e) => setShMaxTries(Number(e.target.value))} />
                </MiniLabel>
                <MiniLabel>
                  Time per Try (sec)
                  <FieldInput type="number" min={1} max={300} value={shTryIntervalSec}
                    onChange={(e) => setShTryIntervalSec(Number(e.target.value))} />
                </MiniLabel>
                <MiniLabel>
                  Chunk Size (% of remaining)
                  <FieldInput type="number" min={5} max={100} step={5} value={shChunkPct}
                    onChange={(e) => setShChunkPct(Number(e.target.value))} />
                </MiniLabel>
                <MiniLabel>
                  Chunk Order Type
                  <FieldInput value="Market" disabled />
                </MiniLabel>
              </MiniGrid>
            </ConfigCard>
          )}

          {/* ── Execution log for smart types ── */}
          {isSmartType && execLog.length > 0 && (
            <ExecLog>
              {execLog.map((line, i) => (
                <span key={i} style={{
                  color: line.includes('✓') ? '#33b48f' : line.includes('✗') ? '#d0616e' : '#7e8b99',
                }}>
                  {line}
                </span>
              ))}
            </ExecLog>
          )}

          {/* ── Last result (standard types) ── */}
          {lastResult && !isSmartType && (
            <ResultMsg $success={lastResult.success}>
              {lastResult.success
                ? `✓ Order placed: ${lastResult.order?.order_id ?? ''}`
                : `✗ ${lastResult.error}`}
            </ResultMsg>
          )}

          {/* ── WS offline warning ── */}
          {wsStatus !== 'connected' && (
            <div style={{
              padding: '0.35rem 0.55rem',
              borderRadius: '3px',
              background: wsStatus === 'error' ? 'rgba(248,81,73,0.10)' : 'rgba(139,148,158,0.10)',
              border: `1px solid ${wsStatus === 'error' ? 'rgba(248,81,73,0.25)' : 'rgba(139,148,158,0.20)'}`,
              color: wsStatus === 'error' ? '#f85149' : '#8b949e',
              fontSize: '0.74rem',
            }}>
              {wsStatus === 'error'
                ? '⚠ WebSocket error — orders will use REST fallback'
                : wsStatus === 'disconnected'
                ? '⚠ WebSocket offline — orders will use REST fallback'
                : '⏳ WebSocket connecting — orders will use REST until live'}
            </div>
          )}

          <SubmitBtn $side={side} type="submit" disabled={busy || !instrument || !activeAccountId}>
            {submitLabel()}
          </SubmitBtn>
        </form>
      </Body>
    </FormContainer>
  );
};

export default OrderForm;
