import { FunctionComponent, useState, useEffect, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import styled from 'styled-components';
import { useAppSelector, useAppDispatch } from '../../hooks';
import { selectAccounts, selectRfqSettings, Account, RfqSettings, setRfqSettings } from '../Settings/settingsSlice';

// ── Types ──────────────────────────────────────────────────────────────────

interface RfqLeg {
  instrumentName: string;
  side: 'BUY' | 'SELL';
  qty: string;
}

interface RfqLegFull extends RfqLeg {
  price?: string;
}

interface ActiveRfq {
  requestId: string;
  createTime: number;
  expiryTime: number;
  state: string;
  legs: RfqLeg[];
}

interface QuoteLeg {
  instrumentName: string;
  side: string;
  price: string;
  quantity: string;
}

interface Quote {
  quoteId: string;
  requestId: string;
  userId: string;
  state: string;
  createTime: number;
  expiryTime: number;
  legs: QuoteLeg[];
}

// From Rust bs::LegPriceResult
interface Greeks {
  delta: number;
  gamma: number;
  vega:  number;
  theta: number;
  rho:   number;
}

interface LegPriceResult {
  instrumentName: string;
  isOption: boolean;
  quotedPrice: number;
  fairValue: number | null;
  diff: number | null;
  iv: number | null;
  spotUsed: number | null;
  greeks: Greeks | null;
}

interface PricingMarketData {
  indexPrice:      number | null;
  underlyingPrice: number | null;  // forward price used in BS model
  markPrice:       number | null;  // option mark price (native units: BTC on Deribit, USD on Bybit/CoInCall)
  /** Pre-computed fair value in USD (Deribit: markPrice×underlying; Bybit/CoInCall: markPrice directly). */
  fairValueUsd:    number | null;
  bidPrice:        number | null;  // top-of-book bid (native units, for reference only)
  askPrice:        number | null;  // top-of-book ask (native units, for reference only)
  markIv:          number | null;
  delta:           number | null;
  gamma:           number | null;
  vega:            number | null;
  theta:           number | null;
  instrumentUsed:  string;
  error:           string | null;
}

interface OrderbookLevel {
  price: number;
  size:  number;
}

interface OrderbookSnapshot {
  instrumentName: string;
  bids: OrderbookLevel[];
  asks: OrderbookLevel[];
  timestamp: number;
}

/** Compute volume-weighted average price sweeping through book levels up to `qty`.
 *  Returns { vwap, filled } — filled < qty if book doesn't have enough liquidity. */
function vwapThrough(levels: OrderbookLevel[], qty: number): { vwap: number; filled: number } {
  let remaining = qty;
  let totalValue = 0;
  let totalFilled = 0;
  for (const lvl of levels) {
    if (remaining <= 0) break;
    const fill = Math.min(remaining, lvl.size);
    totalValue  += fill * lvl.price;
    totalFilled += fill;
    remaining   -= fill;
  }
  return totalFilled > 0 ? { vwap: totalValue / totalFilled, filled: totalFilled } : { vwap: 0, filled: 0 };
}

interface PortfolioGreeks {
  positionDelta: number;
  gamma:         number;
  vega:          number;
  theta:         number;
  positionCount: number;
  coinBalance:   number;
  usdtBalance:   number;
  spotPrice:     number;
  balanceDelta:  number;
  totalDelta:    number;
}

// ── Styles ─────────────────────────────────────────────────────────────────

const Wrapper = styled.div`
  display: flex;
  flex-direction: column;
  height: 100%;
  background: #0d1117;
  overflow: hidden;
`;

const TopBar = styled.div`
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.6rem 1rem;
  border-bottom: 1px solid #1e2738;
  background: #0f1522;
  flex-shrink: 0;
  flex-wrap: wrap;
`;

const Label = styled.div`
  font-size: 0.78rem;
  color: #7e8b99;
  display: flex;
  align-items: center;
  gap: 0.4rem;
`;

const Select = styled.select`
  background: #131c2e;
  color: #e8edf4;
  border: 1px solid #1e2738;
  border-radius: 3px;
  padding: 0.25rem 0.4rem;
  font-size: 0.82rem;
  outline: none;
  &:focus { border-color: #5087f2; }
`;

const Body = styled.div`
  display: flex;
  flex: 1;
  overflow: hidden;
  gap: 0;
`;

const Col = styled.div<{ $width?: string }>`
  width: ${p => p.$width ?? '360px'};
  min-width: ${p => p.$width ?? '360px'};
  display: flex;
  flex-direction: column;
  border-right: 1px solid #1e2738;
  overflow: hidden;
`;

const ColRight = styled.div`
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  min-width: 380px;
`;

const ColHeader = styled.div`
  padding: 0.5rem 0.75rem;
  border-bottom: 1px solid #1e2738;
  font-size: 0.78rem;
  font-weight: 600;
  color: #7e8b99;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-shrink: 0;
`;

const Scroll = styled.div`
  flex: 1;
  overflow-y: auto;
  padding: 0.5rem;
  &::-webkit-scrollbar { width: 4px; }
  &::-webkit-scrollbar-thumb { background: #2a3a52; border-radius: 2px; }
`;

const Card = styled.div<{ $selected?: boolean }>`
  background: ${p => p.$selected ? '#1a2a40' : '#131c2e'};
  border: 1px solid ${p => p.$selected ? '#5087f2' : '#1e2738'};
  border-radius: 4px;
  padding: 0.6rem 0.75rem;
  margin-bottom: 0.4rem;
  cursor: pointer;
  transition: border-color 0.15s;
  &:hover { border-color: #334a6a; }
`;

const CardTitle = styled.div`
  font-size: 0.82rem;
  font-weight: 600;
  color: #e8edf4;
  margin-bottom: 0.3rem;
  display: flex;
  align-items: center;
  justify-content: space-between;
`;

const CardMeta = styled.div`
  font-size: 0.75rem;
  color: #7e8b99;
`;

const LegRow = styled.div`
  display: flex;
  align-items: center;
  gap: 0.3rem;
  margin: 0.25rem 0;
  flex-wrap: wrap;
`;

const Input = styled.input`
  background: #131c2e;
  color: #e8edf4;
  border: 1px solid #1e2738;
  border-radius: 3px;
  padding: 0.25rem 0.4rem;
  font-size: 0.82rem;
  outline: none;
  flex: 1;
  min-width: 0;
  &:focus { border-color: #5087f2; }
`;

const SmallInput = styled(Input)`
  width: 90px;
  flex: none;
  font-size: 0.78rem;
`;

const Btn = styled.button<{ $variant?: 'primary' | 'danger' | 'ghost' | 'success' }>`
  padding: 0.25rem 0.6rem;
  border-radius: 3px;
  border: 1px solid ${p =>
    p.$variant === 'danger'  ? '#7b2929' :
    p.$variant === 'primary' ? '#3a5a8c' :
    p.$variant === 'success' ? '#1a5a2a' : '#1e2738'};
  background: ${p =>
    p.$variant === 'danger'  ? '#2a1a1a' :
    p.$variant === 'primary' ? '#1e3558' :
    p.$variant === 'success' ? '#0e2a18' : 'transparent'};
  color: ${p =>
    p.$variant === 'danger'  ? '#e05252' :
    p.$variant === 'primary' ? '#7eb8f7' :
    p.$variant === 'success' ? '#4ade80' : '#7e8b99'};
  font-size: 0.78rem;
  cursor: pointer;
  white-space: nowrap;
  transition: all 0.12s;
  &:hover { opacity: 0.85; }
  &:disabled { opacity: 0.4; cursor: not-allowed; }
`;

const StatusBadge = styled.span<{ $state: string }>`
  font-size: 0.7rem;
  padding: 0.1rem 0.4rem;
  border-radius: 10px;
  background: ${p =>
    p.$state === 'ACTIVE'    ? '#173a1a' :
    p.$state === 'FILLED'    ? '#1a3a26' :
    p.$state === 'EXPIRED' || p.$state === 'CANCELLED' ? '#2a1a1a' : '#1e2738'};
  color: ${p =>
    p.$state === 'ACTIVE'    ? '#4ade80' :
    p.$state === 'FILLED'    ? '#34d399' :
    p.$state === 'EXPIRED' || p.$state === 'CANCELLED' ? '#e05252' : '#7e8b99'};
  border: 1px solid ${p =>
    p.$state === 'ACTIVE'    ? '#2a5a2f' :
    p.$state === 'FILLED'    ? '#256040' :
    p.$state === 'EXPIRED' || p.$state === 'CANCELLED' ? '#5a2a2a' : '#1e2738'};
`;

const SideBadge = styled.span<{ $side: string }>`
  font-size: 0.72rem;
  padding: 0.1rem 0.35rem;
  border-radius: 3px;
  background: ${p => p.$side.toUpperCase() === 'BUY' ? '#0e2a1a' : '#2a0e0e'};
  color: ${p => p.$side.toUpperCase() === 'BUY' ? '#4ade80' : '#e05252'};
  font-weight: 600;
  text-transform: uppercase;
`;

const Empty = styled.div`
  text-align: center;
  padding: 2rem;
  color: #4a5568;
  font-size: 0.82rem;
`;

const Err = styled.div`
  color: #e05252;
  font-size: 0.78rem;
  padding: 0.4rem 0.5rem;
  background: #1a0e0e;
  border: 1px solid #5a2a2a;
  border-radius: 3px;
  margin-bottom: 0.4rem;
`;

const CreateForm = styled.div`
  padding: 0.75rem;
`;

const Row = styled.div`
  display: flex;
  gap: 0.4rem;
  margin-bottom: 0.4rem;
  align-items: center;
`;

const SectionTitle = styled.div`
  font-size: 0.75rem;
  color: #5087f2;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  margin: 0.5rem 0 0.35rem;
`;

// Pricer styled components
const PricerTable = styled.table`
  width: 100%;
  border-collapse: collapse;
  font-size: 0.75rem;
`;

const PTh = styled.th`
  text-align: left;
  padding: 0.25rem 0.4rem;
  color: #4a5568;
  font-weight: 600;
  border-bottom: 1px solid #1e2738;
  white-space: nowrap;
`;

const PTd = styled.td<{ $align?: string }>`
  padding: 0.25rem 0.4rem;
  color: #c8d6e5;
  border-bottom: 1px solid #131c2e;
  text-align: ${p => p.$align ?? 'left'};
  white-space: nowrap;
`;

const DiffCell = styled.td<{ $pos?: boolean; $neg?: boolean }>`
  padding: 0.25rem 0.4rem;
  text-align: right;
  font-weight: 600;
  color: ${p => p.$pos ? '#4ade80' : p.$neg ? '#e05252' : '#7e8b99'};
  border-bottom: 1px solid #131c2e;
`;

const SpotRow = styled.div`
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin-bottom: 0.35rem;
  font-size: 0.78rem;
`;

const SpotLabel = styled.span`
  color: #7e8b99;
  min-width: 70px;
`;

const NetBox = styled.div`
  background: #0f1a2c;
  border: 1px solid #1e2738;
  border-radius: 4px;
  padding: 0.5rem 0.75rem;
  display: flex;
  gap: 1.5rem;
  flex-wrap: wrap;
  margin-top: 0.5rem;
`;

const NetStat = styled.div`
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
`;

const NetLabel = styled.span`
  font-size: 0.72rem;
  color: #4a5568;
`;

const NetValue = styled.span<{ $color?: string }>`
  font-size: 0.88rem;
  font-weight: 600;
  color: ${p => p.$color ?? '#e8edf4'};
  font-family: monospace;
`;

const MdSourceRow = styled.div`
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.75rem;
  color: #4a5568;
  margin-top: 0.2rem;
`;

// ── Component ──────────────────────────────────────────────────────────────

const EXCHANGES_PRICER = ['deribit', 'okx', 'bybit', 'coincall'] as const;

const RfqPanel: FunctionComponent = () => {
  const dispatch    = useAppDispatch();
  const accounts    = useAppSelector(selectAccounts);
  const rfqDefaults = useAppSelector(selectRfqSettings);
  const ccAccounts  = accounts.filter(a => a.exchange === 'coincall');

  const [mode, setMode]                 = useState<'maker' | 'taker' | 'history'>('maker');
  const [tradingCoin, setTradingCoin]   = useState<string>('BTC');
  const [selectedId, setSelectedId]     = useState<string>('');
  const [rfqs, setRfqs]                 = useState<ActiveRfq[]>([]);
  const [selectedRfq, setSelectedRfq]   = useState<ActiveRfq | null>(null);
  const [quotes, setQuotes]             = useState<Quote[]>([]);
  const [selectedQuote, setSelectedQuote] = useState<Quote | null>(null);
  const [loadingRfqs, setLoadingRfqs]   = useState(false);
  const [loadingQuotes, setLoadingQuotes] = useState(false);
  const [acceptLoading, setAcceptLoading] = useState(false);
  const [rfqError, setRfqError]         = useState<string | null>(null);
  const [submitError, setSubmitError]   = useState('');
  const [submitLoading, setSubmitLoading] = useState(false);
  const [reloadQuotesTrigger, setReloadQuotesTrigger] = useState(0);

  // Pricer state
  const [spotPrices, setSpotPrices]     = useState<Record<string, string>>({});
  const [pricerExchange, setPricerExchange] = useState<string>('deribit');
  const [legResults, setLegResults]     = useState<LegPriceResult[]>([]);
  const [legExchangeData, setLegExchangeData] = useState<Record<string, PricingMarketData>>({});
  const [legOrderbooks, setLegOrderbooks]     = useState<Record<string, OrderbookSnapshot>>({});
  const [pricingLoading, setPricingLoading] = useState(false);
  const [mdError, setMdError]           = useState<string | null>(null);

  // Maker: per-leg editable quote prices (keyed by instrumentName)
  const [quotePrices, setQuotePrices]   = useState<Record<string, string>>({});
  const [legSkews, setLegSkews]         = useState<Record<string, number>>({});
  const [quoteSubmitting, setQuoteSubmitting] = useState(false);
  const [quoteError, setQuoteError]     = useState('');
  const [quoteSuccess, setQuoteSuccess] = useState('');

  // Auto-quote: local toggle per account+coin (not a global setting)
  const [autoQuoteEnabled, setAutoQuoteEnabled] = useState(false);
  // Refs so loadRfqs (memoized) can always read latest values without re-creating
  const autoQuoteEnabledRef = useRef(false);
  const tradingCoinRef = useRef(tradingCoin);
  const autoQuoteRfqRef = useRef<((rfq: ActiveRfq) => Promise<void>) | null>(null);
  // tracks which RFQ IDs have been auto-quoted to avoid re-quoting on polls
  const autoQuotedRef = useRef<Set<string>>(new Set());
  // Map of requestId → cancel timer
  const autoQuoteTimersRef = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());

  // Portfolio Greeks (maker mode) — fetched from open positions
  const [portfolioGreeks, setPortfolioGreeks] = useState<PortfolioGreeks | null>(null);
  const [portfolioLoading, setPortfolioLoading] = useState(false);

  // Taker: create form
  const [legs, setLegs] = useState<RfqLegFull[]>([
    { instrumentName: '', side: 'BUY', qty: '' },
    { instrumentName: '', side: 'SELL', qty: '' },
  ]);

  const account = ccAccounts.find(a => a.id === selectedId) as Account | undefined;

  // Auto-select first CoInCall account
  useEffect(() => {
    if (ccAccounts.length > 0 && !selectedId) setSelectedId(ccAccounts[0].id);
  }, [ccAccounts.length]); // eslint-disable-line react-hooks/exhaustive-deps

  // Load RFQ settings defaults
  useEffect(() => {
    invoke<RfqSettings>('get_rfq_settings').then(s => {
      // Sync new Greek-spread settings into Redux for use in priceSeekLegs
      dispatch(setRfqSettings(s));
      setPricerExchange(s.pricerExchange ?? (s as any).spotSource ?? 'deribit');
      if (s.tradingCoin) setTradingCoin(s.tradingCoin);
    }).catch(() => {
      setPricerExchange(rfqDefaults.pricerExchange);
    });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Keep refs in sync so memoized callbacks always see the latest values
  useEffect(() => { autoQuoteEnabledRef.current = autoQuoteEnabled; }, [autoQuoteEnabled]);
  useEffect(() => { tradingCoinRef.current = tradingCoin; }, [tradingCoin]);

  const loadRfqs = useCallback(async () => {
    if (!selectedId) return;
    setLoadingRfqs(true);
    setRfqError(null);
    try {
      const role     = mode === 'maker' ? 'MAKER' : mode === 'taker' ? 'TAKER' : null;
      const rfqState = mode === 'history' ? 'CLOSED' : null;
      const list = await invoke<ActiveRfq[]>('coincall_get_rfq_list', {
        accountId: selectedId,
        role,
        rfqState,
      });
      console.log('[RfqPanel] loadRfqs mode=' + mode, list);
      const newList = list ?? [];
      setRfqs(newList);

      // Auto-quote: find newly arrived OPEN seeks that haven't been quoted yet
      if (mode === 'maker' && autoQuoteEnabledRef.current && autoQuoteRfqRef.current) {
        const newSeeks = newList.filter(r =>
          r.state === 'OPEN' &&
          r.legs.some(l => l.instrumentName.toUpperCase().includes(tradingCoinRef.current)) &&
          !autoQuotedRef.current.has(r.requestId)
        );
        for (const rfq of newSeeks) {
          autoQuotedRef.current.add(rfq.requestId);
          autoQuoteRfqRef.current(rfq);
        }
      }
    } catch (e) {
      console.error('[RfqPanel] loadRfqs error:', e);
      setRfqError(String(e));
      setRfqs([]);
    }
    finally { setLoadingRfqs(false); }
  }, [selectedId, mode]); // eslint-disable-line react-hooks/exhaustive-deps

  const loadPortfolioGreeks = useCallback(async () => {
    if (!selectedId) return;
    setPortfolioLoading(true);
    try {
      const g = await invoke<PortfolioGreeks>('get_portfolio_greeks', {
        accountId: selectedId,
        tradingCoin: tradingCoin || 'BTC',
      });
      setPortfolioGreeks(g);
    } catch (e) {
      console.warn('[RfqPanel] portfolio greeks failed:', e);
    } finally {
      setPortfolioLoading(false);
    }
  }, [selectedId, tradingCoin]);

  // Listen for block-trade WS events — auto-reload quotes when a new quote arrives
  useEffect(() => {
    const unlisten = listen<{ accountId: string; msgType: number; requestId?: string }>(
      'ws://coincall_rfq_update',
      (event) => {
        const { msgType, requestId } = event.payload;
        console.log('[RfqPanel] ws rfq_update', event.payload);
        // In maker mode, ANY block-trade event may indicate a new/changed seek
        if (mode === 'maker') {
          loadRfqs();
          return;
        }
        // msgType 4 = ADD_QUOTE, 5 = CANCEL_QUOTE, 6 = EXPIRE_QUOTE, 9 = MSG_USER
        if ([4, 5, 6, 9].includes(msgType)) {
          loadRfqs();
          setSelectedRfq(prev => {
            if (prev && requestId && prev.requestId === requestId) {
              setReloadQuotesTrigger(t => t + 1);
            }
            return prev;
          });
        }
        // msgType 1 = new RFQ posted — refresh list
        if (msgType === 1) loadRfqs();
      }
    );
    return () => { unlisten.then(fn => fn()); };
  }, [loadRfqs, mode]); // eslint-disable-line react-hooks/exhaustive-deps

  // Poll for new seeks every 8s in maker mode — CoInCall private WS only delivers
  // events for your own seeks (taker side); new seeks from takers don't push here.
  useEffect(() => {
    if (mode !== 'maker' || !selectedId) return;
    const timer = setInterval(() => { loadRfqs(); }, 8000);
    return () => clearInterval(timer);
  }, [mode, selectedId, loadRfqs]);

  // Load portfolio Greeks on mount in maker mode, and whenever selectedId or mode changes
  useEffect(() => {
    if (mode === 'maker' && selectedId) loadPortfolioGreeks();
  }, [selectedId, mode]); // eslint-disable-line react-hooks/exhaustive-deps

  const loadQuotes = useCallback(async (rfq: ActiveRfq) => {
    if (!selectedId) return;
    setSelectedRfq(rfq);
    setSelectedQuote(null);
    setLegResults([]);
    setMdError(null);
    setLoadingQuotes(true);
    try {
      const list = await invoke<Quote[]>('coincall_get_rfq_quotes', { accountId: selectedId, requestId: rfq.requestId });
      setQuotes(Array.isArray(list) ? list : []);
    } catch (e) {
      setQuotes([]);
      setMdError(`Failed to load quotes: ${String(e)}`);
    } finally { setLoadingQuotes(false); }
  }, [selectedId]);

  // When reloadQuotesTrigger fires (set by WS listener), silently refresh quotes
  useEffect(() => {
    if (reloadQuotesTrigger === 0) return;
    if (!selectedRfq || !selectedId) return;
    invoke<Quote[]>('coincall_get_rfq_quotes', { accountId: selectedId, requestId: selectedRfq.requestId })
      .then(list => setQuotes(Array.isArray(list) ? list : []))
      .catch(() => {});
  }, [reloadQuotesTrigger]); // eslint-disable-line react-hooks/exhaustive-deps

  // When a quote is selected, auto-fetch market data and price
  const selectQuoteAndPrice = useCallback(async (q: Quote) => {
    setSelectedQuote(q);
    setLegResults([]);
    setMdError(null);

    // Collect unique option underlyings from the quote's legs
    const underlyings = new Set<string>();
    q.legs.forEach(leg => {
      const parts = leg.instrumentName.split('-');
      if (parts.length >= 4) {
        const typeStr = parts[parts.length - 1].toUpperCase();
        if (typeStr === 'C' || typeStr === 'P') {
          const raw = parts.slice(0, parts.length - 3).join('-');
          const coin = raw.replace(/USD.*$/, '').replace(/USDT$/, '').toUpperCase();
          underlyings.add(coin);
        }
      }
    });

    // Fetch spot prices for all underlyings
    const newSpotPrices: Record<string, string> = {};
    const errors: string[] = [];

    await Promise.all(Array.from(underlyings).map(async (underlying) => {
      try {
        const md = await invoke<PricingMarketData>('fetch_pricing_market_data', {
          underlying,
          exchange: pricerExchange || 'deribit',
          instrumentOverride: null,
          testnet: null,
        });
        if (md.indexPrice != null) {
          newSpotPrices[underlying] = String(md.indexPrice);
        } else if (md.error) {
          errors.push(`${underlying} spot: ${md.error}`);
        }
      } catch (e) {
        errors.push(`${underlying} spot: ${String(e)}`);
      }
    }));

    setSpotPrices(prev => ({ ...prev, ...newSpotPrices }));
    if (errors.length > 0) setMdError(errors.join(' | '));

    // Price immediately with current spot (will re-price when spotPrices state settles)
    await priceLegsWithSpot(q.legs, { ...spotPrices, ...newSpotPrices });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pricerExchange, spotPrices, rfqDefaults.riskFreeRate, rfqDefaults.defaultVol]);

  const priceLegsWithSpot = async (qLegs: QuoteLeg[], currentSpot: Record<string, string>) => {
    setPricingLoading(true);
    try {
      const spotMap: Record<string, number> = {};
      Object.entries(currentSpot).forEach(([k, v]) => {
        const n = parseFloat(v);
        if (!isNaN(n) && n > 0) spotMap[k] = n;
      });

      const legInputs = qLegs.map(leg => ({
        instrumentName: leg.instrumentName,
        side: leg.side,
        quantity: parseFloat(leg.quantity) || 1,
        quotedPrice: parseFloat(leg.price) || 0,
        spotOverride: null,
      }));

      const results = await invoke<LegPriceResult[]>('price_rfq_legs', {
        legs: legInputs,
        spotPrices: spotMap,
        riskFreeRate: rfqDefaults.riskFreeRate,
        defaultVol: rfqDefaults.defaultVol,
      });
      setLegResults(results);
    } catch (e) {
      setMdError(String(e));
    } finally {
      setPricingLoading(false);
    }
  };

  const handleRefreshMarketData = async () => {
    if (!selectedQuote) return;
    await selectQuoteAndPrice(selectedQuote);
  };

  const handleRePrice = async () => {
    if (!selectedQuote) return;
    await priceLegsWithSpot(selectedQuote.legs, spotPrices);
  };

  // ── Maker: price an incoming seek's legs directly ─────────────────────────

  /** Convert a CoInCall instrument name to the target pricer exchange format. */
  const toExchangeInstrument = useCallback((ccName: string, exchange: string): string => {
    const parts = ccName.split('-');
    if (parts.length < 4) return ccName;
    const base    = parts[0].replace(/USD.*$/i, '').replace(/USDT$/i, '').toUpperCase();
    const dateStr = parts[parts.length - 3];
    const strike  = parts[parts.length - 2];
    const optType = parts[parts.length - 1];
    if (exchange === 'deribit') return `${base}-${dateStr}-${strike}-${optType}`;
    if (exchange === 'bybit')   return `${base}-${dateStr}-${strike}-${optType}-USDT`;
    if (exchange === 'okx') {
      const months: Record<string, string> = {
        JAN:'01',FEB:'02',MAR:'03',APR:'04',MAY:'05',JUN:'06',
        JUL:'07',AUG:'08',SEP:'09',OCT:'10',NOV:'11',DEC:'12',
      };
      const day = dateStr.slice(0, 2);
      const mon = months[dateStr.slice(2, 5).toUpperCase()] ?? '01';
      const yr  = dateStr.slice(5, 7);
      return `${base}-USD-${yr}${mon}${day}-${strike}-${optType}`;
    }
    return ccName; // coincall: use as-is
  }, []);

  /** Price a seek's legs and auto-submit a quote, then cancel it after the configured timeout. */
  const autoQuoteRfq = useCallback(async (rfq: ActiveRfq) => {
    if (!selectedId) return;
    console.log('[AutoQuote] pricing seek', rfq.requestId);
    try {
      const underlyings = new Set<string>();
      rfq.legs.forEach(leg => {
        const parts = leg.instrumentName.split('-');
        if (parts.length >= 4) {
          const raw = parts.slice(0, parts.length - 3).join('-');
          underlyings.add(raw.replace(/USD.*$/, '').replace(/USDT$/, '').toUpperCase());
        }
      });

      const spotMap: Record<string, number> = {};
      const ivMap: Record<string, number> = {};
      const exchangeData: Record<string, PricingMarketData> = {};

      await Promise.all([
        ...Array.from(underlyings).map(async (u) => {
          try {
            const md = await invoke<PricingMarketData>('fetch_pricing_market_data', {
              underlying: u, exchange: pricerExchange || 'deribit', instrumentOverride: null, testnet: null,
            });
            if (md.indexPrice) spotMap[u] = md.indexPrice;
          } catch {}
        }),
        ...rfq.legs.map(async (leg) => {
          const exInstrument = toExchangeInstrument(leg.instrumentName, pricerExchange || 'deribit');
          try {
            const md = await invoke<PricingMarketData>('fetch_option_market_data', {
              instrumentName: exInstrument, exchange: pricerExchange || 'deribit', testnet: null,
            });
            exchangeData[leg.instrumentName] = md;
            const parts = leg.instrumentName.split('-');
            const coin = parts[0].replace(/USD.*$/i, '').replace(/USDT$/i, '').toUpperCase();
            const price = md?.underlyingPrice ?? md?.indexPrice;
            if (price && price > 0) spotMap[coin] = price;
            if (md.markIv != null && md.markIv > 0) {
              ivMap[leg.instrumentName] = md.markIv > 2 ? md.markIv / 100 : md.markIv;
            }
          } catch {}
        }),
      ]);

      const legInputs = rfq.legs.map(leg => ({
        instrumentName: leg.instrumentName,
        side: leg.side,
        quantity: parseFloat(leg.qty) || 1,
        quotedPrice: 0,
        spotOverride: null,
        volOverride: ivMap[leg.instrumentName] ?? null,
      }));

      const results = await invoke<LegPriceResult[]>('price_rfq_legs', {
        legs: legInputs,
        spotPrices: spotMap,
        riskFreeRate: rfqDefaults.riskFreeRate,
        defaultVol: rfqDefaults.defaultVol,
      });

      const prices: Record<string, string> = {};
      results.forEach(r => {
        const seekLeg = rfq.legs.find(l => l.instrumentName === r.instrumentName);
        const exData = exchangeData[r.instrumentName];
        const fairUsd = exData?.fairValueUsd ?? r.fairValue;
        if (fairUsd == null || fairUsd <= 0) return;

        const qty = parseFloat(seekLeg?.qty ?? '1') || 1;
        const takerBuys = seekLeg?.side === 'BUY';
        const makerSign = takerBuys ? -1 : 1;
        const legGamma = (exData?.gamma ?? r.greeks?.gamma ?? 0) * qty * makerSign;
        const legVega  = (exData?.vega  ?? r.greeks?.vega  ?? 0) * qty * makerSign;

        let skew = 0;
        if (portfolioGreeks) {
          const raw = portfolioGreeks.gamma * legGamma * rfqDefaults.gammaSensitivity
                    + portfolioGreeks.vega  * legVega  * rfqDefaults.vegaSensitivity;
          skew = Math.max(-rfqDefaults.maxSkew, Math.min(rfqDefaults.maxSkew, raw));
        }
        const midSkewed = fairUsd * (1 + skew);
        const halfSpread = rfqDefaults.baseSpread / 2;
        const price = takerBuys ? midSkewed * (1 + halfSpread) : midSkewed * (1 - halfSpread);
        prices[r.instrumentName] = Math.max(0, price).toFixed(2);
      });

      const legPayload = rfq.legs.map(l => ({
        instrumentName: l.instrumentName,
        side: l.side === 'BUY' ? 'SELL' : 'BUY',
        price: prices[l.instrumentName] ?? '0',
        quantity: l.qty,
      }));

      const quoteData = await invoke<{ quoteId?: string; id?: string }>('coincall_create_quote', {
        accountId: selectedId,
        requestId: rfq.requestId,
        quoteSide: null,
        legs: legPayload,
      });

      const quoteId = quoteData?.quoteId ?? quoteData?.id ?? String(quoteData);
      console.log('[AutoQuote] submitted quoteId', quoteId, 'for', rfq.requestId);

      const timeoutMs = (rfqDefaults.autoQuoteTimeoutSecs ?? 30) * 1000;
      const timer = setTimeout(async () => {
        autoQuoteTimersRef.current.delete(rfq.requestId);
        if (!quoteId) return;
        try {
          await invoke('coincall_cancel_quote', { accountId: selectedId, quoteId: String(quoteId) });
          console.log('[AutoQuote] cancelled quote', quoteId);
        } catch (e) {
          console.warn('[AutoQuote] cancel failed:', e);
        }
      }, timeoutMs);
      autoQuoteTimersRef.current.set(rfq.requestId, timer);

    } catch (e) {
      console.error('[AutoQuote] error for', rfq.requestId, e);
    }
  }, [selectedId, pricerExchange, rfqDefaults, portfolioGreeks, toExchangeInstrument]); // eslint-disable-line react-hooks/exhaustive-deps

  // Keep ref current so loadRfqs (which is memoized separately) can call latest version
  useEffect(() => { autoQuoteRfqRef.current = autoQuoteRfq; }, [autoQuoteRfq]);

  const priceSeekLegs = useCallback(async (rfq: ActiveRfq) => {
    setSelectedRfq(rfq);
    setLegResults([]);
    setLegOrderbooks({});
    setMdError(null);
    setQuotePrices({});
    setQuoteError('');
    setQuoteSuccess('');

    const underlyings = new Set<string>();
    rfq.legs.forEach(leg => {
      const parts = leg.instrumentName.split('-');
      if (parts.length >= 4) {
        const typeStr = parts[parts.length - 1].toUpperCase();
        if (typeStr === 'C' || typeStr === 'P') {
          const raw = parts.slice(0, parts.length - 3).join('-');
          const coin = raw.replace(/USD.*$/, '').replace(/USDT$/, '').toUpperCase();
          underlyings.add(coin);
        }
      }
    });

    const newSpotPrices: Record<string, string> = {};
    const errors: string[] = [];

    // Fetch spot + per-leg ticker + per-leg orderbook in parallel
    const [, legIvs, legBooks] = await Promise.all([
      // 1. Spot/index prices per underlying
      Promise.all(Array.from(underlyings).map(async (underlying) => {
        try {
          const md = await invoke<PricingMarketData>('fetch_pricing_market_data', {
            underlying, exchange: pricerExchange || 'deribit', instrumentOverride: null, testnet: null,
          });
          if (md.indexPrice != null) newSpotPrices[underlying] = String(md.indexPrice);
          else if (md.error) errors.push(`${underlying} spot: ${md.error}`);
        } catch (e) { errors.push(`${underlying} spot: ${String(e)}`); }
      })),
      // 2. Per-leg market data from the exchange option ticker
      Promise.all(rfq.legs.map(async (leg) => {
        const exchangeInstrument = toExchangeInstrument(leg.instrumentName, pricerExchange || 'deribit');
        try {
          const md = await invoke<PricingMarketData>('fetch_option_market_data', {
            instrumentName: exchangeInstrument,
            exchange: pricerExchange || 'deribit',
            testnet: null,
          });
          return { instrumentName: leg.instrumentName, md };
        } catch {
          return { instrumentName: leg.instrumentName, md: null as PricingMarketData | null };
        }
      })),
      // 3. Per-leg orderbook (top 10 levels) for VWAP calculation at RFQ quantity
      Promise.all(rfq.legs.map(async (leg) => {
        const exchangeInstrument = toExchangeInstrument(leg.instrumentName, pricerExchange || 'deribit');
        try {
          const ob = await invoke<OrderbookSnapshot>('fetch_option_orderbook', {
            instrumentName: exchangeInstrument,
            exchange: pricerExchange || 'deribit',
            testnet: null,
          });
          return { instrumentName: leg.instrumentName, ob };
        } catch {
          return { instrumentName: leg.instrumentName, ob: null as OrderbookSnapshot | null };
        }
      })),
    ]);

    // Store exchange data keyed by instrumentName
    const newExchangeData: Record<string, PricingMarketData> = {};
    // Convert markIv to decimal vol for BS:
    // Deribit & OKX: markIv/markVol is in % (e.g. 36.5 → 0.365)
    // Bybit: markIv is decimal (e.g. 0.365) — values > 2 are assumed to be %
    // CoInCall: iv is in % as well
    const ivMap: Record<string, number> = {};
    legIvs.forEach(({ instrumentName, md }) => {
      if (md) {
        newExchangeData[instrumentName] = md;
        if (md.markIv != null && md.markIv > 0) {
          // Heuristic: if markIv > 2 it's in percent form, otherwise decimal
          ivMap[instrumentName] = md.markIv > 2 ? md.markIv / 100 : md.markIv;
        }
      }
    });
    setLegExchangeData(newExchangeData);

    // Store orderbooks
    const newOrderbooks: Record<string, OrderbookSnapshot> = {};
    legBooks.forEach(({ instrumentName, ob }) => {
      if (ob) newOrderbooks[instrumentName] = ob;
    });
    setLegOrderbooks(newOrderbooks);
    setSpotPrices(prev => ({ ...prev, ...newSpotPrices }));
    if (errors.length > 0) setMdError(errors.join(' | '));

    setPricingLoading(true);
    try {
      // Build spot map for BS:
      // Prefer underlyingPrice (forward) > indexPrice (spot) > previously fetched spot
      // underlyingPrice is the correct input for Black-Scholes on most exchanges
      const spotMap: Record<string, number> = {};
      Object.entries({ ...spotPrices, ...newSpotPrices }).forEach(([k, v]) => {
        const n = parseFloat(v);
        if (!isNaN(n) && n > 0) spotMap[k] = n;
      });
      rfq.legs.forEach(leg => {
        const exData = newExchangeData[leg.instrumentName];
        const parts = leg.instrumentName.split('-');
        const coin = parts[0].replace(/USD.*$/i, '').replace(/USDT$/i, '').toUpperCase();
        // Prefer underlyingPrice (forward), then indexPrice (spot)
        const price = exData?.underlyingPrice ?? exData?.indexPrice;
        if (price != null && price > 0) spotMap[coin] = price;
      });
      const legInputs = rfq.legs.map(leg => ({
        instrumentName: leg.instrumentName,
        side: leg.side,
        quantity: parseFloat(leg.qty) || 1,
        quotedPrice: 0,
        spotOverride: null,
        // Use live exchange IV if available, otherwise fall back to defaultVol
        volOverride: ivMap[leg.instrumentName] ?? null,
      }));
      const results = await invoke<LegPriceResult[]>('price_rfq_legs', {
        legs: legInputs,
        spotPrices: spotMap,
        riskFreeRate: rfqDefaults.riskFreeRate,
        defaultVol: rfqDefaults.defaultVol,
      });
      setLegResults(results);
      // Pre-fill quote prices — apply Greek skew based on portfolio state
      // greek_skew = clamp(portfolio.gamma × rfq_gamma_impact × gammaSensitivity
      //                  + portfolio.vega  × rfq_vega_impact  × vegaSensitivity,  ±maxSkew)
      // quote_price = fair × (1 + skew)
      // bid = quote_price × (1 - baseSpread/2), ask = quote_price × (1 + baseSpread/2)
      const initial: Record<string, string> = {};
      const newLegSkew: Record<string, number> = {};

      results.forEach(r => {
        const seekLeg = rfq.legs.find(l => l.instrumentName === r.instrumentName);
        const exData = newExchangeData[r.instrumentName];
        const fairUsd = exData?.fairValueUsd ?? r.fairValue;
        if (fairUsd == null || fairUsd <= 0) return;

        // Compute maker's Greek exposure for this leg:
        //   taker BUY  → maker SELL → maker LOSES gamma/vega
        //   taker SELL → maker BUY  → maker GAINS gamma/vega
        const qty = parseFloat(seekLeg?.qty ?? '1') || 1;
        const takerBuys = seekLeg?.side === 'BUY';
        const makerSign = takerBuys ? -1 : 1;
        const legGamma = (exData?.gamma ?? r.greeks?.gamma ?? 0) * qty * makerSign;
        const legVega  = (exData?.vega  ?? r.greeks?.vega  ?? 0) * qty * makerSign;

        // Skew: positive = price up (charging more), negative = price down (quoting aggressively)
        let skew = 0;
        if (portfolioGreeks) {
          const gammaScore = portfolioGreeks.gamma * legGamma * rfqDefaults.gammaSensitivity;
          const vegaScore  = portfolioGreeks.vega  * legVega  * rfqDefaults.vegaSensitivity;
          const raw = gammaScore + vegaScore;
          const maxSkew = rfqDefaults.maxSkew;
          skew = Math.max(-maxSkew, Math.min(maxSkew, raw));
        }
        newLegSkew[r.instrumentName] = skew;

        const midSkewed = fairUsd * (1 + skew);
        // Maker is always quoting the opposite side from taker:
        // Taker wants to BUY → maker SELLS → use ask price (higher)
        // Taker wants to SELL → maker BUYS → use bid price (lower)
        const halfSpread = rfqDefaults.baseSpread / 2;
        const price = takerBuys
          ? midSkewed * (1 + halfSpread)   // maker's ask
          : midSkewed * (1 - halfSpread);  // maker's bid
        initial[r.instrumentName] = Math.max(0, price).toFixed(2);
      });
      setQuotePrices(initial);
      setLegSkews(newLegSkew);
    } catch (e) {
      setMdError(String(e));
    } finally {
      setPricingLoading(false);
    }
  }, [pricerExchange, spotPrices, rfqDefaults]); // eslint-disable-line react-hooks/exhaustive-deps

  const submitQuote = async () => {
    if (!selectedId || !selectedRfq) return;
    setQuoteSubmitting(true);
    setQuoteError('');
    setQuoteSuccess('');
    try {
      const legPayload = selectedRfq.legs.map(l => {
        const instrumentName = l.instrumentName;
        // Maker takes the OPPOSITE side from the taker
        const makerSide = l.side === 'BUY' ? 'SELL' : 'BUY';
        return {
          instrumentName,
          side: makerSide,
          price: quotePrices[instrumentName] ?? '0',
          quantity: l.qty,
        };
      });
      await invoke('coincall_create_quote', {
        accountId: selectedId,
        requestId: selectedRfq.requestId,
        quoteSide: null,
        legs: legPayload,
      });
      setQuoteSuccess('Quote submitted!');
    } catch (e: any) {
      setQuoteError(String(e));
    } finally {
      setQuoteSubmitting(false);
    }
  };

  const cancelRfq = async (requestId: string) => {
    if (!selectedId) return;
    try {
      await invoke('coincall_cancel_rfq', { accountId: selectedId, requestId });
      setRfqs(prev => prev.map(r => r.requestId === requestId ? { ...r, state: 'CANCELLED' } : r));
      if (selectedRfq?.requestId === requestId) setSelectedRfq(r => r ? { ...r, state: 'CANCELLED' } : null);
    } catch (e: any) { alert('Cancel failed: ' + String(e)); }
  };

  const acceptQuote = async () => {
    if (!selectedId || !selectedRfq || !selectedQuote) return;
    setAcceptLoading(true);
    try {
      const ok = await invoke<boolean>('coincall_accept_quote', {
        accountId: selectedId,
        requestId: selectedRfq.requestId,
        quoteId: selectedQuote.quoteId,
      });
      if (ok) {
        setQuotes(prev => prev.map(q => q.quoteId === selectedQuote.quoteId ? { ...q, state: 'FILLED' } : q));
        setSelectedQuote(q => q ? { ...q, state: 'FILLED' } : null);
      } else {
        alert('Accept quote returned false — check logs');
      }
    } catch (e: any) { alert('Accept failed: ' + String(e)); }
    finally { setAcceptLoading(false); }
  };

  const createRfq = async () => {
    if (!selectedId) return;
    setSubmitError('');
    const validLegs = legs.filter(l => l.instrumentName.trim() && l.qty.trim());
    if (validLegs.length < 1) { setSubmitError('Add at least one leg'); return; }
    setSubmitLoading(true);
    try {
      const rfqLegs = validLegs.map(l => ({
        instrumentName: l.instrumentName.trim(), side: l.side, qty: l.qty.trim(),
      }));
      const created = await invoke<ActiveRfq>('coincall_create_rfq', { accountId: selectedId, legs: rfqLegs });
      setRfqs(prev => [created, ...prev]);
      setLegs([{ instrumentName: '', side: 'BUY', qty: '' }, { instrumentName: '', side: 'SELL', qty: '' }]);
    } catch (e: any) { setSubmitError(String(e)); }
    finally { setSubmitLoading(false); }
  };

  const addLeg    = () => setLegs(prev => [...prev, { instrumentName: '', side: 'BUY', qty: '' }]);
  const removeLeg = (i: number) => setLegs(prev => prev.filter((_, idx) => idx !== i));
  const updateLeg = (i: number, field: keyof RfqLegFull, value: string) =>
    setLegs(prev => prev.map((l, idx) => idx === i ? { ...l, [field]: value } : l));

  useEffect(() => { loadRfqs(); }, [loadRfqs]);

  // Reset selection when mode changes
  useEffect(() => {
    setRfqs([]);
    setSelectedRfq(null);
    setSelectedQuote(null);
    setQuotes([]);
    setLegResults([]);
    setLegExchangeData({});
    setLegOrderbooks({});
    setQuotePrices({});
    setQuoteError('');
    setQuoteSuccess('');
    setMdError(null);
    setRfqError(null);
    // Clear auto-quote state when switching modes
    autoQuotedRef.current.clear();
    autoQuoteTimersRef.current.forEach(t => clearTimeout(t));
    autoQuoteTimersRef.current.clear();
  }, [mode]);

  // Reset auto-quote toggle when account or trading coin changes
  useEffect(() => {
    setAutoQuoteEnabled(false);
    autoQuotedRef.current.clear();
    autoQuoteTimersRef.current.forEach(t => clearTimeout(t));
    autoQuoteTimersRef.current.clear();
  }, [selectedId, tradingCoin]);

  const fmtTime = (ms: number) => {
    if (!ms) return '—';
    return new Date(ms).toLocaleTimeString('en-US', { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' });
  };
  const timeLeft = (expiryMs: number) => {
    const diff = expiryMs - Date.now();
    if (diff <= 0) return 'Expired';
    return `${Math.floor(diff / 60000)}m ${Math.floor((diff % 60000) / 1000)}s`;
  };
  const fmt = (v: number | null | undefined, dp = 4) => v == null ? '—' : v.toFixed(dp);

  // Net strategy summary from leg results + exchange greeks + quote prices
  // Greeks are from maker's perspective: taker BUY → maker SELL → negate greeks
  const net = legResults.reduce(
    (acc, r) => {
      const exData = legExchangeData[r.instrumentName];
      const seekLeg = selectedRfq?.legs.find(l => l.instrumentName === r.instrumentName);
      // Use exchange greeks if available, else BS
      const delta = exData?.delta ?? r.greeks?.delta ?? null;
      const gamma = exData?.gamma ?? r.greeks?.gamma ?? null;
      const vega  = exData?.vega  ?? r.greeks?.vega  ?? null;
      const theta = exData?.theta ?? r.greeks?.theta ?? null;
      // Maker takes opposite side: taker BUY → maker SELL → sign = -1
      const qty = parseFloat(seekLeg?.qty ?? '1') || 1;
      const makerSign = seekLeg?.side === 'BUY' ? -1 : 1;
      if (delta != null) acc.delta += makerSign * delta * qty;
      if (gamma != null) acc.gamma += makerSign * gamma * qty;
      if (vega  != null) acc.vega  += makerSign * vega  * qty;
      if (theta != null) acc.theta += makerSign * theta * qty;
      // Net price: positive = premium received (maker sold), negative = premium paid (maker bought)
      const px = parseFloat(quotePrices[r.instrumentName] ?? '0') || 0;
      acc.netPrice += makerSign * px * qty;
      if (r.diff != null) acc.totalDiff += r.diff;
      acc.hasGreeks = acc.hasGreeks || delta != null;
      return acc;
    },
    { delta: 0, gamma: 0, vega: 0, theta: 0, netPrice: 0, totalDiff: 0, hasGreeks: false }
  );

  const underlyings = Array.from(new Set(
    legResults.filter(r => r.spotUsed != null).map(r => {
      const parts = r.instrumentName.split('-');
      return parts.slice(0, parts.length - 3).join('-').replace(/USD.*$/, '').replace(/USDT$/, '').toUpperCase();
    })
  ));

  if (ccAccounts.length === 0) {
    return (
      <Wrapper>
        <Empty style={{ margin: 'auto' }}>
          No CoInCall accounts configured.<br />
          Go to ⚙ Settings → Exchange to add a CoInCall account.
        </Empty>
      </Wrapper>
    );
  }

  return (
    <Wrapper>
      <TopBar>
        <Label>
          Account
          <Select value={selectedId} onChange={e => setSelectedId(e.target.value)}>
            {ccAccounts.map(a => (
              <option key={a.id} value={a.id}>{a.name}{a.testnet ? ' [testnet]' : ''}</option>
            ))}
          </Select>
        </Label>
        <Label>
          Coin
          <Select value={tradingCoin} onChange={e => setTradingCoin(e.target.value.toUpperCase())} style={{ width: 68 }}>
            {['BTC', 'ETH', 'SOL', 'XRP', 'BNB'].map(c => <option key={c} value={c}>{c}</option>)}
          </Select>
        </Label>
        {/* Mode toggle */}
        <div style={{ display: 'flex', gap: 0, border: '1px solid #2a3a50', borderRadius: 4, overflow: 'hidden' }}>
          {(['maker', 'taker', 'history'] as const).map(m => (
            <Btn key={m}
              style={{ borderRadius: 0, background: mode === m ? '#1e7ed4' : 'transparent', color: mode === m ? '#fff' : '#7e8b99', padding: '0.25rem 0.75rem', fontSize: '0.78rem' }}
              onClick={() => setMode(m)}
            >
              {m === 'maker' ? '🏦 Maker' : m === 'taker' ? '📤 Taker' : '📋 History'}
            </Btn>
          ))}
        </div>
        <Btn onClick={loadRfqs} disabled={loadingRfqs}>{loadingRfqs ? '⟳' : '↺'} Refresh</Btn>
        <div style={{ borderLeft: '1px solid #1e2738', paddingLeft: '0.75rem', display: 'flex', alignItems: 'center', gap: '0.5rem' }}>
          <Label>
            Exchange
            <Select value={pricerExchange} onChange={e => setPricerExchange(e.target.value)} style={{ width: 90 }}>
              {EXCHANGES_PRICER.map(ex => <option key={ex} value={ex}>{ex}</option>)}
            </Select>
          </Label>
        </div>
        {account?.testnet && (
          <span style={{ fontSize: '0.72rem', color: '#e0b94a', marginLeft: 'auto' }}>⚠ Testnet</span>
        )}
        {mode === 'maker' && (
          <button
            onClick={() => setAutoQuoteEnabled(v => !v)}
            title={autoQuoteEnabled ? `Auto-Quote ON — cancels after ${rfqDefaults.autoQuoteTimeoutSecs}s. Click to disable.` : 'Enable auto-quote for this account & coin'}
            style={{
              marginLeft: account?.testnet ? '0' : 'auto',
              display: 'flex', alignItems: 'center', gap: '0.3rem',
              fontSize: '0.75rem', fontWeight: 600,
              padding: '0.25rem 0.6rem', borderRadius: 4, cursor: 'pointer',
              border: autoQuoteEnabled ? '1px solid rgba(74,222,128,0.5)' : '1px solid #2a3a50',
              background: autoQuoteEnabled ? 'rgba(74,222,128,0.12)' : 'transparent',
              color: autoQuoteEnabled ? '#4ade80' : '#7e8b99',
              transition: 'all 0.15s',
            }}
          >
            ⚡ Auto-Quote {autoQuoteEnabled ? `ON · ${rfqDefaults.autoQuoteTimeoutSecs}s` : 'OFF'}
          </button>
        )}
      </TopBar>

      {mode === 'maker' ? (
        /* ── MAKER MODE ──────────────────────────────────────────────────── */
        <Body>
          {/* Incoming seeks from takers */}
          <Col $width="300px">
            {/* Portfolio Greeks panel */}
            <div style={{ padding: '0.5rem 0.75rem', background: '#0d1520', borderBottom: '1px solid #1e2738' }}>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: '0.3rem' }}>
                <span style={{ fontSize: '0.72rem', color: '#7e8b99', fontWeight: 600, letterSpacing: '0.05em' }}>
                  PORTFOLIO · {tradingCoin}
                </span>
                <button
                  onClick={loadPortfolioGreeks}
                  disabled={portfolioLoading}
                  style={{ fontSize: '0.68rem', background: 'none', border: 'none', color: '#4a7aaa', cursor: 'pointer', padding: '0 0.2rem' }}
                >
                  {portfolioLoading ? '⟳' : '↺'}
                </button>
              </div>
              {portfolioGreeks ? (
                <>
                  {/* Total delta row */}
                  <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr 1fr', gap: '0.25rem', marginBottom: '0.3rem' }}>
                    {([
                      { label: 'Total Δ', value: portfolioGreeks.totalDelta,    decimals: 3 },
                      { label: 'Γ',       value: portfolioGreeks.gamma,          decimals: 4 },
                      { label: 'ν Vega',  value: portfolioGreeks.vega,           decimals: 1 },
                      { label: 'θ',       value: portfolioGreeks.theta,          decimals: 1 },
                    ] as const).map(({ label, value, decimals }) => (
                      <div key={label} style={{ textAlign: 'center' }}>
                        <div style={{ fontSize: '0.63rem', color: '#4a5568' }}>{label}</div>
                        <div style={{ fontSize: '0.75rem', fontFamily: 'monospace',
                          color: value > 0 ? '#4ade80' : value < 0 ? '#f87171' : '#7e8b99' }}>
                          {value >= 0 ? '+' : ''}{value.toFixed(decimals)}
                        </div>
                      </div>
                    ))}
                  </div>
                  {/* Balance breakdown */}
                  <div style={{ fontSize: '0.67rem', color: '#4a5568', borderTop: '1px solid #1a2535', paddingTop: '0.25rem', display: 'flex', gap: '0.5rem', flexWrap: 'wrap' }}>
                    <span>
                      <span style={{ color: '#7e8b99' }}>Pos Δ </span>
                      <span style={{ color: '#c8d6e5', fontFamily: 'monospace' }}>
                        {portfolioGreeks.positionDelta >= 0 ? '+' : ''}{portfolioGreeks.positionDelta.toFixed(3)}
                      </span>
                    </span>
                    <span style={{ color: '#4a5568' }}>+</span>
                    <span title={`${portfolioGreeks.coinBalance.toFixed(4)} ${tradingCoin} + ${portfolioGreeks.usdtBalance.toFixed(0)} USDT @ ${portfolioGreeks.spotPrice.toFixed(0)}`}>
                      <span style={{ color: '#7e8b99' }}>Bal Δ </span>
                      <span style={{ color: '#e0b94a', fontFamily: 'monospace' }}>
                        +{portfolioGreeks.balanceDelta.toFixed(3)}
                      </span>
                      <span style={{ color: '#4a5568' }}> ({portfolioGreeks.coinBalance.toFixed(3)} + {(portfolioGreeks.usdtBalance / (portfolioGreeks.spotPrice || 1)).toFixed(3)})</span>
                    </span>
                  </div>
                </>
              ) : (
                <div style={{ fontSize: '0.7rem', color: '#4a5568', textAlign: 'center' }}>
                  {portfolioLoading ? 'Loading…' : 'No data'}
                </div>
              )}
            </div>
            <ColHeader>
              Incoming RFQs
              {(() => {
                const filtered = rfqs.filter(r => r.legs.some(l => l.instrumentName.toUpperCase().includes(tradingCoin)));
                return (
                  <span style={{ fontSize: '0.72rem', color: '#4a5568' }}>{filtered.length} for {tradingCoin}</span>
                );
              })()}
            </ColHeader>
            <Scroll>
              {loadingRfqs && <Empty>Loading…</Empty>}
              {!loadingRfqs && rfqError && (
                <Empty style={{ color: '#e07070', fontSize: '0.72rem', padding: '0.5rem' }}>
                  ⚠ {rfqError}
                </Empty>
              )}
              {!loadingRfqs && !rfqError && rfqs.length === 0 && <Empty>No incoming RFQs</Empty>}
              {rfqs
                .filter(rfq => rfq.legs.some(l => l.instrumentName.toUpperCase().includes(tradingCoin)))
                .map(rfq => (
                <Card key={rfq.requestId} $selected={selectedRfq?.requestId === rfq.requestId}
                  onClick={() => priceSeekLegs(rfq)}>
                  <CardTitle>
                    <span style={{ fontFamily: 'monospace', fontSize: '0.72rem', color: '#7eb8f7' }}>
                      #{rfq.requestId.slice(-8)}
                    </span>
                    <StatusBadge $state={rfq.state}>{rfq.state}</StatusBadge>
                  </CardTitle>
                  <CardMeta>
                    {fmtTime(rfq.createTime)} · {rfq.state === 'ACTIVE' ? timeLeft(rfq.expiryTime) : fmtTime(rfq.expiryTime)}
                  </CardMeta>
                  {rfq.legs.map((leg, i) => (
                    <LegRow key={i} style={{ marginTop: '0.2rem' }}>
                      <SideBadge $side={leg.side}>{leg.side}</SideBadge>
                      <span style={{ fontSize: '0.72rem', color: '#c8d6e5' }}>{leg.instrumentName}</span>
                      <span style={{ fontSize: '0.7rem', color: '#7e8b99', marginLeft: 'auto' }}>×{leg.qty}</span>
                    </LegRow>
                  ))}
                </Card>
              ))}
            </Scroll>
          </Col>

          {/* BS Pricer + Quote Builder */}
          <ColRight>
            <ColHeader>
              BS Pricer & Quote
              {selectedRfq && (
                <div style={{ display: 'flex', gap: '0.4rem', alignItems: 'center' }}>
                  <span style={{ fontSize: '0.72rem', color: '#4a5568' }}>
                    RFQ #{selectedRfq.requestId.slice(-8)}
                  </span>
                  <Btn onClick={() => priceSeekLegs(selectedRfq)} disabled={pricingLoading}>
                    {pricingLoading ? '⟳' : '↺'} Re-price
                  </Btn>
                </div>
              )}
            </ColHeader>

            {!selectedRfq ? (
              <Empty style={{ margin: 'auto' }}>Select an incoming RFQ to price and quote</Empty>
            ) : (
              <Scroll>
                {mdError && <Err style={{ marginBottom: '0.5rem' }}>⚠ {mdError}</Err>}

                {/* Spot prices */}
                {underlyings.length > 0 && (
                  <>
                    <SectionTitle>Spot / Index Prices</SectionTitle>
                    {underlyings.map(u => (
                      <SpotRow key={u}>
                        <SpotLabel>{u}</SpotLabel>
                        <SmallInput
                          type="number" placeholder="spot"
                          value={spotPrices[u] ?? ''}
                          onChange={e => setSpotPrices(p => ({ ...p, [u]: e.target.value }))}
                        />
                        <MdSourceRow>via {pricerExchange}</MdSourceRow>
                      </SpotRow>
                    ))}
                    <Row>
                      <Btn onClick={() => selectedRfq && priceSeekLegs(selectedRfq)} disabled={pricingLoading} style={{ fontSize: '0.78rem' }}>
                        {pricingLoading ? 'Pricing…' : '⚡ Re-price'}
                      </Btn>
                    </Row>
                  </>
                )}

                {pricingLoading && legResults.length === 0 && <Empty>Fetching market data & pricing…</Empty>}

                {/* Leg table with editable quote prices */}
                {legResults.length > 0 && (
                  <>
                    <SectionTitle style={{ marginTop: '0.5rem' }}>
                      Quote Prices — pre-filled from {pricerExchange} mark price · adjust before submitting
                    </SectionTitle>
                    <div style={{ overflowX: 'auto' }}>
                      <PricerTable>
                        <thead>
                          <tr>
                            <PTh>Instrument</PTh>
                            <PTh>Taker Side</PTh>
                            <PTh style={{ textAlign: 'right' }}>Qty</PTh>
                            <PTh style={{ textAlign: 'right' }}>IV %</PTh>
                            <PTh style={{ textAlign: 'right' }}>Δ</PTh>
                            <PTh style={{ textAlign: 'right' }}>Vega</PTh>
                            <PTh style={{ textAlign: 'right', color: '#4ade80' }}>Bid (VWAP@Qty)</PTh>
                            <PTh style={{ textAlign: 'right', color: '#f87171' }}>Ask (VWAP@Qty)</PTh>
                            <PTh style={{ textAlign: 'right' }}>Skew</PTh>
                            <PTh style={{ textAlign: 'right' }}>Your Price (USD)</PTh>
                          </tr>
                        </thead>
                        <tbody>
                          {legResults.map((r, i) => {
                            const seekLeg = selectedRfq.legs[i];
                            const exData = legExchangeData[r.instrumentName];
                            const ob = legOrderbooks[r.instrumentName];
                            // Prefer exchange greeks; fall back to our BS computation
                            const displayDelta = exData?.delta ?? r.greeks?.delta ?? null;
                            const displayVega  = exData?.vega  ?? r.greeks?.vega  ?? null;
                            const displayIv    = r.iv != null ? r.iv * 100 : null;
                            const currentPrice = quotePrices[r.instrumentName] ?? '';
                            const skew = legSkews[r.instrumentName];
                            const skewPct = skew != null ? skew * 100 : null;
                            // Orderbook VWAP at RFQ quantity.
                            // Deribit: prices in BTC fractions → multiply by forward price.
                            // OKX: already scaled ×100 in Rust (per-contract → per-underlying).
                            // Bybit/CoInCall: already in USD per underlying.
                            const rfqQty = parseFloat(seekLeg?.qty ?? '1') || 1;
                            const fwd = exData?.underlyingPrice ?? exData?.indexPrice ?? null;
                            const toUsd = (p: number) =>
                              pricerExchange === 'deribit' && fwd ? p * fwd : p;
                            const obBid = ob?.bids?.length
                              ? vwapThrough(ob.bids.map(l => ({ price: toUsd(l.price), size: l.size })), rfqQty)
                              : null;
                            const obAsk = ob?.asks?.length
                              ? vwapThrough(ob.asks.map(l => ({ price: toUsd(l.price), size: l.size })), rfqQty)
                              : null;
                            // Indicate if book is shallow (couldn't fill full qty)
                            const bidPartial = obBid && obBid.filled < rfqQty * 0.99;
                            const askPartial = obAsk && obAsk.filled < rfqQty * 0.99;
                            return (
                              <tr key={i}>
                                <PTd style={{ fontFamily: 'monospace', fontSize: '0.7rem', color: '#7eb8f7' }}>
                                  {r.instrumentName}
                                </PTd>
                                <PTd><SideBadge $side={seekLeg?.side ?? 'BUY'}>{seekLeg?.side ?? '?'}</SideBadge></PTd>
                                <PTd $align="right">{seekLeg?.qty ?? '?'}</PTd>
                                <PTd $align="right" style={{ color: '#e0b94a' }}>
                                  {displayIv != null ? displayIv.toFixed(1) + '%' : '—'}
                                </PTd>
                                <PTd $align="right" style={{ color: displayDelta != null ? '#c8d6e5' : '#4a5568' }}>
                                  {displayDelta != null ? fmt(displayDelta, 4) : '—'}
                                </PTd>
                                <PTd $align="right" style={{ color: '#7e8b99' }}>
                                  {displayVega != null ? fmt(displayVega, 4) : '—'}
                                </PTd>
                                <PTd $align="right" style={{ color: obBid ? '#4ade80' : '#4a5568', fontFamily: 'monospace', fontSize: '0.72rem' }}
                                  title={obBid ? `VWAP of ${obBid.filled.toFixed(2)} filled` : 'No orderbook'}>
                                  {obBid && obBid.vwap > 0
                                    ? `${obBid.vwap.toFixed(2)}${bidPartial ? '⚠' : ''}`
                                    : '—'}
                                </PTd>
                                <PTd $align="right" style={{ color: obAsk ? '#f87171' : '#4a5568', fontFamily: 'monospace', fontSize: '0.72rem' }}
                                  title={obAsk ? `VWAP of ${obAsk.filled.toFixed(2)} filled` : 'No orderbook'}>
                                  {obAsk && obAsk.vwap > 0
                                    ? `${obAsk.vwap.toFixed(2)}${askPartial ? '⚠' : ''}`
                                    : '—'}
                                </PTd>
                                <PTd $align="right" style={{ color: skewPct == null ? '#4a5568' : skewPct > 0.05 ? '#f87171' : skewPct < -0.05 ? '#4ade80' : '#e0b94a', fontSize: '0.72rem' }}>
                                  {skewPct != null
                                    ? (skewPct >= 0 ? '+' : '') + skewPct.toFixed(2) + '%'
                                    : portfolioGreeks ? '0.00%' : '—'}
                                </PTd>
                                <PTd $align="right">
                                  <SmallInput
                                    type="number" step="1" min="0"
                                    value={currentPrice}
                                    onChange={e => setQuotePrices(p => ({ ...p, [r.instrumentName]: e.target.value }))}
                                    style={{ width: 90 }}
                                  />
                                </PTd>
                              </tr>
                            );
                          })}
                        </tbody>
                      </PricerTable>
                    </div>

                    {/* Net greeks + net price for the strategy */}
                    {net.hasGreeks && (
                      <NetBox>
                        <NetStat>
                          <NetLabel>Net Δ</NetLabel>
                          <NetValue $color={net.delta > 0.01 ? '#4ade80' : net.delta < -0.01 ? '#f87171' : undefined}>
                            {net.delta >= 0 ? '+' : ''}{net.delta.toFixed(4)}
                          </NetValue>
                        </NetStat>
                        <NetStat>
                          <NetLabel>Net Γ</NetLabel>
                          <NetValue $color={net.gamma > 0 ? '#4ade80' : net.gamma < 0 ? '#f87171' : undefined}>
                            {net.gamma >= 0 ? '+' : ''}{net.gamma.toFixed(5)}
                          </NetValue>
                        </NetStat>
                        <NetStat>
                          <NetLabel>Net ν</NetLabel>
                          <NetValue $color={net.vega > 0 ? '#4ade80' : net.vega < 0 ? '#f87171' : undefined}>
                            {net.vega >= 0 ? '+' : ''}{net.vega.toFixed(2)}
                          </NetValue>
                        </NetStat>
                        <NetStat>
                          <NetLabel>Net θ</NetLabel>
                          <NetValue $color={net.theta > 0 ? '#4ade80' : net.theta < 0 ? '#f87171' : undefined}>
                            {net.theta >= 0 ? '+' : ''}{net.theta.toFixed(2)}
                          </NetValue>
                        </NetStat>
                        <NetStat>
                          <NetLabel>Net Price</NetLabel>
                          <NetValue $color={net.netPrice > 0 ? '#4ade80' : net.netPrice < 0 ? '#f87171' : undefined}
                            title={net.netPrice > 0 ? 'Premium received (net short)' : 'Premium paid (net long)'}>
                            {net.netPrice >= 0 ? '+' : ''}{net.netPrice.toFixed(2)}
                          </NetValue>
                        </NetStat>
                      </NetBox>
                    )}

                    {/* Submit quote */}
                    {quoteError && <Err style={{ marginTop: '0.5rem' }}>⚠ {quoteError}</Err>}
                    {quoteSuccess && (
                      <div style={{ color: '#4ade80', fontSize: '0.82rem', marginTop: '0.5rem' }}>✅ {quoteSuccess}</div>
                    )}
                    {selectedRfq.state === 'ACTIVE' && (
                      <Btn
                        $variant="success" onClick={submitQuote} disabled={quoteSubmitting}
                        style={{ marginTop: '0.75rem', width: '100%', padding: '0.5rem', fontSize: '0.82rem' }}
                      >
                        {quoteSubmitting ? 'Submitting…' : '📨 Submit Quote to Taker'}
                      </Btn>
                    )}
                  </>
                )}
              </Scroll>
            )}
          </ColRight>
        </Body>
      ) : mode === 'taker' ? (
        /* ── TAKER MODE ──────────────────────────────────────────────────── */
        <Body>
          {/* Create RFQ */}
          <Col $width="320px">
            <ColHeader>Create RFQ</ColHeader>
            <Scroll>
              <CreateForm>
                <SectionTitle>Legs</SectionTitle>
                {legs.map((leg, i) => (
                  <LegRow key={i}>
                    <Input
                      placeholder="e.g. BTCUSD-27JUN25-100000-C"
                      value={leg.instrumentName}
                      onChange={e => updateLeg(i, 'instrumentName', e.target.value)}
                      style={{ flex: 3, minWidth: 130 }}
                    />
                    <Select value={leg.side} onChange={e => updateLeg(i, 'side', e.target.value)} style={{ width: 68 }}>
                      <option value="BUY">BUY</option>
                      <option value="SELL">SELL</option>
                    </Select>
                    <Input
                      placeholder="Qty" value={leg.qty}
                      onChange={e => updateLeg(i, 'qty', e.target.value)}
                      style={{ flex: 1, width: 55, minWidth: 45 }}
                    />
                    <Btn $variant="danger" onClick={() => removeLeg(i)} disabled={legs.length <= 1}>✕</Btn>
                  </LegRow>
                ))}
                <Row style={{ marginTop: '0.4rem' }}><Btn onClick={addLeg}>+ Add Leg</Btn></Row>
                {submitError && <Err>{submitError}</Err>}
                <Btn
                  $variant="primary" onClick={createRfq}
                  disabled={submitLoading || !selectedId}
                  style={{ marginTop: '0.5rem', width: '100%', padding: '0.4rem' }}
                >
                  {submitLoading ? 'Creating…' : '📤 Submit RFQ'}
                </Btn>
              </CreateForm>
            </Scroll>
          </Col>

          {/* Your RFQs */}
          <Col $width="280px">
            <ColHeader>
              Your RFQs
              <span style={{ fontSize: '0.72rem', color: '#4a5568' }}>{rfqs.length} total</span>
            </ColHeader>
            <Scroll>
              {loadingRfqs && <Empty>Loading…</Empty>}
              {!loadingRfqs && rfqError && (
                <Empty style={{ color: '#e07070', fontSize: '0.72rem', padding: '0.5rem' }}>⚠ {rfqError}</Empty>
              )}
              {!loadingRfqs && !rfqError && rfqs.length === 0 && <Empty>No RFQs found</Empty>}
              {rfqs.map(rfq => (
                <Card key={rfq.requestId} $selected={selectedRfq?.requestId === rfq.requestId} onClick={() => loadQuotes(rfq)}>
                  <CardTitle>
                    <span style={{ fontFamily: 'monospace', fontSize: '0.72rem', color: '#7eb8f7' }}>
                      #{rfq.requestId.slice(-8)}
                    </span>
                    <div style={{ display: 'flex', gap: '0.3rem', alignItems: 'center' }}>
                      <StatusBadge $state={rfq.state}>{rfq.state}</StatusBadge>
                      {rfq.state === 'ACTIVE' && (
                        <Btn $variant="danger" onClick={e => { e.stopPropagation(); cancelRfq(rfq.requestId); }}>Cancel</Btn>
                      )}
                    </div>
                  </CardTitle>
                  <CardMeta>
                    {fmtTime(rfq.createTime)} · {rfq.state === 'ACTIVE' ? timeLeft(rfq.expiryTime) : fmtTime(rfq.expiryTime)}
                  </CardMeta>
                  {rfq.legs.map((leg, i) => (
                    <LegRow key={i} style={{ marginTop: '0.2rem' }}>
                      <SideBadge $side={leg.side}>{leg.side}</SideBadge>
                      <span style={{ fontSize: '0.72rem', color: '#c8d6e5' }}>{leg.instrumentName}</span>
                      <span style={{ fontSize: '0.7rem', color: '#7e8b99', marginLeft: 'auto' }}>×{leg.qty}</span>
                    </LegRow>
                  ))}
                </Card>
              ))}
            </Scroll>
          </Col>

          {/* Quotes received */}
          <Col $width="260px">
            <ColHeader>
              Quotes Received
              {selectedRfq && <span style={{ fontSize: '0.72rem', color: '#4a5568' }}>{quotes.length} received</span>}
            </ColHeader>
            <Scroll>
              {!selectedRfq && <Empty>Select an RFQ</Empty>}
              {selectedRfq && loadingQuotes && <Empty>Loading…</Empty>}
              {selectedRfq && !loadingQuotes && quotes.length === 0 && !mdError && <Empty>No quotes yet</Empty>}
              {selectedRfq && !loadingQuotes && mdError && quotes.length === 0 && (
                <Empty style={{ color: '#e07070', padding: '0.5rem', fontSize: '0.72rem' }}>{mdError}</Empty>
              )}
              {quotes.map(q => (
                <Card key={q.quoteId} $selected={selectedQuote?.quoteId === q.quoteId}
                  onClick={() => selectQuoteAndPrice(q)}>
                  <CardTitle>
                    <span style={{ fontFamily: 'monospace', fontSize: '0.72rem', color: '#7eb8f7' }}>
                      #{q.quoteId.slice(-8)}
                    </span>
                    <StatusBadge $state={q.state}>{q.state}</StatusBadge>
                  </CardTitle>
                  <CardMeta>Exp: {fmtTime(q.expiryTime)}</CardMeta>
                  {q.legs.map((leg, i) => (
                    <LegRow key={i} style={{ marginTop: '0.25rem' }}>
                      <SideBadge $side={leg.side}>{leg.side}</SideBadge>
                      <span style={{ fontSize: '0.72rem', color: '#c8d6e5', flex: 1 }}>{leg.instrumentName}</span>
                      <span style={{ fontSize: '0.78rem', color: '#e8edf4', fontWeight: 600 }}>{leg.price}</span>
                    </LegRow>
                  ))}
                  <CardMeta style={{ marginTop: '0.3rem', fontSize: '0.7rem', color: '#4a5568' }}>
                    Click to price →
                  </CardMeta>
                </Card>
              ))}
            </Scroll>
          </Col>

          {/* ── BS Pricer ───────────────────────────────────────────────── */}
          <ColRight>
          <ColHeader>
            BS Pricer
            {selectedQuote && (
              <div style={{ display: 'flex', gap: '0.4rem', alignItems: 'center' }}>
                <span style={{ fontSize: '0.72rem', color: '#4a5568' }}>
                  Quote #{selectedQuote.quoteId.slice(-8)}
                </span>
                <Btn onClick={handleRefreshMarketData} disabled={pricingLoading} title="Re-fetch market data & reprice">
                  {pricingLoading ? '⟳' : '↺'} Refresh
                </Btn>
              </div>
            )}
          </ColHeader>

          {!selectedQuote ? (
            <Empty style={{ margin: 'auto' }}>Select a quote to analyse with Black-Scholes</Empty>
          ) : (
            <Scroll>
              {/* Spot overrides */}
              {underlyings.length > 0 && (
                <>
                  <SectionTitle>Spot / Index Prices</SectionTitle>
                  {underlyings.map(u => (
                    <SpotRow key={u}>
                      <SpotLabel>{u}</SpotLabel>
                      <SmallInput
                        type="number" placeholder="spot"
                        value={spotPrices[u] ?? ''}
                        onChange={e => setSpotPrices(p => ({ ...p, [u]: e.target.value }))}
                      />
                      <MdSourceRow>via {pricerExchange}</MdSourceRow>
                    </SpotRow>
                  ))}
                  <Row>
                    <Btn onClick={handleRePrice} disabled={pricingLoading} style={{ fontSize: '0.78rem' }}>
                      {pricingLoading ? 'Pricing…' : '⚡ Re-price'}
                    </Btn>
                  </Row>
                </>
              )}

              {mdError && <Err style={{ marginBottom: '0.5rem' }}>⚠ {mdError}</Err>}

              {/* Leg analysis table */}
              {legResults.length > 0 && (
                <>
                  <SectionTitle style={{ marginTop: '0.5rem' }}>Leg Analysis</SectionTitle>
                  <div style={{ overflowX: 'auto' }}>
                    <PricerTable>
                      <thead>
                        <tr>
                          <PTh>Instrument</PTh>
                          <PTh>Side</PTh>
                          <PTh style={{ textAlign: 'right' }}>Qty</PTh>
                          <PTh style={{ textAlign: 'right' }}>Quoted</PTh>
                          <PTh style={{ textAlign: 'right' }}>BS Fair</PTh>
                          <PTh style={{ textAlign: 'right' }}>Diff</PTh>
                          <PTh style={{ textAlign: 'right' }}>IV %</PTh>
                          <PTh style={{ textAlign: 'right' }}>Δ</PTh>
                          <PTh style={{ textAlign: 'right' }}>Γ</PTh>
                          <PTh style={{ textAlign: 'right' }}>ν</PTh>
                          <PTh style={{ textAlign: 'right' }}>Θ</PTh>
                        </tr>
                      </thead>
                      <tbody>
                        {legResults.map((r, i) => {
                          const qLeg = selectedQuote.legs[i];
                          return (
                            <tr key={i}>
                              <PTd style={{ fontFamily: 'monospace', fontSize: '0.7rem', color: '#7eb8f7' }}>
                                {r.instrumentName}
                              </PTd>
                              <PTd><SideBadge $side={qLeg?.side ?? 'BUY'}>{qLeg?.side ?? '?'}</SideBadge></PTd>
                              <PTd $align="right">{qLeg?.quantity ?? '?'}</PTd>
                              <PTd $align="right" style={{ color: '#e8edf4', fontWeight: 600 }}>
                                {r.quotedPrice > 0 ? r.quotedPrice.toFixed(4) : qLeg?.price ?? '—'}
                              </PTd>
                              <PTd $align="right" style={{ color: '#c8d6e5' }}>
                                {r.isOption ? fmt(r.fairValue) : 'N/A'}
                              </PTd>
                              <DiffCell $pos={r.diff != null && r.diff < 0} $neg={r.diff != null && r.diff > 0}>
                                {r.isOption && r.diff != null ? (r.diff > 0 ? '+' : '') + r.diff.toFixed(4) : '—'}
                              </DiffCell>
                              <PTd $align="right" style={{ color: '#e0b94a' }}>
                                {r.iv != null ? (r.iv * 100).toFixed(1) + '%' : '—'}
                              </PTd>
                              <PTd $align="right">{r.greeks ? fmt(r.greeks.delta, 4) : '—'}</PTd>
                              <PTd $align="right">{r.greeks ? fmt(r.greeks.gamma, 6) : '—'}</PTd>
                              <PTd $align="right">{r.greeks ? fmt(r.greeks.vega, 4) : '—'}</PTd>
                              <PTd $align="right" style={{ color: '#e05252' }}>
                                {r.greeks ? fmt(r.greeks.theta, 4) : '—'}
                              </PTd>
                            </tr>
                          );
                        })}
                      </tbody>
                    </PricerTable>
                  </div>

                  {/* Net summary */}
                  <SectionTitle style={{ marginTop: '0.75rem' }}>Strategy Summary</SectionTitle>
                  <NetBox>
                    {net.totalDiff !== 0 && (
                      <NetStat>
                        <NetLabel>Total Diff (Q−Fair)</NetLabel>
                        <NetValue $color={net.totalDiff < 0 ? '#4ade80' : '#e05252'}>
                          {net.totalDiff >= 0 ? '+' : ''}{net.totalDiff.toFixed(4)}
                        </NetValue>
                      </NetStat>
                    )}
                    {net.hasGreeks && (
                      <>
                        <NetStat>
                          <NetLabel>Net Δ</NetLabel>
                          <NetValue $color={net.delta > 0.01 ? '#4ade80' : net.delta < -0.01 ? '#f87171' : undefined}>
                            {net.delta >= 0 ? '+' : ''}{net.delta.toFixed(4)}
                          </NetValue>
                        </NetStat>
                        <NetStat>
                          <NetLabel>Net Γ</NetLabel>
                          <NetValue $color={net.gamma > 0 ? '#4ade80' : net.gamma < 0 ? '#f87171' : undefined}>
                            {net.gamma >= 0 ? '+' : ''}{net.gamma.toFixed(5)}
                          </NetValue>
                        </NetStat>
                        <NetStat>
                          <NetLabel>Net ν</NetLabel>
                          <NetValue $color={net.vega > 0 ? '#4ade80' : net.vega < 0 ? '#f87171' : undefined}>
                            {net.vega >= 0 ? '+' : ''}{net.vega.toFixed(2)}
                          </NetValue>
                        </NetStat>
                        <NetStat>
                          <NetLabel>Net θ</NetLabel>
                          <NetValue $color={net.theta > 0 ? '#4ade80' : net.theta < 0 ? '#f87171' : undefined}>
                            {net.theta >= 0 ? '+' : ''}{net.theta.toFixed(2)}
                          </NetValue>
                        </NetStat>
                        <NetStat>
                          <NetLabel>Net Price</NetLabel>
                          <NetValue $color={net.netPrice > 0 ? '#4ade80' : net.netPrice < 0 ? '#f87171' : undefined}>
                            {net.netPrice >= 0 ? '+' : ''}{net.netPrice.toFixed(2)}
                          </NetValue>
                        </NetStat>
                      </>
                    )}
                    <NetStat>
                      <NetLabel>Quote State</NetLabel>
                      <NetValue><StatusBadge $state={selectedQuote.state}>{selectedQuote.state}</StatusBadge></NetValue>
                    </NetStat>
                  </NetBox>

                  {/* Accept button */}
                  {selectedRfq?.state === 'ACTIVE' && selectedQuote.state === 'OPEN' && (
                    <Btn
                      $variant="success" onClick={acceptQuote} disabled={acceptLoading}
                      style={{ marginTop: '0.75rem', width: '100%', padding: '0.5rem', fontSize: '0.82rem' }}
                    >
                      {acceptLoading ? 'Accepting…' : '✅ Accept This Quote'}
                    </Btn>
                  )}
                </>
              )}

              {pricingLoading && legResults.length === 0 && <Empty>Fetching market data & pricing…</Empty>}
            </Scroll>
          )}
        </ColRight>
        </Body>
      ) : mode === 'history' ? (
        /* ── HISTORY MODE ────────────────────────────────────────────────── */
        <Body style={{ flexDirection: 'column', overflow: 'auto' }}>
          <div style={{ padding: '0.5rem 0.75rem', borderBottom: '1px solid #1e2738', display: 'flex', gap: '1rem', alignItems: 'center' }}>
            <span style={{ fontSize: '0.78rem', color: '#7e8b99' }}>
              Filled block trades for <strong style={{ color: '#7eb8f7' }}>{tradingCoin}</strong>
            </span>
            <span style={{ fontSize: '0.72rem', color: '#4a5568' }}>{rfqs.filter(r => r.legs.some(l => l.instrumentName.toUpperCase().includes(tradingCoin))).length} trades</span>
          </div>
          <Scroll style={{ flex: 1 }}>
            {loadingRfqs && <Empty>Loading…</Empty>}
            {!loadingRfqs && rfqError && <Empty style={{ color: '#e07070' }}>⚠ {rfqError}</Empty>}
            {!loadingRfqs && !rfqError && rfqs.length === 0 && <Empty>No filled trades found</Empty>}
            {rfqs
              .filter(rfq => rfq.legs.some(l => l.instrumentName.toUpperCase().includes(tradingCoin)))
              .map(rfq => {
                // Compute net fill price (legs may have price field from filled trade)
                const netPrice = rfq.legs.reduce((sum, leg) => {
                  const px = parseFloat((leg as any).price ?? '0') || 0;
                  const qty = parseFloat(leg.qty) || 1;
                  const sign = leg.side === 'BUY' ? -1 : 1;
                  return sum + sign * px * qty;
                }, 0);
                return (
                  <Card key={rfq.requestId} $selected={false}>
                    <CardTitle>
                      <span style={{ fontFamily: 'monospace', fontSize: '0.72rem', color: '#7eb8f7' }}>
                        #{rfq.requestId.slice(-8)}
                      </span>
                      <StatusBadge $state={rfq.state}>{rfq.state}</StatusBadge>
                      {netPrice !== 0 && (
                        <span style={{ marginLeft: 'auto', fontFamily: 'monospace', fontSize: '0.75rem',
                          color: netPrice > 0 ? '#4ade80' : '#f87171' }}>
                          {netPrice > 0 ? '+' : ''}{netPrice.toFixed(2)} USD
                        </span>
                      )}
                    </CardTitle>
                    <CardMeta>{fmtTime(rfq.createTime)}</CardMeta>
                    {rfq.legs.map((leg, i) => {
                      const fillPx = (leg as any).price ?? (leg as any).tradePrice ?? (leg as any).fillPrice ?? null;
                      return (
                        <LegRow key={i} style={{ marginTop: '0.2rem' }}>
                          <SideBadge $side={leg.side}>{leg.side}</SideBadge>
                          <span style={{ fontSize: '0.72rem', color: '#c8d6e5', flex: 1 }}>{leg.instrumentName}</span>
                          <span style={{ fontSize: '0.7rem', color: '#7e8b99' }}>×{leg.qty}</span>
                          {fillPx != null && (
                            <span style={{ fontSize: '0.72rem', color: '#e0b94a', fontFamily: 'monospace', marginLeft: '0.4rem' }}>
                              @ {fillPx}
                            </span>
                          )}
                        </LegRow>
                      );
                    })}
                  </Card>
                );
              })}
          </Scroll>
        </Body>
      ) : null}
    </Wrapper>
  );
};

export default RfqPanel;
