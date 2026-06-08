import { FunctionComponent, useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import styled from 'styled-components';
import { useAppSelector } from '../../hooks';
import { selectAccounts, Account } from '../Settings/settingsSlice';

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

const Btn = styled.button<{ $variant?: 'primary' | 'danger' | 'ghost' }>`
  padding: 0.25rem 0.6rem;
  border-radius: 3px;
  border: 1px solid ${p =>
    p.$variant === 'danger' ? '#7b2929' :
    p.$variant === 'primary' ? '#3a5a8c' : '#1e2738'};
  background: ${p =>
    p.$variant === 'danger' ? '#2a1a1a' :
    p.$variant === 'primary' ? '#1e3558' : 'transparent'};
  color: ${p =>
    p.$variant === 'danger' ? '#e05252' :
    p.$variant === 'primary' ? '#7eb8f7' : '#7e8b99'};
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
    p.$state === 'ACTIVE' ? '#173a1a' :
    p.$state === 'FILLED' ? '#1a3a26' :
    p.$state === 'EXPIRED' || p.$state === 'CANCELLED' ? '#2a1a1a' : '#1e2738'};
  color: ${p =>
    p.$state === 'ACTIVE' ? '#4ade80' :
    p.$state === 'FILLED' ? '#34d399' :
    p.$state === 'EXPIRED' || p.$state === 'CANCELLED' ? '#e05252' : '#7e8b99'};
  border: 1px solid ${p =>
    p.$state === 'ACTIVE' ? '#2a5a2f' :
    p.$state === 'FILLED' ? '#256040' :
    p.$state === 'EXPIRED' || p.$state === 'CANCELLED' ? '#5a2a2a' : '#1e2738'};
`;

const SideBadge = styled.span<{ $side: string }>`
  font-size: 0.72rem;
  padding: 0.1rem 0.35rem;
  border-radius: 3px;
  background: ${p => p.$side === 'BUY' || p.$side === 'buy' ? '#0e2a1a' : '#2a0e0e'};
  color: ${p => p.$side === 'BUY' || p.$side === 'buy' ? '#4ade80' : '#e05252'};
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

// ── Component ──────────────────────────────────────────────────────────────

const RfqPanel: FunctionComponent = () => {
  const accounts = useAppSelector(selectAccounts);
  const ccAccounts = accounts.filter(a => a.exchange === 'coincall');

  const [selectedId, setSelectedId] = useState<string>('');
  const [rfqs, setRfqs]             = useState<ActiveRfq[]>([]);
  const [selectedRfq, setSelectedRfq] = useState<ActiveRfq | null>(null);
  const [quotes, setQuotes]         = useState<Quote[]>([]);
  const [loadingRfqs, setLoadingRfqs] = useState(false);
  const [loadingQuotes, setLoadingQuotes] = useState(false);
  const [submitError, setSubmitError] = useState('');
  const [submitLoading, setSubmitLoading] = useState(false);

  // Create form state
  const [legs, setLegs] = useState<RfqLegFull[]>([
    { instrumentName: '', side: 'BUY', qty: '' },
    { instrumentName: '', side: 'SELL', qty: '' },
  ]);

  const account = ccAccounts.find(a => a.id === selectedId) as Account | undefined;

  // auto-select first coincall account
  useEffect(() => {
    if (ccAccounts.length > 0 && !selectedId) setSelectedId(ccAccounts[0].id);
  }, [ccAccounts.length]); // eslint-disable-line react-hooks/exhaustive-deps

  const loadRfqs = useCallback(async () => {
    if (!selectedId) return;
    setLoadingRfqs(true);
    try {
      const list = await invoke<ActiveRfq[]>('coincall_get_rfq_list', { accountId: selectedId });
      setRfqs(list ?? []);
    } catch (e: any) {
      // fallback: empty list
      setRfqs([]);
    } finally {
      setLoadingRfqs(false);
    }
  }, [selectedId]);

  const loadQuotes = useCallback(async (rfq: ActiveRfq) => {
    if (!selectedId) return;
    setSelectedRfq(rfq);
    setLoadingQuotes(true);
    try {
      const list = await invoke<Quote[]>('coincall_get_rfq_quotes', { accountId: selectedId, requestId: rfq.requestId });
      setQuotes(Array.isArray(list) ? list : []);
    } catch {
      setQuotes([]);
    } finally {
      setLoadingQuotes(false);
    }
  }, [selectedId]);

  const cancelRfq = async (requestId: string) => {
    if (!selectedId) return;
    try {
      await invoke('coincall_cancel_rfq', { accountId: selectedId, requestId });
      setRfqs(prev => prev.map(r => r.requestId === requestId ? { ...r, state: 'CANCELLED' } : r));
      if (selectedRfq?.requestId === requestId) setSelectedRfq(r => r ? { ...r, state: 'CANCELLED' } : null);
    } catch (e: any) {
      alert('Cancel failed: ' + String(e));
    }
  };

  const createRfq = async () => {
    if (!selectedId) return;
    setSubmitError('');
    const validLegs = legs.filter(l => l.instrumentName.trim() && l.qty.trim());
    if (validLegs.length < 1) { setSubmitError('Add at least one leg'); return; }
    setSubmitLoading(true);
    try {
      const rfqLegs = validLegs.map(l => ({ instrumentName: l.instrumentName.trim(), side: l.side, qty: l.qty.trim() }));
      const created = await invoke<ActiveRfq>('coincall_create_rfq', { accountId: selectedId, legs: rfqLegs });
      setRfqs(prev => [created, ...prev]);
      // Reset legs
      setLegs([{ instrumentName: '', side: 'BUY', qty: '' }, { instrumentName: '', side: 'SELL', qty: '' }]);
    } catch (e: any) {
      setSubmitError(String(e));
    } finally {
      setSubmitLoading(false);
    }
  };

  const addLeg = () => setLegs(prev => [...prev, { instrumentName: '', side: 'BUY', qty: '' }]);
  const removeLeg = (i: number) => setLegs(prev => prev.filter((_, idx) => idx !== i));

  const updateLeg = (i: number, field: keyof RfqLegFull, value: string) => {
    setLegs(prev => prev.map((l, idx) => idx === i ? { ...l, [field]: value } : l));
  };

  useEffect(() => { loadRfqs(); }, [loadRfqs]);

  const fmtTime = (ms: number) => {
    if (!ms) return '—';
    return new Date(ms).toLocaleTimeString('en-US', { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' });
  };

  const timeLeft = (expiryMs: number) => {
    const diff = expiryMs - Date.now();
    if (diff <= 0) return 'Expired';
    const m = Math.floor(diff / 60000);
    const s = Math.floor((diff % 60000) / 1000);
    return `${m}m ${s}s`;
  };

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
      {/* Top bar: account selector + refresh */}
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
        {account?.testnet && (
          <span style={{ fontSize: '0.72rem', color: '#e0b94a', marginLeft: 'auto' }}>⚠ Testnet</span>
        )}
      </TopBar>

      <Body>
        {/* ── Left: Create RFQ ─────────────────────────────────────── */}
        <Col $width="380px">
          <ColHeader>Create RFQ</ColHeader>
          <Scroll>
            <CreateForm>
              <SectionTitle>Legs</SectionTitle>
              {legs.map((leg, i) => (
                <LegRow key={i}>
                  <Input
                    placeholder="Instrument e.g. BTCUSD-27JUN25-100000-C"
                    value={leg.instrumentName}
                    onChange={e => updateLeg(i, 'instrumentName', e.target.value)}
                    style={{ flex: 3, minWidth: 160 }}
                  />
                  <Select
                    value={leg.side}
                    onChange={e => updateLeg(i, 'side', e.target.value)}
                    style={{ width: 70 }}
                  >
                    <option value="BUY">BUY</option>
                    <option value="SELL">SELL</option>
                  </Select>
                  <Input
                    placeholder="Qty"
                    value={leg.qty}
                    onChange={e => updateLeg(i, 'qty', e.target.value)}
                    style={{ flex: 1, width: 70, minWidth: 50 }}
                  />
                  <Btn $variant="danger" onClick={() => removeLeg(i)} disabled={legs.length <= 1}>✕</Btn>
                </LegRow>
              ))}

              <Row style={{ marginTop: '0.4rem' }}>
                <Btn onClick={addLeg}>+ Add Leg</Btn>
              </Row>

              {submitError && <Err>{submitError}</Err>}

              <Btn
                $variant="primary"
                onClick={createRfq}
                disabled={submitLoading || !selectedId}
                style={{ marginTop: '0.5rem', width: '100%', padding: '0.4rem' }}
              >
                {submitLoading ? 'Creating…' : '📤 Submit RFQ'}
              </Btn>
            </CreateForm>
          </Scroll>
        </Col>

        {/* ── Middle: Active RFQs ───────────────────────────────────── */}
        <Col $width="360px">
          <ColHeader>
            Active RFQs
            <span style={{ fontSize: '0.72rem', color: '#4a5568' }}>{rfqs.length} total</span>
          </ColHeader>
          <Scroll>
            {rfqs.length === 0 && !loadingRfqs && <Empty>No RFQs found</Empty>}
            {loadingRfqs && <Empty>Loading…</Empty>}
            {rfqs.map(rfq => (
              <Card
                key={rfq.requestId}
                $selected={selectedRfq?.requestId === rfq.requestId}
                onClick={() => loadQuotes(rfq)}
              >
                <CardTitle>
                  <span style={{ fontFamily: 'monospace', fontSize: '0.72rem', color: '#7eb8f7' }}>
                    #{rfq.requestId.slice(-8)}
                  </span>
                  <div style={{ display: 'flex', gap: '0.3rem', alignItems: 'center' }}>
                    <StatusBadge $state={rfq.state}>{rfq.state}</StatusBadge>
                    {rfq.state === 'ACTIVE' && (
                      <Btn $variant="danger" onClick={e => { e.stopPropagation(); cancelRfq(rfq.requestId); }}>
                        Cancel
                      </Btn>
                    )}
                  </div>
                </CardTitle>
                <CardMeta>
                  <div>Created: {fmtTime(rfq.createTime)} · Expires: {rfq.state === 'ACTIVE' ? timeLeft(rfq.expiryTime) : fmtTime(rfq.expiryTime)}</div>
                </CardMeta>
                {rfq.legs.map((leg, i) => (
                  <LegRow key={i} style={{ marginTop: '0.2rem' }}>
                    <SideBadge $side={leg.side}>{leg.side}</SideBadge>
                    <span style={{ fontSize: '0.78rem', color: '#c8d6e5' }}>{leg.instrumentName}</span>
                    <span style={{ fontSize: '0.75rem', color: '#7e8b99', marginLeft: 'auto' }}>qty: {leg.qty}</span>
                  </LegRow>
                ))}
                <CardMeta style={{ marginTop: '0.3rem', color: '#4a5568' }}>
                  Click to view quotes →
                </CardMeta>
              </Card>
            ))}
          </Scroll>
        </Col>

        {/* ── Right: Quotes ─────────────────────────────────────────── */}
        <ColRight>
          <ColHeader>
            Quotes Received
            {selectedRfq && (
              <span style={{ fontSize: '0.72rem', color: '#4a5568' }}>
                RFQ #{selectedRfq.requestId.slice(-8)} · {quotes.length} quote{quotes.length !== 1 ? 's' : ''}
              </span>
            )}
          </ColHeader>
          <Scroll>
            {!selectedRfq && <Empty>Select an RFQ to view quotes</Empty>}
            {selectedRfq && loadingQuotes && <Empty>Loading quotes…</Empty>}
            {selectedRfq && !loadingQuotes && quotes.length === 0 && <Empty>No quotes received yet</Empty>}
            {quotes.map(q => (
              <Card key={q.quoteId}>
                <CardTitle>
                  <span style={{ fontFamily: 'monospace', fontSize: '0.72rem', color: '#7eb8f7' }}>
                    Quote #{q.quoteId.slice(-8)}
                  </span>
                  <StatusBadge $state={q.state}>{q.state}</StatusBadge>
                </CardTitle>
                <CardMeta>
                  From: {q.userId.slice(-6)} · Expires: {fmtTime(q.expiryTime)}
                </CardMeta>
                {q.legs.map((leg, i) => (
                  <LegRow key={i} style={{ marginTop: '0.3rem' }}>
                    <SideBadge $side={leg.side}>{leg.side}</SideBadge>
                    <span style={{ fontSize: '0.78rem', color: '#c8d6e5', flex: 1 }}>{leg.instrumentName}</span>
                    <span style={{ fontSize: '0.82rem', color: '#e8edf4', fontWeight: 600, marginRight: '0.5rem' }}>
                      {leg.price}
                    </span>
                    <span style={{ fontSize: '0.75rem', color: '#7e8b99' }}>× {leg.quantity}</span>
                  </LegRow>
                ))}
              </Card>
            ))}
          </Scroll>
        </ColRight>
      </Body>
    </Wrapper>
  );
};

export default RfqPanel;
