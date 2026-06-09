import { FunctionComponent, memo, useState, useEffect, useRef, useMemo } from 'react';
import { createPortal } from 'react-dom';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import styled from 'styled-components';
import { useAppDispatch, useAppSelector } from '../../hooks';
import { Account, setActiveAccount, selectGeneral } from '../Settings/settingsSlice';
import { setSelectedInstrument, setInstruments, setExchangeSymbol, setPriceFromBook, ReferenceData, Ticker } from './instrumentsSlice';

// ── Types ──────────────────────────────────────────────────────────────────

type BookMap = Map<number, number>;
interface BookLevel { price: number; size: number; total: number; depth: number; }
interface ExpiryOption { key: string; label: string; }

interface MarketBookEvent {
  symbol: string;
  exchange: string;
  exchangeSymbol: string;
  bids: [number, number][];
  asks: [number, number][];
  timestamp: number;
}

interface MarketTickerEvent {
  symbol: string;
  exchange: string;
  exchangeSymbol: string;
  last?: number;
  mark?: number;
  index?: number;
  bid?: number;
  ask?: number;
  bidIv?: number;
  askIv?: number;
  markIv?: number;
  delta?: number;
  gamma?: number;
  vega?: number;
  theta?: number;
  openInterest?: number;
  priceChange24h?: number;
  volume24h?: number;
  high24h?: number;
  low24h?: number;
  timestamp: number;
}

// ── Constants ──────────────────────────────────────────────────────────────

const LEVELS = 50;
const BASE_CURRENCIES = ['BTC', 'ETH', 'SOL', 'XRP', 'DOGE', 'MATIC', 'BNB', 'USDC'];
const KINDS = ['future', 'option', 'spot'];

const EXCHANGE_COLORS: Record<string, string> = {
  deribit:  '#5087f2',
  okx:      '#e0b94a',
  bybit:    '#f7a600',
  coincall: '#33b48f',
};

// ── Helpers ────────────────────────────────────────────────────────────────

/** Expiry key from a ReferenceData entry: perpetuals → 'PERP', futures → 'YYYYMMDD' */
const expiryKey = (r: ReferenceData): string =>
  (r.kind === 'perpetual' || !r.expiry) ? 'PERP' : r.expiry;

/** Format a YYYYMMDD key for display (e.g. '20240329' → '29MAR24'). */
const formatExpiryKey = (key: string): string => {
  if (key === 'PERP') return 'PERP';
  const y = key.slice(0, 4), m = +key.slice(4, 6) - 1, d = key.slice(6, 8);
  const mon = ['JAN','FEB','MAR','APR','MAY','JUN','JUL','AUG','SEP','OCT','NOV','DEC'][m] ?? '???';
  return `${d}${mon}${y.slice(2)}`;
};

const buildLevels = (m: BookMap, side: 'bid' | 'ask', n: number): BookLevel[] => {
  const entries = [...m.entries()];
  entries.sort((a, b) => side === 'bid' ? b[0] - a[0] : a[0] - b[0]);
  const top = entries.slice(0, n);
  let running = 0;
  const wt = top.map(([price, size]) => { running += size; return { price, size, total: running, depth: 0 }; });
  const maxT = wt[wt.length - 1]?.total ?? 1;
  return wt.map(l => ({ ...l, depth: (l.total / maxT) * 100 }));
};

// ── Styled ─────────────────────────────────────────────────────────────────

const Panel = styled.div`
  display: flex;
  flex-direction: column;
  background: #0d1117;
  border: 1px solid #1e2738;
  border-radius: 4px;
  min-width: 270px;
  flex: 1;
  overflow: hidden;
`;

const PanelHeader = styled.div`
  display: flex;
  align-items: center;
  gap: 0.4rem;
  padding: 0.4rem 0.6rem;
  border-bottom: 1px solid #1e2738;
  background: #0f1522;
  flex-shrink: 0;
`;

const ExBadge = styled.span<{ $exchange: string }>`
  padding: 0.1rem 0.4rem;
  border-radius: 3px;
  font-size: 0.68rem;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: ${p => EXCHANGE_COLORS[p.$exchange] ?? '#5087f2'};
  background: ${p => EXCHANGE_COLORS[p.$exchange] ?? '#5087f2'}22;
  flex-shrink: 0;
`;

const AccountName = styled.span`
  color: #d9dde4;
  font-size: 0.78rem;
  font-weight: 500;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
`;

const InstrLabel = styled.span`
  color: #7e8b99;
  font-size: 0.67rem;
  font-family: 'JetBrains Mono', monospace;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 130px;
`;

const SettleBadge = styled.span<{ $inverse: boolean }>`
  padding: 0.08rem 0.3rem;
  border-radius: 3px;
  font-size: 0.6rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  flex-shrink: 0;
  color:      ${p => p.$inverse ? '#e0b94a' : '#5087f2'};
  background: ${p => p.$inverse ? '#e0b94a22' : '#5087f222'};
  border: 1px solid ${p => p.$inverse ? '#e0b94a44' : '#5087f244'};
`;

const ConnDot = styled.span<{ $connected: boolean }>`
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: ${p => p.$connected ? '#33b48f' : '#4a5568'};
  flex-shrink: 0;
`;

const SelectorsBlock = styled.div`
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 0.35rem 0.5rem;
  border-bottom: 1px solid #1e2738;
  background: #0d1117;
  flex-shrink: 0;
`;

const SelectorRow = styled.div`
  display: flex;
  gap: 3px;
`;

const SLabel = styled.div`
  color: #4a5568;
  font-size: 0.6rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex: 1;
  min-width: 0;
`;

// ── TinySelect ──────────────────────────────────────────────────────────────
// Custom dropdown replacing <select>: WebView2's native popup steals Win32
// focus, making document.activeElement null inside the WebView — so our guard
// can't detect it and DOM updates close the native dropdown.  A div-based
// dropdown controlled purely by React state is immune to this.

const TSWrapper = styled.div`
  position: relative;
  width: 100%;
  min-width: 0;
`;

const TSTrigger = styled.button`
  background: #141a28;
  border: 1px solid #29303e;
  color: #e8edf4;
  padding: 0.22rem 1.4rem 0.22rem 0.3rem;
  border-radius: 3px;
  font-size: 0.74rem;
  width: 100%;
  min-width: 0;
  text-align: left;
  cursor: pointer;
  position: relative;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  &:focus { border-color: #5087f2; outline: none; }
  &::after {
    content: '▾';
    position: absolute;
    right: 0.3rem;
    top: 50%;
    transform: translateY(-50%);
    font-size: 0.6rem;
    color: #7e8b99;
    pointer-events: none;
  }
`;

const TSOption = styled.div<{ $active: boolean }>`
  padding: 0.26rem 0.4rem;
  font-size: 0.74rem;
  color: ${p => p.$active ? '#5087f2' : '#e8edf4'};
  background: ${p => p.$active ? '#1e2a4a' : 'transparent'};
  cursor: pointer;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  &:hover { background: #1e2a3a; }
`;

interface TinySelectOption { value: string | number; label: string; }
interface TinySelectProps {
  value: string | number;
  onChange: (v: string) => void;
  options: TinySelectOption[];
}

const TinySelect: FunctionComponent<TinySelectProps> = ({ value, onChange, options }) => {
  const [open, setOpen]   = useState(false);
  const [pos, setPos]     = useState({ top: 0, left: 0, width: 0 });
  const triggerRef        = useRef<HTMLButtonElement>(null);

  const openDropdown = () => {
    if (triggerRef.current) {
      const r = triggerRef.current.getBoundingClientRect();
      setPos({ top: r.bottom + 2, left: r.left, width: r.width });
    }
    setOpen(true);
  };

  // Close on outside click (mousedown so it fires before blur).
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (!triggerRef.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  const selected = options.find(o => String(o.value) === String(value));

  return (
    <TSWrapper>
      <TSTrigger
        ref={triggerRef}
        type="button"
        onClick={() => open ? setOpen(false) : openDropdown()}
        onKeyDown={e => {
          if (e.key === 'Escape') setOpen(false);
          if ((e.key === 'Enter' || e.key === ' ') && !open) { e.preventDefault(); openDropdown(); }
        }}
      >
        {selected?.label ?? String(value)}
      </TSTrigger>
      {open && createPortal(
        <div
          style={{
            position: 'fixed', top: pos.top, left: pos.left, width: pos.width,
            zIndex: 99999,
            background: '#141a28', border: '1px solid #5087f2',
            borderRadius: '3px', maxHeight: '200px', overflowY: 'auto',
            boxShadow: '0 4px 20px rgba(0,0,0,0.7)',
            fontFamily: 'inherit',
          }}
        >
          {options.map(opt => (
            <TSOption
              key={String(opt.value)}
              $active={String(opt.value) === String(value)}
              onMouseDown={e => {
                e.preventDefault();   // prevent trigger blur
                onChange(String(opt.value));
                setOpen(false);
              }}
            >
              {opt.label}
            </TSOption>
          ))}
        </div>,
        document.body
      )}
    </TSWrapper>
  );
};

const CPToggle = styled.div`
  display: flex;
  border-radius: 3px;
  overflow: hidden;
  border: 1px solid #29303e;
  background: #141a28;
  flex: 1;
`;

const CPBtn = styled.button<{ $active: boolean; $side: 'C' | 'P' }>`
  flex: 1;
  padding: 0.22rem;
  border: none;
  cursor: pointer;
  font-size: 0.74rem;
  font-weight: 600;
  background: ${p => p.$active ? (p.$side === 'C' ? '#1a3a5e' : '#3a1a2a') : 'transparent'};
  color: ${p => p.$active ? (p.$side === 'C' ? '#5087f2' : '#d0616e') : '#4a5568'};
  transition: background 0.12s;
  &:hover { opacity: 0.85; }
`;

// Ticker: 4 cols × 2 rows = 8 cells
const TickerBar = styled.div`
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  background: #0f1522;
  border-bottom: 1px solid #1e2738;
  flex-shrink: 0;
`;

const TickerCell = styled.div`
  padding: 0.28rem 0.4rem;
  border-right: 1px solid #1a2233;
  border-bottom: 1px solid #1a2233;
  &:nth-child(4n) { border-right: none; }
  &:nth-last-child(-n+4) { border-bottom: none; }
  .lbl { color: #4a5568; font-size: 0.58rem; text-transform: uppercase; letter-spacing: 0.04em; display: block; }
  .val { color: #e8edf4; font-size: 0.78rem; font-weight: 500; display: block; }
  .bid { color: #33b48f; }
  .ask { color: #d0616e; }
  .pos { color: #33b48f; }
  .neg { color: #d0616e; }
  .dim { color: #7e8b99; }
`;

const BookWrapper = styled.div`
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  font-family: 'JetBrains Mono', 'Fira Mono', 'Courier New', monospace;
  font-size: 0.72rem;
`;

const BookHeader = styled.div`
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  padding: 0.2rem 0.5rem;
  color: #4a5568;
  font-size: 0.6rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  border-bottom: 1px solid #1a2233;
  flex-shrink: 0;
`;

const BookSection = styled.div<{ $reverse?: boolean }>`
  flex: 1;
  overflow-y: scroll;
  overflow-x: hidden;
  min-height: 0;
  display: flex;
  flex-direction: ${p => p.$reverse ? 'column-reverse' : 'column'};
  &::-webkit-scrollbar {
    width: 4px;
  }
  &::-webkit-scrollbar-track {
    background: transparent;
  }
  &::-webkit-scrollbar-thumb {
    background: #2a3a55;
    border-radius: 2px;
  }
  &::-webkit-scrollbar-thumb:hover {
    background: #3a5080;
  }
`;

const BookRow = styled.div<{ $side: 'bid' | 'ask' }>`
  position: relative;
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  padding: 0.14rem 0.5rem;
  cursor: pointer;
  user-select: none;
  &:hover { background: rgba(255,255,255,0.07); }
  &::before {
    content: '';
    position: absolute;
    top: 0; bottom: 0;
    ${p => p.$side === 'bid' ? 'right: 0;' : 'left: 0;'}
    width: var(--depth, 0%);
    background: ${p => p.$side === 'bid' ? 'rgba(51,180,143,0.10)' : 'rgba(208,97,110,0.10)'};
    pointer-events: none;
  }
`;

const PriceCell = styled.span<{ $side: 'bid' | 'ask' }>`
  color: ${p => p.$side === 'bid' ? '#33b48f' : '#d0616e'};
  font-weight: 500;
  z-index: 1;
  display: flex;
  flex-direction: column;
  gap: 0;
`;

const UsdHint = styled.span`
  color: #4a5568;
  font-size: 0.62rem;
  font-weight: 400;
  line-height: 1;
`;

const NumCell = styled.span`
  color: #bfc1c8;
  text-align: right;
  z-index: 1;
`;

const SpreadLine = styled.div`
  text-align: center;
  padding: 0.2rem;
  color: #4a5568;
  font-size: 0.67rem;
  background: #0f1522;
  border-top: 1px solid #1a2233;
  border-bottom: 1px solid #1a2233;
  flex-shrink: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  .sv { color: #7e8b99; font-weight: 500; }
`;

const TradeBtn = styled.button`
  margin: 0.35rem 0.5rem;
  padding: 0.38rem;
  background: #1e3a6e;
  color: #5087f2;
  border: 1px solid #2a4a8a;
  border-radius: 3px;
  font-size: 0.8rem;
  font-weight: 600;
  cursor: pointer;
  flex-shrink: 0;
  transition: opacity 0.15s;
  &:hover { opacity: 0.82; }
  &:disabled { opacity: 0.4; cursor: not-allowed; }
`;

const EmptyBook = styled.div`
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #4a5568;
  font-size: 0.78rem;
`;

// ── OrderBookPanel ──────────────────────────────────────────────────────────
// Isolated sub-component. All market data arrives via Tauri events from the
// Rust backend WS tasks; no frontend WebSocket code here.

interface OBPanelProps {
  account: Account;
  symbol: string;          // canonical system symbol
  exchangeSymbol: string;  // venue-native symbol (for subscribe / display)
  kind: string;
  rawInstruments: ReferenceData[];
  onConnectedChange: (connected: boolean) => void;
}

const OrderBookPanel: FunctionComponent<OBPanelProps> = memo(({
  account, symbol, exchangeSymbol, kind, rawInstruments, onConnectedChange,
}) => {
  const dispatch = useAppDispatch();

  const [bids, setBids]     = useState<BookMap>(new Map());
  const [asks, setAsks]     = useState<BookMap>(new Map());
  const [ticker, setTicker] = useState<Ticker | null>(null);

  const pendingBidsRef   = useRef<BookMap>(new Map());
  const pendingAsksRef   = useRef<BookMap>(new Map());
  const pendingTickerRef = useRef<Ticker | null>(null);
  const bookDirtyRef     = useRef(false);
  const tickerDirtyRef   = useRef(false);

  // Flush pending data into React state at ~10 fps.
  useEffect(() => {
    const id = setInterval(() => {
      if (bookDirtyRef.current) {
        setBids(new Map(pendingBidsRef.current));
        setAsks(new Map(pendingAsksRef.current));
        bookDirtyRef.current = false;
      }
      if (tickerDirtyRef.current) {
        setTicker(pendingTickerRef.current);
        tickerDirtyRef.current = false;
      }
    }, 100);
    return () => clearInterval(id);
  }, []);

  // Subscribe to backend market data via Tauri events.
  useEffect(() => {
    if (!exchangeSymbol || !symbol) {
      onConnectedChange(false);
      return;
    }

    let cancelled = false;
    const unlisteners: Array<() => void> = [];

    // Reset book / ticker on every new subscription.
    setBids(new Map()); setAsks(new Map()); setTicker(null);
    pendingBidsRef.current = new Map(); pendingAsksRef.current = new Map();
    pendingTickerRef.current = null; bookDirtyRef.current = false; tickerDirtyRef.current = false;

    invoke('subscribe_market_data', {
      exchange: account.exchange,
      exchangeSymbol,
      symbol,
      kind,
      accountId: account.exchange === 'coincall' ? account.id : undefined,
    }).then(() => { if (!cancelled) onConnectedChange(true); }).catch(console.error);

    listen<MarketBookEvent>('market://book', (event) => {
      const e = event.payload;
      if (e.symbol !== symbol || e.exchange !== account.exchange) return;
      const bm = new Map<number, number>();
      const am = new Map<number, number>();
      for (const [p, s] of e.bids) bm.set(p, s);
      for (const [p, s] of e.asks) am.set(p, s);
      pendingBidsRef.current = bm;
      pendingAsksRef.current = am;
      bookDirtyRef.current = true;
    }).then(u => { if (cancelled) u(); else unlisteners.push(u); });

    listen<MarketTickerEvent>('market://ticker', (event) => {
      const e = event.payload;
      if (e.symbol !== symbol || e.exchange !== account.exchange) return;
      pendingTickerRef.current = {
        instrument_name: e.exchangeSymbol,
        best_bid_price:  e.bid,
        best_ask_price:  e.ask,
        last_price:      e.last,
        mark_price:      e.mark,
        index_price:     e.index,
        open_interest:   e.openInterest,
        stats: { high: e.high24h, low: e.low24h, price_change: e.priceChange24h, volume: e.volume24h },
        mark_iv: e.markIv,
        bid_iv:  e.bidIv,
        ask_iv:  e.askIv,
        delta:   e.delta,
        gamma:   e.gamma,
        vega:    e.vega,
        theta:   e.theta,
      };
      tickerDirtyRef.current = true;
    }).then(u => { if (cancelled) u(); else unlisteners.push(u); });

    return () => {
      cancelled = true;
      invoke('unsubscribe_market_data', { exchange: account.exchange, exchangeSymbol }).catch(() => {});
      for (const u of unlisteners) u();
      onConnectedChange(false);
    };
  }, [symbol, exchangeSymbol, account.exchange, account.id, kind]); // eslint-disable-line react-hooks/exhaustive-deps

  const fmtP   = (n: number) => n >= 1000 ? n.toLocaleString('en-US', { minimumFractionDigits: 1, maximumFractionDigits: 1 }) : n.toFixed(4);
  const fmtN   = (n: number) => n.toLocaleString('en-US', { maximumFractionDigits: 4 });
  const fmtPct = (n: number) => (n >= 0 ? '+' : '') + n.toFixed(2) + '%';
  const fmtVol = (n: number) => n >= 1_000_000 ? (n / 1_000_000).toFixed(2) + 'M'
    : n >= 1_000 ? (n / 1_000).toFixed(1) + 'K' : n.toFixed(2);

  const askLevels = buildLevels(asks, 'ask', LEVELS);
  const bidLevels = buildLevels(bids, 'bid', LEVELS);
  const bestBid   = bidLevels[0]?.price;
  const bestAsk   = askLevels[0]?.price;
  const spread    = bestBid != null && bestAsk != null ? bestAsk - bestBid : null;
  const mid       = bestBid != null && bestAsk != null ? (bestBid + bestAsk) / 2 : null;
  const hasBook   = bidLevels.length > 0 || askLevels.length > 0;
  const pctChg    = ticker?.stats.price_change;

  const handleTrade = () => {
    dispatch(setActiveAccount(account.id));
    dispatch(setInstruments(rawInstruments));
    dispatch(setSelectedInstrument(symbol));
    dispatch(setExchangeSymbol(exchangeSymbol));
  };

  const handleBookRowClick = (price: number, side: 'buy' | 'sell') => {
    dispatch(setActiveAccount(account.id));
    dispatch(setInstruments(rawInstruments));
    dispatch(setSelectedInstrument(symbol));
    dispatch(setExchangeSymbol(exchangeSymbol));
    dispatch(setPriceFromBook({ price, side }));
  };

  return (
    <>
      {/* Ticker: 4 cols × 2 rows */}
      <TickerBar>
        <TickerCell>
          <span className="lbl">Last</span>
          <span className="val">{ticker?.last_price != null ? fmtP(ticker.last_price) : '—'}</span>
        </TickerCell>
        <TickerCell>
          <span className="lbl">Mark</span>
          <span className="val">{ticker?.mark_price != null ? fmtP(ticker.mark_price) : '—'}</span>
        </TickerCell>
        <TickerCell>
          <span className="lbl">Index</span>
          <span className="val dim">{ticker?.index_price != null ? fmtP(ticker.index_price) : '—'}</span>
        </TickerCell>
        <TickerCell>
          <span className="lbl">24h %</span>
          <span className={`val ${(pctChg ?? 0) >= 0 ? 'pos' : 'neg'}`}>
            {pctChg != null ? fmtPct(pctChg) : '—'}
          </span>
        </TickerCell>
        <TickerCell>
          <span className="lbl">Bid</span>
          <span className="val bid">{ticker?.best_bid_price != null ? fmtP(ticker.best_bid_price) : (bestBid != null ? fmtP(bestBid) : '—')}</span>
        </TickerCell>
        <TickerCell>
          <span className="lbl">Ask</span>
          <span className="val ask">{ticker?.best_ask_price != null ? fmtP(ticker.best_ask_price) : (bestAsk != null ? fmtP(bestAsk) : '—')}</span>
        </TickerCell>
        <TickerCell>
          <span className="lbl">Spread</span>
          <span className="val">{spread != null ? (mid ? `${(spread / mid * 100).toFixed(3)}%` : fmtP(spread)) : '—'}</span>
        </TickerCell>
        <TickerCell>
          <span className="lbl">24h Vol</span>
          <span className="val dim">{ticker?.stats.volume != null ? fmtVol(ticker.stats.volume) : '—'}</span>
        </TickerCell>
      </TickerBar>

      {/* Orderbook */}
      <BookWrapper>
        <BookHeader>
          <span>PRICE{account.exchange === 'deribit' && kind === 'option' && ticker?.index_price ? ' / USD' : ''}</span>
          <span style={{ textAlign: 'right' }}>SIZE</span>
          <span style={{ textAlign: 'right' }}>TOTAL</span>
        </BookHeader>
        {!hasBook ? (
          <EmptyBook>{exchangeSymbol ? 'Waiting for data…' : 'Select an instrument'}</EmptyBook>
        ) : (
          <>
            <BookSection $reverse>
              {askLevels.map(l => (
                <BookRow key={l.price} $side="ask"
                  style={{ '--depth': `${l.depth.toFixed(1)}%` } as React.CSSProperties}
                  onClick={() => handleBookRowClick(l.price, 'buy')}
                  title={`Buy at ${fmtP(l.price)}`}>
                  <PriceCell $side="ask">
                    {fmtP(l.price)}
                    {account.exchange === 'deribit' && kind === 'option' && ticker?.index_price != null && (
                      <UsdHint>${(l.price * ticker.index_price).toLocaleString('en-US', { maximumFractionDigits: 0 })}</UsdHint>
                    )}
                  </PriceCell>
                  <NumCell style={{ textAlign: 'right' }}>{fmtN(l.size)}</NumCell>
                  <NumCell style={{ textAlign: 'right' }}>{fmtN(l.total)}</NumCell>
                </BookRow>
              ))}
            </BookSection>
            <SpreadLine>
              {spread != null
                ? <><span className="sv">{fmtP(spread)}</span>{mid ? ` · ${(spread / mid * 100).toFixed(3)}%` : ''}</>
                : '—'}
            </SpreadLine>
            <BookSection>
              {bidLevels.map(l => (
                <BookRow key={l.price} $side="bid"
                  style={{ '--depth': `${l.depth.toFixed(1)}%` } as React.CSSProperties}
                  onClick={() => handleBookRowClick(l.price, 'sell')}
                  title={`Sell at ${fmtP(l.price)}`}>
                  <PriceCell $side="bid">
                    {fmtP(l.price)}
                    {account.exchange === 'deribit' && kind === 'option' && ticker?.index_price != null && (
                      <UsdHint>${(l.price * ticker.index_price).toLocaleString('en-US', { maximumFractionDigits: 0 })}</UsdHint>
                    )}
                  </PriceCell>
                  <NumCell style={{ textAlign: 'right' }}>{fmtN(l.size)}</NumCell>
                  <NumCell style={{ textAlign: 'right' }}>{fmtN(l.total)}</NumCell>
                </BookRow>
              ))}
            </BookSection>
          </>
        )}
      </BookWrapper>

      {/* Trade */}
      <TradeBtn onClick={handleTrade} disabled={!exchangeSymbol}>
        ⚡ Trade {exchangeSymbol || '…'}
      </TradeBtn>
    </>
  );
});

// ── Component ──────────────────────────────────────────────────────────────

interface Props { account: Account; }

const ExchangePanel: FunctionComponent<Props> = ({ account }) => {
  const general   = useAppSelector(selectGeneral);

  // Compute filtered base currencies from watchedCoins setting
  const availableBaseCurrencies = useMemo(() => {
    if (!general.watchedCoins) return BASE_CURRENCIES;
    const watched = general.watchedCoins.split(',').map(s => s.trim().toUpperCase()).filter(Boolean);
    // Only include coins that appear in BASE_CURRENCIES AND watchedCoins (exclude USDT/USDC/USD)
    const filtered = BASE_CURRENCIES.filter(c => watched.includes(c));
    return filtered.length > 0 ? filtered : BASE_CURRENCIES;
  }, [general.watchedCoins]);

  // Selector state — default baseCcy to first available filtered currency
  const [baseCcy, setBaseCcy]           = useState(() =>
    general.watchedCoins
      ? (general.watchedCoins.split(',').map(s => s.trim().toUpperCase()).find(c => BASE_CURRENCIES.includes(c)) ?? 'BTC')
      : 'BTC'
  );
  const [kind, setKind]                 = useState('future');

  // If watchedCoins changes and baseCcy is no longer in filtered list, reset to first available
  useEffect(() => {
    if (!availableBaseCurrencies.includes(baseCcy)) {
      setBaseCcy(availableBaseCurrencies[0] ?? 'BTC');
    }
  }, [availableBaseCurrencies]); // eslint-disable-line react-hooks/exhaustive-deps
  const [rawInstruments, setRawInst]    = useState<ReferenceData[]>([]);
  const [quote, setQuote]               = useState('USD');
  const [selExpiryKey, setSelExpiryKey] = useState('PERP');
  const [strike, setStrike]             = useState<number>(0);
  const [optionSide, setOptionSide]     = useState<'C' | 'P'>('C');

  // Connected state for header ConnDot — bubbled up from OrderBookPanel child
  const [connected, setConnected] = useState(false);

  const instrumentCacheRef = useRef<Map<string, ReferenceData[]>>(new Map());

  // Derive settlement: USD-quoted = inverse (coin-margined), else = linear
  const settlement = useMemo((): 'linear' | 'inverse' =>
    quote === 'USD' ? 'inverse' : 'linear', [quote]);

  // ── Cascading derived selections ──────────────────────────────────────────

  const availableQuotes = useMemo(() =>
    [...new Set(rawInstruments.map(r => r.quote))].sort(),
    [rawInstruments]);

  const byQuote = useMemo(() => {
    if (kind === 'option') return rawInstruments;
    const filtered = rawInstruments.filter(r => r.quote === quote);
    return filtered.length > 0 ? filtered : rawInstruments;
  }, [rawInstruments, quote, kind]);

  const availableExpiries = useMemo((): ExpiryOption[] => {
    const seen = new Set<string>();
    const result: ExpiryOption[] = [];
    const sorted = [...byQuote].sort((a, b) => {
      const ak = expiryKey(a), bk = expiryKey(b);
      if (ak === 'PERP') return -1; if (bk === 'PERP') return 1;
      return +ak - +bk;
    });
    for (const r of sorted) {
      const key = expiryKey(r);
      if (!seen.has(key)) { seen.add(key); result.push({ key, label: formatExpiryKey(key) }); }
    }
    return result;
  }, [byQuote]);

  const byExpiry = useMemo(() => {
    const filtered = byQuote.filter(r => expiryKey(r) === selExpiryKey);
    return filtered.length > 0 ? filtered : byQuote;
  }, [byQuote, selExpiryKey]);

  const availableStrikes = useMemo(() => {
    if (kind !== 'option') return [];
    return [...new Set(byExpiry.map(r => r.strike).filter((s): s is number => s != null))]
      .sort((a, b) => a - b);
  }, [byExpiry, kind]);

  // The selected ReferenceData entry
  const selectedRef = useMemo((): ReferenceData | null => {
    if (rawInstruments.length === 0) return null;
    if (kind === 'spot')   return byQuote[0] ?? null;
    if (kind === 'future') return byExpiry[0] ?? null;
    const exact = byExpiry.find(r => r.strike === strike && r.optionType === optionSide);
    if (exact) return exact;
    return byExpiry.find(r => r.optionType === optionSide) ?? null;
  }, [rawInstruments, kind, byQuote, byExpiry, strike, optionSide]);

  const symbol = selectedRef?.symbol ?? '';

  const exchangeSymbol = useMemo(() => {
    if (!selectedRef) return '';
    return selectedRef.venues.find(v => v.exchange === account.exchange)?.exchangeSymbol ?? '';
  }, [selectedRef, account.exchange]);

  // ── Cascade: compute quote→expiry→strike in one pass when instruments load ──
  useEffect(() => {
    if (rawInstruments.length === 0) return;

    if (kind === 'option') {
      const expiryKeys = [...new Set(rawInstruments.map(r => expiryKey(r)))];
      const nonPerp = expiryKeys.filter(k => k !== 'PERP').sort((a, b) => +a - +b);
      const bestExpiry = nonPerp[0] ?? expiryKeys[0] ?? 'PERP';
      const byE = rawInstruments.filter(r => expiryKey(r) === bestExpiry);
      const strikes = [...new Set(byE.map(r => r.strike).filter((s): s is number => s != null))]
        .sort((a, b) => a - b);
      setSelExpiryKey(bestExpiry);
      if (strikes.length > 0) setStrike(strikes[0]);
    } else {
      const quotes = [...new Set(rawInstruments.map(r => r.quote))].sort();
      const bestQuote = quotes.includes('USD') ? 'USD'
        : quotes.includes('USDT') ? 'USDT'
        : quotes[0] ?? 'USD';
      const byQ = rawInstruments.filter(r => r.quote === bestQuote);
      const expiryKeys = [...new Set(byQ.map(r => expiryKey(r)))];
      const hasPerp = expiryKeys.includes('PERP');
      const nonPerp = expiryKeys.filter(k => k !== 'PERP').sort((a, b) => +a - +b);
      const bestExpiry = hasPerp ? 'PERP' : (nonPerp[0] ?? 'PERP');
      setQuote(bestQuote);
      setSelExpiryKey(bestExpiry);
    }
  }, [rawInstruments]); // eslint-disable-line react-hooks/exhaustive-deps

  // When user manually changes quote (futures only) → reset expiry
  useEffect(() => {
    if (rawInstruments.length === 0 || kind === 'option') return;
    const byQ = rawInstruments.filter(r => r.quote === quote);
    const expiryKeys = [...new Set(byQ.map(r => expiryKey(r)))];
    const hasPerp = expiryKeys.includes('PERP');
    const nonPerp = expiryKeys.filter(k => k !== 'PERP').sort((a, b) => +a - +b);
    setSelExpiryKey(hasPerp ? 'PERP' : (nonPerp[0] ?? 'PERP'));
  }, [quote]); // eslint-disable-line react-hooks/exhaustive-deps

  // When user manually changes expiry → reset strike (options only)
  useEffect(() => {
    if (rawInstruments.length === 0 || kind !== 'option') return;
    const byE = rawInstruments.filter(r => expiryKey(r) === selExpiryKey);
    const strikes = [...new Set(byE.map(r => r.strike).filter((s): s is number => s != null))]
      .sort((a, b) => a - b);
    if (strikes.length > 0) setStrike(strikes[0]);
  }, [selExpiryKey]); // eslint-disable-line react-hooks/exhaustive-deps

  // ── Fetch reference data ──────────────────────────────────────────────────

  useEffect(() => {
    const cacheKey = `${account.exchange}|${baseCcy}|${kind}`;
    const cached = instrumentCacheRef.current.get(cacheKey);
    if (cached) {
      setRawInst(cached);
      return;
    }
    setRawInst([]);
    setQuote('USD');
    setSelExpiryKey('PERP');
    setStrike(0);
    invoke<ReferenceData[]>('fetch_reference_data', { exchange: account.exchange, currency: baseCcy, kind })
      .then(list => {
        const sorted = [...list].sort((a, b) => a.symbol.localeCompare(b.symbol));
        instrumentCacheRef.current.set(cacheKey, sorted);
        setRawInst(sorted);
      })
      .catch(console.error);
  }, [account.exchange, baseCcy, kind]);

  // ── Render ────────────────────────────────────────────────────────────────

  return (
    <Panel>
      {/* Header */}
      <PanelHeader>
        <ExBadge $exchange={account.exchange}>{account.exchange}</ExBadge>
        <AccountName title={account.name}>{account.name}</AccountName>
        {kind !== 'spot' && (
          <SettleBadge $inverse={settlement === 'inverse'}
            title={settlement === 'inverse' ? 'Coin-margined (Inverse)' : 'USDT-margined (Linear)'}>
            {settlement === 'inverse' ? 'INV' : 'LIN'}
          </SettleBadge>
        )}
        {exchangeSymbol && <InstrLabel title={symbol}>{exchangeSymbol}</InstrLabel>}
        <ConnDot $connected={connected} title={connected ? 'Live' : 'Disconnected'} />
      </PanelHeader>

      {/* Instrument Selectors */}
      <SelectorsBlock>
        {/* Row 1: Base + Kind */}
        <SelectorRow>
          <SLabel>
            Base
            <TinySelect
              value={baseCcy}
              onChange={v => setBaseCcy(v)}
              options={availableBaseCurrencies.map(c => ({ value: c, label: c }))}
            />
          </SLabel>
          <SLabel>
            Kind
            <TinySelect
              value={kind}
              onChange={v => setKind(v)}
              options={KINDS.map(k => ({ value: k, label: k.charAt(0).toUpperCase() + k.slice(1) }))}
            />
          </SLabel>
        </SelectorRow>

        {/* Row 2: Quote + Expiry (futures & options) */}
        {(kind === 'future' || kind === 'option') && (
          <SelectorRow>
            <SLabel>
              Quote
              <TinySelect
                value={quote}
                onChange={v => setQuote(v)}
                options={availableQuotes.length === 0
                  ? [{ value: 'USD', label: 'USD' }]
                  : availableQuotes.map(q => ({ value: q, label: q }))}
              />
            </SLabel>
            <SLabel style={{ flex: 2 }}>
              Expiry
              <TinySelect
                value={selExpiryKey}
                onChange={v => setSelExpiryKey(v)}
                options={availableExpiries.length === 0
                  ? [{ value: 'PERP', label: 'PERP' }]
                  : availableExpiries.map(e => ({ value: e.key, label: e.label }))}
              />
            </SLabel>
          </SelectorRow>
        )}

        {/* Row 2b: Quote (spot) */}
        {kind === 'spot' && (
          <SelectorRow>
            <SLabel>
              Quote
              <TinySelect
                value={quote}
                onChange={v => setQuote(v)}
                options={availableQuotes.length === 0
                  ? [{ value: 'USDT', label: 'USDT' }]
                  : availableQuotes.map(q => ({ value: q, label: q }))}
              />
            </SLabel>
          </SelectorRow>
        )}

        {/* Row 3: Strike + Call/Put (options only) */}
        {kind === 'option' && (
          <SelectorRow>
            <SLabel style={{ flex: 3 }}>
              Strike
              <TinySelect
                value={strike}
                onChange={v => setStrike(+v)}
                options={availableStrikes.length === 0
                  ? [{ value: 0, label: '—' }]
                  : availableStrikes.map(s => ({ value: s, label: s.toLocaleString() }))}
              />
            </SLabel>
            <SLabel>
              Side
              <CPToggle>
                <CPBtn $active={optionSide === 'C'} $side="C" onClick={() => setOptionSide('C')}>Call</CPBtn>
                <CPBtn $active={optionSide === 'P'} $side="P" onClick={() => setOptionSide('P')}>Put</CPBtn>
              </CPToggle>
            </SLabel>
          </SelectorRow>
        )}
      </SelectorsBlock>

      {/* Ticker + Book + Trade button — isolated in memo child so backend events never re-render selectors */}
      <OrderBookPanel
        account={account}
        symbol={symbol}
        exchangeSymbol={exchangeSymbol}
        kind={kind}
        rawInstruments={rawInstruments}
        onConnectedChange={setConnected}
      />
    </Panel>
  );
};

export default ExchangePanel;
