import { FunctionComponent, useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import styled from 'styled-components';
import { useAppSelector } from '../../hooks';
import { selectAccounts, selectRfqSettings, Account, RfqSettings } from '../Settings/settingsSlice';

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
  indexPrice:     number | null;
  markIv:         number | null;
  instrumentUsed: string;
  error:          string | null;
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
  const accounts    = useAppSelector(selectAccounts);
  const rfqDefaults = useAppSelector(selectRfqSettings);
  const ccAccounts  = accounts.filter(a => a.exchange === 'coincall');

  const [selectedId, setSelectedId]     = useState<string>('');
  const [rfqs, setRfqs]                 = useState<ActiveRfq[]>([]);
  const [selectedRfq, setSelectedRfq]   = useState<ActiveRfq | null>(null);
  const [quotes, setQuotes]             = useState<Quote[]>([]);
  const [selectedQuote, setSelectedQuote] = useState<Quote | null>(null);
  const [loadingRfqs, setLoadingRfqs]   = useState(false);
  const [loadingQuotes, setLoadingQuotes] = useState(false);
  const [acceptLoading, setAcceptLoading] = useState(false);
  const [submitError, setSubmitError]   = useState('');
  const [submitLoading, setSubmitLoading] = useState(false);

  // Pricer state
  const [spotPrices, setSpotPrices]     = useState<Record<string, string>>({});
  const [riskFreeRate, setRiskFreeRate] = useState('');   // loaded from settings
  const [defaultVol, setDefaultVol]     = useState('');
  const [spotSource, setSpotSource]     = useState<string>('deribit');
  const [volSource, setVolSource]       = useState<string>('deribit');
  const [legResults, setLegResults]     = useState<LegPriceResult[]>([]);
  const [pricingLoading, setPricingLoading] = useState(false);
  const [mdError, setMdError]           = useState<string | null>(null);

  // Create form
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
      setRiskFreeRate(String((s.riskFreeRate * 100).toFixed(2)));
      setDefaultVol(String((s.defaultVol * 100).toFixed(0)));
      setSpotSource(s.spotSource);
      setVolSource(s.volSource);
    }).catch(() => {
      setRiskFreeRate(String((rfqDefaults.riskFreeRate * 100).toFixed(2)));
      setDefaultVol(String((rfqDefaults.defaultVol * 100).toFixed(0)));
      setSpotSource(rfqDefaults.spotSource);
      setVolSource(rfqDefaults.volSource);
    });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const loadRfqs = useCallback(async () => {
    if (!selectedId) return;
    setLoadingRfqs(true);
    try {
      const list = await invoke<ActiveRfq[]>('coincall_get_rfq_list', { accountId: selectedId });
      setRfqs(list ?? []);
    } catch { setRfqs([]); }
    finally { setLoadingRfqs(false); }
  }, [selectedId]);

  const loadQuotes = useCallback(async (rfq: ActiveRfq) => {
    if (!selectedId) return;
    setSelectedRfq(rfq);
    setSelectedQuote(null);
    setLegResults([]);
    setLoadingQuotes(true);
    try {
      const list = await invoke<Quote[]>('coincall_get_rfq_quotes', { accountId: selectedId, requestId: rfq.requestId });
      setQuotes(Array.isArray(list) ? list : []);
    } catch { setQuotes([]); }
    finally { setLoadingQuotes(false); }
  }, [selectedId]);

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
          exchange: spotSource,
          instrumentOverride: null,
          testnet: false,
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
  }, [spotSource, spotPrices, riskFreeRate, defaultVol]);

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
        riskFreeRate: parseFloat(riskFreeRate) / 100 || 0.05,
        defaultVol: parseFloat(defaultVol) / 100 || 0.80,
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

  // Net strategy summary from leg results
  const net = legResults.reduce(
    (acc, r) => {
      if (r.greeks) {
        acc.delta += r.greeks.delta;
        acc.vega  += r.greeks.vega;
      }
      if (r.diff != null) acc.totalDiff += r.diff;
      acc.hasGreeks = acc.hasGreeks || r.greeks != null;
      return acc;
    },
    { delta: 0, vega: 0, totalDiff: 0, hasGreeks: false }
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
        <Btn onClick={loadRfqs} disabled={loadingRfqs}>{loadingRfqs ? '⟳' : '↺'} Refresh</Btn>
        <div style={{ borderLeft: '1px solid #1e2738', paddingLeft: '0.75rem', display: 'flex', alignItems: 'center', gap: '0.5rem' }}>
          <Label>
            Spot src
            <Select value={spotSource} onChange={e => setSpotSource(e.target.value)} style={{ width: 90 }}>
              {EXCHANGES_PRICER.map(ex => <option key={ex} value={ex}>{ex}</option>)}
            </Select>
          </Label>
          <Label>
            Vol src
            <Select value={volSource} onChange={e => setVolSource(e.target.value)} style={{ width: 90 }}>
              {EXCHANGES_PRICER.map(ex => <option key={ex} value={ex}>{ex}</option>)}
            </Select>
          </Label>
          <Label>
            RFR %
            <SmallInput
              type="number" step="0.01" min="0" max="50"
              value={riskFreeRate}
              onChange={e => setRiskFreeRate(e.target.value)}
              style={{ width: 60 }}
            />
          </Label>
          <Label>
            Vol % (fallback)
            <SmallInput
              type="number" step="1" min="1" max="2000"
              value={defaultVol}
              onChange={e => setDefaultVol(e.target.value)}
              style={{ width: 60 }}
            />
          </Label>
        </div>
        {account?.testnet && (
          <span style={{ fontSize: '0.72rem', color: '#e0b94a', marginLeft: 'auto' }}>⚠ Testnet</span>
        )}
      </TopBar>

      <Body>
        {/* ── Create RFQ ────────────────────────────────────────────────── */}
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

        {/* ── Active RFQs ───────────────────────────────────────────────── */}
        <Col $width="280px">
          <ColHeader>
            Active RFQs
            <span style={{ fontSize: '0.72rem', color: '#4a5568' }}>{rfqs.length} total</span>
          </ColHeader>
          <Scroll>
            {rfqs.length === 0 && !loadingRfqs && <Empty>No RFQs found</Empty>}
            {loadingRfqs && <Empty>Loading…</Empty>}
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

        {/* ── Quotes Received ───────────────────────────────────────────── */}
        <Col $width="260px">
          <ColHeader>
            Quotes
            {selectedRfq && <span style={{ fontSize: '0.72rem', color: '#4a5568' }}>{quotes.length} received</span>}
          </ColHeader>
          <Scroll>
            {!selectedRfq && <Empty>Select an RFQ</Empty>}
            {selectedRfq && loadingQuotes && <Empty>Loading…</Empty>}
            {selectedRfq && !loadingQuotes && quotes.length === 0 && <Empty>No quotes yet</Empty>}
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

        {/* ── BS Pricer ─────────────────────────────────────────────────── */}
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
                      <MdSourceRow>via {spotSource}</MdSourceRow>
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
                          <NetLabel>Net Δ Delta</NetLabel>
                          <NetValue>{net.delta >= 0 ? '+' : ''}{net.delta.toFixed(4)}</NetValue>
                        </NetStat>
                        <NetStat>
                          <NetLabel>Net ν Vega</NetLabel>
                          <NetValue>{net.vega >= 0 ? '+' : ''}{net.vega.toFixed(4)}</NetValue>
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
    </Wrapper>
  );
};

export default RfqPanel;
