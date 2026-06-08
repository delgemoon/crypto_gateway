import { FunctionComponent, useState, useRef, useEffect } from 'react';
import styled from 'styled-components';
import { invoke } from '@tauri-apps/api/core';
import { useAppSelector } from '../../hooks';
import { selectAggBookConfigs, selectAggBookSnapshots, AggBookSnapshot, AggLevel, AggBookConfig } from './aggBookSlice';
import { selectAccounts } from '../Settings/settingsSlice';
import { selectWsStatus } from '../WsManager/wsSlice';

// ── Exchange colour palette ────────────────────────────────────────────────

const EXCHANGE_COLORS: Record<string, string> = {
  deribit:     '#5087f2',
  okx:         '#e0b94a',
  bybit:       '#f7a600',
  coincall:    '#33b48f',
  binance:     '#f0b90b',
  mexc:        '#1db1a8',
  hyperliquid: '#00e5ff',
  uniswap:     '#ff007a',
};

function exColor(exchange: string): string {
  return EXCHANGE_COLORS[exchange.toLowerCase()] ?? '#7e8b99';
}

// ── Styled components ─────────────────────────────────────────────────────

const Wrapper = styled.div`
  display: flex;
  height: 100%;
  overflow: hidden;
  background: #0d1520;
  color: #c8d6e5;
  font-family: 'Roboto Mono', monospace, sans-serif;
  font-size: 0.82rem;
`;

const Sidebar = styled.div`
  width: 200px;
  flex-shrink: 0;
  background: #0d1117;
  border-right: 1px solid #1a2233;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
`;

const SidebarTitle = styled.div`
  padding: 0.75rem 1rem;
  font-size: 0.78rem;
  font-weight: 600;
  color: #7e8b99;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  border-bottom: 1px solid #1a2233;
`;

const ConfigItem = styled.div<{ $active: boolean }>`
  padding: 0.6rem 1rem;
  cursor: pointer;
  border-left: 3px solid ${p => p.$active ? '#5087f2' : 'transparent'};
  background: ${p => p.$active ? 'rgba(80,135,242,0.08)' : 'transparent'};
  &:hover { background: rgba(255,255,255,0.04); }

  .name { font-weight: 600; color: #d9dde4; font-size: 0.82rem; }
  .meta { font-size: 0.7rem; color: #4a5568; margin-top: 0.15rem; }
`;

const MainArea = styled.div`
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
`;

const Header = styled.div`
  padding: 0.6rem 1rem;
  background: #0d1117;
  border-bottom: 1px solid #1a2233;
  display: flex;
  align-items: center;
  gap: 1rem;
  flex-shrink: 0;
`;

const HeaderTitle = styled.span`
  font-size: 0.9rem;
  font-weight: 700;
  color: #e8edf4;
`;

const KindBadge = styled.span`
  font-size: 0.7rem;
  padding: 0.15rem 0.45rem;
  border-radius: 3px;
  background: rgba(80,135,242,0.15);
  border: 1px solid rgba(80,135,242,0.3);
  color: #5087f2;
`;

const StatusBadge = styled.span<{ $status: string }>`
  font-size: 0.68rem;
  padding: 0.1rem 0.4rem;
  border-radius: 3px;
  background: ${p => p.$status === 'ok'
    ? 'rgba(51,180,143,0.15)'
    : p.$status.startsWith('error')
    ? 'rgba(208,97,110,0.15)'
    : 'rgba(255,255,255,0.06)'};
  border: 1px solid ${p => p.$status === 'ok'
    ? 'rgba(51,180,143,0.3)'
    : p.$status.startsWith('error')
    ? 'rgba(208,97,110,0.3)'
    : 'rgba(255,255,255,0.1)'};
  color: ${p => p.$status === 'ok' ? '#33b48f' : p.$status.startsWith('error') ? '#d0616e' : '#7e8b99'};
`;

const BookArea = styled.div`
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
`;

const HalfBook = styled.div`
  flex: 1;
  overflow-y: auto;
`;

const BookTable = styled.table`
  width: 100%;
  border-collapse: collapse;
`;

const BookTHead = styled.thead`
  position: sticky;
  top: 0;
  background: #0d1117;
  z-index: 1;

  th {
    padding: 0.35rem 0.6rem;
    text-align: right;
    font-size: 0.68rem;
    font-weight: 500;
    color: #4a5568;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    border-bottom: 1px solid #1a2233;
    &:first-child { text-align: left; }
  }
`;

const BookRow = styled.tr<{ $side: 'bid' | 'ask'; $hovered?: boolean }>`
  border-bottom: 1px solid rgba(255,255,255,0.03);
  background: ${p => p.$hovered ? 'rgba(255,255,255,0.06)' : 'transparent'};

  td {
    padding: 0.28rem 0.6rem;
    text-align: right;
    &:first-child { text-align: left; }
  }

  .price {
    color: ${p => p.$side === 'bid' ? '#33b48f' : '#d0616e'};
    font-weight: 600;
    font-size: 0.84rem;
  }
  .size { color: #d9dde4; }
  .contribs { display: flex; gap: 3px; flex-wrap: wrap; justify-content: flex-end; }
`;

const ExBadge = styled.span<{ $color: string }>`
  display: inline-flex;
  align-items: center;
  gap: 2px;
  font-size: 0.65rem;
  padding: 0.05rem 0.3rem;
  border-radius: 3px;
  background: ${p => p.$color}22;
  border: 1px solid ${p => p.$color}55;
  color: ${p => p.$color};
  white-space: nowrap;
`;

const SpreadRow = styled.div`
  padding: 0.25rem 0.6rem;
  background: #111827;
  border-top: 1px solid #1a2233;
  border-bottom: 1px solid #1a2233;
  font-size: 0.72rem;
  color: #4a5568;
  display: flex;
  gap: 1rem;
  flex-shrink: 0;
`;

const EmptyState = styled.div`
  display: flex;
  align-items: center;
  justify-content: center;
  flex: 1;
  color: #4a5568;
  font-size: 0.85rem;
`;

// ── Order panel styles ────────────────────────────────────────────────────

const RightPanel = styled.div`
  width: 300px;
  flex-shrink: 0;
  background: #0d1117;
  border-left: 1px solid #1a2233;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
`;

const OFHeader = styled.div`
  padding: 0.65rem 1rem;
  font-size: 0.8rem;
  font-weight: 600;
  color: #7e8b99;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  border-bottom: 1px solid #1a2233;
  flex-shrink: 0;
`;

const OFBody = styled.div`
  padding: 0.9rem 1rem;
  display: flex;
  flex-direction: column;
  gap: 0.7rem;
  flex: 1;
`;

const OFLabel = styled.div`
  color: #4a5568;
  font-size: 0.75rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
`;

const OFSelect = styled.select`
  background: #0f1522;
  border: 1px solid #29303e;
  color: #e8edf4;
  padding: 0.45rem 0.6rem;
  border-radius: 3px;
  font-size: 0.9rem;
  width: 100%;
  &:focus { border-color: #5087f2; outline: none; }
`;

const OFInput = styled.input`
  background: #0f1522;
  border: 1px solid #29303e;
  color: #e8edf4;
  padding: 0.45rem 0.6rem;
  border-radius: 3px;
  font-size: 0.9rem;
  width: 100%;
  &:focus { border-color: #5087f2; outline: none; }
  &:disabled { opacity: 0.45; }
`;

const TabRow = styled.div`
  display: flex;
  gap: 2px;
  flex-shrink: 0;
`;

const SideTab = styled.button<{ $side: 'buy' | 'sell'; $active: boolean }>`
  flex: 1;
  padding: 0.55rem;
  border: 1px solid ${p => !p.$active ? '#29303e' : p.$side === 'buy' ? '#33b48f' : '#d0616e'};
  border-radius: 3px;
  cursor: pointer;
  font-size: 0.9rem;
  font-weight: 600;
  background: ${p => !p.$active ? '#141a28'
    : p.$side === 'buy' ? 'rgba(51,180,143,0.15)' : 'rgba(208,97,110,0.15)'};
  color: ${p => !p.$active ? '#4a5568'
    : p.$side === 'buy' ? '#33b48f' : '#d0616e'};
  transition: all 0.12s;
  &:hover { opacity: 0.85; }
`;

const OFSubmit = styled.button<{ $side: 'buy' | 'sell' }>`
  width: 100%;
  padding: 0.75rem;
  border: 1px solid ${p => p.$side === 'buy' ? '#33b48f55' : '#d0616e55'};
  border-radius: 3px;
  cursor: pointer;
  font-size: 1rem;
  font-weight: 700;
  background: ${p => p.$side === 'buy' ? '#0f3320' : '#3a1010'};
  color: ${p => p.$side === 'buy' ? '#33b48f' : '#d0616e'};
  margin-top: auto;
  transition: opacity 0.12s;
  &:hover { opacity: 0.85; }
  &:disabled { opacity: 0.45; cursor: not-allowed; }
`;

const OFResult = styled.div<{ $ok: boolean }>`
  font-size: 0.8rem;
  padding: 0.4rem 0.65rem;
  border-radius: 3px;
  background: ${p => p.$ok ? '#0f3320' : '#3a1010'};
  color: ${p => p.$ok ? '#33b48f' : '#d0616e'};
  border: 1px solid ${p => p.$ok ? '#33b48f30' : '#d0616e30'};
  word-break: break-all;
`;

const InstTag = styled.div`
  font-size: 0.85rem;
  color: #5087f2;
  padding: 0.2rem 0;
  font-weight: 600;
`;

const WsDot = styled.span<{ $ok: boolean }>`
  display: inline-block;
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: ${p => p.$ok ? '#3fb950' : '#8b949e'};
  margin-right: 4px;
  flex-shrink: 0;
`;

// ── Helpers ────────────────────────────────────────────────────────────────

function fmtPrice(n: number): string {
  if (n >= 10000) return n.toFixed(1);
  if (n >= 1000)  return n.toFixed(2);
  if (n >= 100)   return n.toFixed(3);
  return n.toFixed(4);
}

function fmtSize(n: number): string {
  if (n >= 1000000) return (n / 1000000).toFixed(2) + 'M';
  if (n >= 1000)    return (n / 1000).toFixed(2) + 'K';
  return n.toFixed(4);
}

// ── Agg Order Form ────────────────────────────────────────────────────────

interface AggOrderFormProps {
  config: AggBookConfig;
  snapshot: AggBookSnapshot | null;
  prefilledPrice: number | null;
  onClearPrice: () => void;
}

const AggOrderForm: FunctionComponent<AggOrderFormProps> = ({
  config, prefilledPrice, onClearPrice,
}) => {
  const allAccounts = useAppSelector(selectAccounts);
  const configAccounts = allAccounts.filter(a => config.accountIds.includes(a.id));

  const [accountId, setAccountId] = useState<string>(configAccounts[0]?.id ?? '');
  const [side, setSide] = useState<'buy' | 'sell'>('buy');
  const [orderType, setOrderType] = useState('limit');
  const [price, setPrice] = useState('');
  const [amount, setAmount] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; msg: string } | null>(null);
  const mountedRef = useRef(true);
  useEffect(() => { mountedRef.current = true; return () => { mountedRef.current = false; }; }, []);

  // When a price level is clicked in the book, apply it
  useEffect(() => {
    if (prefilledPrice != null) {
      setPrice(prefilledPrice.toString());
      onClearPrice();
    }
  }, [prefilledPrice]);

  // Resolve instrument for selected account by invoking fetch_instruments once per account change
  const [instrument, setInstrument] = useState<string | null>(null);

  useEffect(() => {
    if (!accountId) { setInstrument(null); return; }
    // Map instrument_kind to fetch kind
    const fetchKind = config.instrumentKind.startsWith('perpetual') ? 'future'
      : config.instrumentKind === 'option' ? 'option'
      : config.instrumentKind === 'spot' ? 'spot'
      : 'future';
    invoke<any[]>('fetch_instruments', {
      accountId,
      currency: config.baseSymbol,
      kind: fetchKind,
    }).then(list => {
      const match = list.find((i: any) => {
        const name: string = i.instrument_name ?? '';
        const kind: string = i.kind ?? '';
        const settle: string = (i.settlement_currency ?? '').toUpperCase();
        const stablecoins = ['USDT','USDC','USD','BUSD'];
        if (config.instrumentKind === 'perpetual_inverse') {
          return (kind === 'perpetual' || name.toUpperCase().includes('PERPETUAL'))
            && !stablecoins.includes(settle);
        }
        if (config.instrumentKind === 'perpetual_linear') {
          return (kind === 'perpetual' || name.toUpperCase().includes('PERPETUAL'))
            && stablecoins.includes(settle);
        }
        return kind === config.instrumentKind;
      });
      setInstrument(match?.instrument_name ?? null);
    }).catch(() => setInstrument(null));
  }, [accountId, config.baseSymbol, config.instrumentKind]);

  const wsStatus = useAppSelector(selectWsStatus(accountId));
  const wsOk = wsStatus === 'connected';

  const showPrice = ['limit', 'limit_post', 'stop_limit'].includes(orderType);
  const isPost = orderType === 'limit_post';

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!accountId || !instrument || submitting) return;
    setSubmitting(true);
    setResult(null);
    try {
      const res = await invoke<any>('place_order', {
        req: {
          account_id: accountId,
          instrument_name: instrument,
          side,
          order_type: isPost ? 'limit' : orderType,
          amount: parseFloat(amount),
          price: showPrice && price ? parseFloat(price) : null,
          time_in_force: 'good_til_cancelled',
          post_only: isPost,
          label: null,
        },
      });
      if (mountedRef.current) {
        setResult({ ok: res.success ?? true, msg: res.order?.order_id ?? 'Order placed' });
        if (res.success) { setAmount(''); }
      }
    } catch (err: any) {
      if (mountedRef.current) setResult({ ok: false, msg: String(err) });
    } finally {
      if (mountedRef.current) setSubmitting(false);
    }
  };

  if (configAccounts.length === 0) {
    return (
      <RightPanel>
        <OFHeader>Order</OFHeader>
        <OFBody>
          <div style={{ color: '#4a5568', fontSize: '0.75rem' }}>
            No accounts linked to this config.
          </div>
        </OFBody>
      </RightPanel>
    );
  }

  return (
    <RightPanel>
      <OFHeader>Order</OFHeader>
      <OFBody as="form" onSubmit={handleSubmit}>
        {/* Account */}
        <OFLabel>
          Account
          <OFSelect value={accountId} onChange={e => setAccountId(e.target.value)}>
            {configAccounts.map(a => (
              <option key={a.id} value={a.id}>{a.name} ({a.exchange})</option>
            ))}
          </OFSelect>
        </OFLabel>

        {/* WS + Instrument */}
        <div style={{ display: 'flex', alignItems: 'center', gap: '4px', fontSize: '0.78rem', color: '#7e8b99' }}>
          <WsDot $ok={wsOk} />
          {wsOk ? 'WS Live' : 'WS Offline'}
        </div>
        {instrument
          ? <InstTag>{instrument}</InstTag>
          : <div style={{ fontSize: '0.72rem', color: '#4a5568' }}>No instrument yet</div>
        }

        {/* Side */}
        <TabRow>
          <SideTab type="button" $side="buy" $active={side === 'buy'} onClick={() => setSide('buy')}>Buy</SideTab>
          <SideTab type="button" $side="sell" $active={side === 'sell'} onClick={() => setSide('sell')}>Sell</SideTab>
        </TabRow>

        {/* Order type */}
        <OFLabel>
          Type
          <OFSelect value={orderType} onChange={e => setOrderType(e.target.value)}>
            <option value="limit">Limit</option>
            <option value="limit_post">Limit Post-Only</option>
            <option value="market">Market</option>
            <option value="stop_limit">Stop Limit</option>
            <option value="stop_market">Stop Market</option>
          </OFSelect>
        </OFLabel>

        {/* Price */}
        {showPrice && (
          <OFLabel>
            Price
            <OFInput
              type="number"
              step="any"
              value={price}
              onChange={e => setPrice(e.target.value)}
              placeholder="0.00"
            />
          </OFLabel>
        )}

        {/* Amount */}
        <OFLabel>
          Amount
          <OFInput
            type="number"
            step="any"
            value={amount}
            onChange={e => setAmount(e.target.value)}
            placeholder="0.00"
            required
          />
        </OFLabel>

        {result && <OFResult $ok={result.ok}>{result.ok ? '✓ ' : '✗ '}{result.msg}</OFResult>}

        <OFSubmit
          type="submit"
          $side={side}
          disabled={!instrument || !amount || submitting}
        >
          {submitting ? 'Sending…' : `${side === 'buy' ? 'Buy' : 'Sell'} ${instrument ?? '—'}`}
        </OFSubmit>
      </OFBody>
    </RightPanel>
  );
};

// ── Book level row ────────────────────────────────────────────────────────

interface LevelRowProps {
  level: AggLevel;
  side: 'bid' | 'ask';
  hoveredExchange: string | null;
  onHoverExchange: (exchange: string | null) => void;
  onClickPrice: (price: number) => void;
}

const LevelRow: FunctionComponent<LevelRowProps> = ({ level, side, hoveredExchange, onHoverExchange, onClickPrice }) => {
  const isHovered = hoveredExchange != null && level.contributions.some(c => c.exchange === hoveredExchange);
  return (
    <BookRow $side={side} $hovered={isHovered}>
      <td
        className="price"
        style={{ cursor: 'pointer' }}
        title="Click to fill order price"
        onClick={() => onClickPrice(level.price)}
      >
        {fmtPrice(level.price)}
      </td>
      <td className="size">{fmtSize(level.totalSize)}</td>
      <td>
        <div className="contribs">
          {level.contributions.map((c, i) => (
            <ExBadge
              key={i}
              $color={exColor(c.exchange)}
              onMouseEnter={() => onHoverExchange(c.exchange)}
              onMouseLeave={() => onHoverExchange(null)}
              title={`${c.exchange} · ${fmtSize(c.size)}`}
            >
              {c.exchange.slice(0, 3).toUpperCase()} {fmtSize(c.size)}
            </ExBadge>
          ))}
        </div>
      </td>
    </BookRow>
  );
};

// ── Snapshot view ─────────────────────────────────────────────────────────

interface SnapshotViewProps {
  snapshot: AggBookSnapshot;
  onClickPrice: (price: number) => void;
}

const SnapshotView: FunctionComponent<SnapshotViewProps> = ({ snapshot, onClickPrice }) => {
  const [hoveredExchange, setHoveredExchange] = useState<string | null>(null);

  const bestBid = snapshot.bids[0]?.price ?? 0;
  const bestAsk = snapshot.asks[0]?.price ?? 0;
  const spread  = bestAsk > 0 && bestBid > 0 ? bestAsk - bestBid : 0;
  const spreadPct = bestBid > 0 && spread > 0 ? (spread / bestBid * 100) : 0;

  return (
    <>
      <Header>
        <HeaderTitle>{snapshot.name}</HeaderTitle>
        <KindBadge>{snapshot.instrumentKind}</KindBadge>
        <span style={{ color: '#7e8b99', fontSize: '0.78rem' }}>{snapshot.baseSymbol}</span>
        <div style={{ display: 'flex', gap: '0.3rem', marginLeft: 'auto', flexWrap: 'wrap' }}>
          {Object.entries(snapshot.exchangeStatus).map(([id, status]) => (
            <StatusBadge key={id} $status={status} title={`${id}: ${status}`}>
              {id.slice(0, 8)} · {status === 'ok' ? '✓' : status === 'no_instrument' ? '—' : '✗'}
            </StatusBadge>
          ))}
        </div>
      </Header>

      <BookArea>
        {/* Asks — reversed so lowest ask is at bottom (closest to spread) */}
        <HalfBook style={{ display: 'flex', flexDirection: 'column' }}>
          <BookTable>
            <BookTHead>
              <tr>
                <th>Price</th>
                <th>Total Size</th>
                <th>Exchanges</th>
              </tr>
            </BookTHead>
            <tbody>
              {[...snapshot.asks].reverse().map((level, i) => (
                <LevelRow
                  key={i}
                  level={level}
                  side="ask"
                  hoveredExchange={hoveredExchange}
                  onHoverExchange={setHoveredExchange}
                  onClickPrice={onClickPrice}
                />
              ))}
            </tbody>
          </BookTable>
        </HalfBook>

        <SpreadRow>
          {bestBid > 0 && bestAsk > 0 ? (
            <>
              <span>Spread: <strong style={{ color: '#c8d6e5' }}>{fmtPrice(spread)}</strong></span>
              <span>({spreadPct.toFixed(4)}%)</span>
              <span style={{ marginLeft: 'auto', color: '#3a4555', fontSize: '0.68rem' }}>
                {new Date(snapshot.timestamp).toLocaleTimeString()}
              </span>
            </>
          ) : (
            <span>No data</span>
          )}
        </SpreadRow>

        {/* Bids */}
        <HalfBook>
          <BookTable>
            <tbody>
              {snapshot.bids.map((level, i) => (
                <LevelRow
                  key={i}
                  level={level}
                  side="bid"
                  hoveredExchange={hoveredExchange}
                  onHoverExchange={setHoveredExchange}
                  onClickPrice={onClickPrice}
                />
              ))}
            </tbody>
          </BookTable>
        </HalfBook>
      </BookArea>
    </>
  );
};

// ── Main component ─────────────────────────────────────────────────────────

const AggBook: FunctionComponent = () => {
  const configs   = useAppSelector(selectAggBookConfigs);
  const snapshots = useAppSelector(selectAggBookSnapshots);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [prefilledPrice, setPrefilledPrice] = useState<number | null>(null);

  const effectiveId = selectedId ?? configs[0]?.id ?? null;
  const snapshot    = effectiveId ? snapshots[effectiveId] : null;
  const config      = effectiveId ? configs.find(c => c.id === effectiveId) : null;

  return (
    <Wrapper>
      <Sidebar>
        <SidebarTitle>Agg Books</SidebarTitle>
        {configs.length === 0 && (
          <div style={{ padding: '1rem', fontSize: '0.75rem', color: '#4a5568' }}>
            No configs yet — create one in Settings → Agg Book.
          </div>
        )}
        {configs.map(cfg => (
          <ConfigItem
            key={cfg.id}
            $active={cfg.id === effectiveId}
            onClick={() => setSelectedId(cfg.id)}
          >
            <div className="name">{cfg.name}</div>
            <div className="meta">{cfg.baseSymbol} · {cfg.instrumentKind}</div>
            {!cfg.active && <div style={{ fontSize: '0.65rem', color: '#d0616e' }}>inactive</div>}
          </ConfigItem>
        ))}
      </Sidebar>

      <MainArea>
        {!effectiveId || !config ? (
          <EmptyState>Select or create an Agg Book config in Settings.</EmptyState>
        ) : !snapshot ? (
          <>
            <Header>
              <HeaderTitle>{config.name}</HeaderTitle>
              <KindBadge>{config.instrumentKind}</KindBadge>
              <span style={{ color: '#7e8b99', fontSize: '0.78rem' }}>{config.baseSymbol}</span>
            </Header>
            <EmptyState>Waiting for data…</EmptyState>
          </>
        ) : (
          <SnapshotView snapshot={snapshot} onClickPrice={setPrefilledPrice} />
        )}
      </MainArea>

      {config && (
        <AggOrderForm
          config={config}
          snapshot={snapshot ?? null}
          prefilledPrice={prefilledPrice}
          onClearPrice={() => setPrefilledPrice(null)}
        />
      )}
    </Wrapper>
  );
};

export default AggBook;
