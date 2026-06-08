import { FunctionComponent, useState, useEffect, useRef, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import styled from 'styled-components';
import { useAppSelector } from '../../hooks';
import { selectAccounts, selectGeneral, Account } from '../Settings/settingsSlice';

// ── Types ──────────────────────────────────────────────────────────────────

interface AccountSummary {
  currency: string;
  equity: number;
  availableFunds: number;
  initialMargin: number;
  maintenanceMargin: number;
  unrealizedPl: number;
}

interface Position {
  instrumentName: string;
  direction: string;
  size: number;
  averagePrice: number;
  markPrice: number;
  markIv: number;
  unrealizedPnl: number;
  delta: number;
  gamma: number;
  theta: number;
  vega: number;
}

interface NetGreeks {
  delta: number;
  gamma: number;
  theta: number;
  vega: number;
}

interface AccountRow {
  account: Account;
  summary: AccountSummary | null;
  positions: Position[];
  loading: boolean;
  posLoading: boolean;
  error: string | null;
  showPositions: boolean;
}

// ── Styles ─────────────────────────────────────────────────────────────────

const EXCHANGE_COLORS: Record<string, string> = {
  deribit:  '#5087f2',
  okx:      '#e0b94a',
  bybit:    '#f7a600',
  coincall: '#33b48f',
};

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

const Title = styled.div`
  font-size: 0.9rem;
  font-weight: 600;
  color: #e8edf4;
`;

const Btn = styled.button<{ $variant?: 'primary' | 'ghost' }>`
  padding: 0.25rem 0.7rem;
  border-radius: 3px;
  border: 1px solid ${p => p.$variant === 'primary' ? '#3a5a8c' : '#1e2738'};
  background: ${p => p.$variant === 'primary' ? '#1e3558' : 'transparent'};
  color: ${p => p.$variant === 'primary' ? '#7eb8f7' : '#7e8b99'};
  font-size: 0.82rem;
  cursor: pointer;
  transition: all 0.12s;
  &:hover { opacity: 0.85; }
  &:disabled { opacity: 0.4; cursor: not-allowed; }
`;

const Grid = styled.div`
  flex: 1;
  overflow-y: auto;
  padding: 0.75rem;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(380px, 1fr));
  gap: 0.75rem;
  align-content: start;
  &::-webkit-scrollbar { width: 4px; }
  &::-webkit-scrollbar-thumb { background: #2a3a52; border-radius: 2px; }
`;

const Card = styled.div`
  background: #131c2e;
  border: 1px solid #1e2738;
  border-radius: 6px;
  overflow: hidden;
`;

const CardHeader = styled.div<{ $exchange: string }>`
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.55rem 0.75rem;
  background: #0f1522;
  border-bottom: 1px solid #1e2738;
  border-top: 2px solid ${p => EXCHANGE_COLORS[p.$exchange] ?? '#5087f2'};
`;

const ExBadge = styled.span<{ $exchange: string }>`
  font-size: 0.7rem;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: ${p => EXCHANGE_COLORS[p.$exchange] ?? '#5087f2'};
  background: ${p => EXCHANGE_COLORS[p.$exchange] ?? '#5087f2'}22;
  border: 1px solid ${p => EXCHANGE_COLORS[p.$exchange] ?? '#5087f2'}44;
  padding: 0.1rem 0.4rem;
  border-radius: 3px;
`;

const AccountName = styled.div`
  font-size: 0.85rem;
  font-weight: 600;
  color: #e8edf4;
  margin-left: 0.5rem;
  flex: 1;
`;

const TestnetBadge = styled.span`
  font-size: 0.68rem;
  color: #e0b94a;
  border: 1px solid #e0b94a44;
  padding: 0.1rem 0.35rem;
  border-radius: 3px;
`;

const CardBody = styled.div`
  padding: 0.75rem;
`;

const MetricGrid = styled.div`
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  gap: 0.5rem;
`;

const Metric = styled.div<{ $highlight?: boolean }>`
  background: #0d1117;
  border: 1px solid #1e2738;
  border-radius: 4px;
  padding: 0.5rem 0.6rem;
`;

const MetricLabel = styled.div`
  font-size: 0.68rem;
  color: #4a5568;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  margin-bottom: 0.2rem;
`;

const MetricValue = styled.div<{ $color?: string }>`
  font-size: 0.92rem;
  font-weight: 600;
  color: ${p => p.$color ?? '#e8edf4'};
  font-variant-numeric: tabular-nums;
`;

const MetricSub = styled.div`
  font-size: 0.7rem;
  color: #4a5568;
  margin-top: 0.1rem;
`;

const Divider = styled.div`
  height: 1px;
  background: #1e2738;
  margin: 0.6rem 0;
`;

const MarginBar = styled.div`
  margin-top: 0.5rem;
`;

const BarLabel = styled.div`
  display: flex;
  justify-content: space-between;
  font-size: 0.72rem;
  color: #7e8b99;
  margin-bottom: 0.2rem;
`;

const BarTrack = styled.div`
  height: 6px;
  background: #1e2738;
  border-radius: 3px;
  overflow: hidden;
`;

const BarFill = styled.div<{ $pct: number; $color: string }>`
  height: 100%;
  width: ${p => Math.min(p.$pct, 100)}%;
  background: ${p => p.$color};
  border-radius: 3px;
  transition: width 0.4s ease;
`;

const LoadingBlock = styled.div`
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 1.5rem;
  color: #4a5568;
  font-size: 0.82rem;
  gap: 0.5rem;
`;

const ErrorBlock = styled.div`
  color: #e05252;
  font-size: 0.78rem;
  padding: 0.75rem;
  background: #1a0e0e;
  border: 1px solid #5a2a2a;
  border-radius: 4px;
  margin: 0.5rem;
`;

const CurrencySelect = styled.select`
  background: #0d1117;
  color: #7eb8f7;
  border: 1px solid #1e2738;
  border-radius: 3px;
  padding: 0.15rem 0.3rem;
  font-size: 0.75rem;
  outline: none;
`;

const Empty = styled.div`
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: #4a5568;
  gap: 0.5rem;
  font-size: 0.9rem;
  strong { color: #7e8b99; }
  span { font-size: 0.8rem; }
`;

// ── Positions + Greeks styles ───────────────────────────────────────────────

const GreeksRow = styled.div`
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 0.4rem;
  margin-top: 0.5rem;
`;

const GreekBox = styled.div`
  background: #0a0f1a;
  border: 1px solid #1e2738;
  border-radius: 4px;
  padding: 0.35rem 0.5rem;
  text-align: center;
`;

const GreekLabel = styled.div`
  font-size: 0.62rem;
  color: #4a5568;
  text-transform: uppercase;
  letter-spacing: 0.06em;
`;

const GreekValue = styled.div<{ $color?: string }>`
  font-size: 0.85rem;
  font-weight: 600;
  color: ${p => p.$color ?? '#e8edf4'};
  font-variant-numeric: tabular-nums;
`;

const SectionTitle = styled.div`
  font-size: 0.72rem;
  font-weight: 600;
  color: #4a5568;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  margin: 0.6rem 0 0.3rem;
  display: flex;
  align-items: center;
  justify-content: space-between;
`;

const ToggleBtn = styled.button`
  background: none;
  border: none;
  color: #5087f2;
  font-size: 0.72rem;
  cursor: pointer;
  padding: 0;
  &:hover { text-decoration: underline; }
`;

const PosTable = styled.table`
  width: 100%;
  border-collapse: collapse;
  font-size: 0.7rem;
  margin-top: 0.25rem;
`;

const PTh = styled.th`
  text-align: left;
  padding: 4px 6px;
  background: #0a0f1a;
  color: #4a5568;
  font-weight: 500;
  border-bottom: 1px solid #1e2738;
  white-space: nowrap;
`;

const PTd = styled.td`
  padding: 4px 6px;
  border-bottom: 1px solid #151d2e;
  color: #c9d1db;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
`;

const DirTag = styled.span<{ $dir: string }>`
  color: ${p => p.$dir === 'long' || p.$dir === 'buy' ? '#4ade80' : '#e05252'};
  font-weight: 600;
`;

// ── Helpers ────────────────────────────────────────────────────────────────

const CURRENCIES_BY_EXCHANGE: Record<string, string[]> = {
  deribit:  ['BTC', 'ETH', 'USDC', 'USDT'],
  okx:      ['BTC', 'ETH', 'USDT', 'USDC'],
  bybit:    ['USD', 'USDT', 'BTC', 'ETH', 'USDC'],  // USD = account-level totals (UNIFIED)
  coincall: ['USDT', 'BTC', 'ETH'],
};

function fmtNum(n: number, decimals = 2): string {
  if (Math.abs(n) >= 1_000_000) return (n / 1_000_000).toFixed(2) + 'M';
  if (Math.abs(n) >= 1_000) return (n / 1_000).toFixed(2) + 'K';
  return n.toFixed(decimals);
}

function pnlColor(v: number): string {
  if (v > 0) return '#4ade80';
  if (v < 0) return '#e05252';
  return '#7e8b99';
}

function marginColor(pct: number): string {
  if (pct >= 80) return '#e05252';
  if (pct >= 50) return '#e0b94a';
  return '#5087f2';
}

// ── Account Card ───────────────────────────────────────────────────────────

interface AccountCardProps {
  row: AccountRow;
  currency: string;
  availableCurrencies: string[];
  onCurrencyChange: (id: string, c: string) => void;
  onRefresh: (id: string) => void;
  onTogglePositions: (id: string) => void;
}

function computeGreeks(positions: Position[]): NetGreeks {
  return positions.reduce(
    (acc, p) => ({
      delta: acc.delta + p.delta,
      gamma: acc.gamma + p.gamma,
      theta: acc.theta + p.theta,
      vega:  acc.vega  + p.vega,
    }),
    { delta: 0, gamma: 0, theta: 0, vega: 0 }
  );
}

function fmtGreek(n: number): string {
  if (n === 0) return '0.00';
  return n.toFixed(4);
}

const AccountCard: FunctionComponent<AccountCardProps> = ({
  row, currency, availableCurrencies, onCurrencyChange, onRefresh, onTogglePositions,
}) => {
  const { account, summary, positions, loading, posLoading, error, showPositions } = row;
  const imRatio = summary ? (summary.equity > 0 ? (summary.initialMargin / summary.equity) * 100 : 0) : 0;
  const mmRatio = summary ? (summary.equity > 0 ? (summary.maintenanceMargin / summary.equity) * 100 : 0) : 0;
  const greeks = computeGreeks(positions);

  return (
    <Card>
      <CardHeader $exchange={account.exchange}>
        <ExBadge $exchange={account.exchange}>{account.exchange}</ExBadge>
        <AccountName>{account.name}</AccountName>
        {account.testnet && <TestnetBadge>testnet</TestnetBadge>}
        <CurrencySelect
          value={currency}
          onChange={e => onCurrencyChange(account.id, e.target.value)}
          style={{ marginLeft: '0.5rem' }}
        >
          {availableCurrencies.map(c => <option key={c} value={c}>{c}</option>)}
        </CurrencySelect>
        <Btn onClick={() => onRefresh(account.id)} style={{ marginLeft: '0.4rem' }} disabled={loading}>
          {loading ? '⟳' : '↺'}
        </Btn>
      </CardHeader>

      <CardBody>
        {loading && <LoadingBlock>⟳ Loading…</LoadingBlock>}
        {error && !loading && <ErrorBlock>⚠ {error}</ErrorBlock>}
        {summary && !loading && (
          <>
            <MetricGrid>
              <Metric>
                <MetricLabel>Equity</MetricLabel>
                <MetricValue>{fmtNum(summary.equity)}</MetricValue>
                <MetricSub>{currency}</MetricSub>
              </Metric>
              <Metric>
                <MetricLabel>Available</MetricLabel>
                <MetricValue $color="#7eb8f7">{fmtNum(summary.availableFunds)}</MetricValue>
                <MetricSub>{currency}</MetricSub>
              </Metric>
              <Metric>
                <MetricLabel>Unreal. P&L</MetricLabel>
                <MetricValue $color={pnlColor(summary.unrealizedPl)}>
                  {summary.unrealizedPl >= 0 ? '+' : ''}{fmtNum(summary.unrealizedPl)}
                </MetricValue>
                <MetricSub>{currency}</MetricSub>
              </Metric>
            </MetricGrid>

            <Divider />

            <MetricGrid>
              <Metric>
                <MetricLabel>Init. Margin</MetricLabel>
                <MetricValue $color={marginColor(imRatio)}>{fmtNum(summary.initialMargin)}</MetricValue>
                <MetricSub>{imRatio.toFixed(1)}% of equity</MetricSub>
              </Metric>
              <Metric>
                <MetricLabel>Maint. Margin</MetricLabel>
                <MetricValue $color={marginColor(mmRatio)}>{fmtNum(summary.maintenanceMargin)}</MetricValue>
                <MetricSub>{mmRatio.toFixed(1)}% of equity</MetricSub>
              </Metric>
              <Metric>
                <MetricLabel>Margin Ratio</MetricLabel>
                <MetricValue $color={marginColor(imRatio)}>
                  {imRatio.toFixed(1)}%
                </MetricValue>
                <MetricSub>IM / Equity</MetricSub>
              </Metric>
            </MetricGrid>

            {summary.equity > 0 && (
              <MarginBar>
                <BarLabel>
                  <span>Margin utilisation</span>
                  <span style={{ color: marginColor(imRatio) }}>{imRatio.toFixed(1)}%</span>
                </BarLabel>
                <BarTrack>
                  <BarFill $pct={mmRatio} $color="#e05252" />
                </BarTrack>
                <BarTrack style={{ marginTop: 2 }}>
                  <BarFill $pct={imRatio} $color={marginColor(imRatio)} />
                </BarTrack>
                <BarLabel style={{ marginTop: '0.15rem' }}>
                  <span style={{ color: '#e05252' }}>▬ Maint {mmRatio.toFixed(1)}%</span>
                  <span style={{ color: marginColor(imRatio) }}>▬ Init {imRatio.toFixed(1)}%</span>
                </BarLabel>
              </MarginBar>
            )}

            {/* ── Net Greeks ───────────────────────────── */}
            <SectionTitle>
              <span>Net Greeks</span>
              {posLoading && <span style={{ color: '#4a5568', fontWeight: 400 }}>loading…</span>}
            </SectionTitle>
            <GreeksRow>
              <GreekBox>
                <GreekLabel>Delta</GreekLabel>
                <GreekValue $color={greeks.delta > 0 ? '#4ade80' : greeks.delta < 0 ? '#e05252' : '#7e8b99'}>
                  {greeks.delta >= 0 ? '+' : ''}{fmtGreek(greeks.delta)}
                </GreekValue>
              </GreekBox>
              <GreekBox>
                <GreekLabel>Gamma</GreekLabel>
                <GreekValue>{fmtGreek(greeks.gamma)}</GreekValue>
              </GreekBox>
              <GreekBox>
                <GreekLabel>Theta</GreekLabel>
                <GreekValue $color={greeks.theta < 0 ? '#e0b94a' : '#e8edf4'}>
                  {fmtGreek(greeks.theta)}
                </GreekValue>
              </GreekBox>
              <GreekBox>
                <GreekLabel>Vega</GreekLabel>
                <GreekValue>{fmtGreek(greeks.vega)}</GreekValue>
              </GreekBox>
            </GreeksRow>

            {/* ── Positions ────────────────────────────── */}
            <SectionTitle>
              <span>Positions ({positions.length})</span>
              <ToggleBtn onClick={() => onTogglePositions(account.id)}>
                {showPositions ? 'Hide' : 'Show'}
              </ToggleBtn>
            </SectionTitle>

            {showPositions && (
              positions.length === 0 ? (
                <div style={{ fontSize: '0.75rem', color: '#4a5568', padding: '0.4rem 0' }}>
                  No open positions
                </div>
              ) : (
                <div style={{ overflowX: 'auto', maxHeight: '220px', overflowY: 'auto' }}>
                  <PosTable>
                    <thead>
                      <tr>
                        <PTh>Instrument</PTh>
                        <PTh>Dir</PTh>
                        <PTh>Size</PTh>
                        <PTh>Avg Px</PTh>
                        <PTh>Mark Px</PTh>
                        <PTh>IV%</PTh>
                        <PTh>UPnL</PTh>
                        <PTh>Δ</PTh>
                        <PTh>Γ</PTh>
                        <PTh>Θ</PTh>
                        <PTh>V</PTh>
                      </tr>
                    </thead>
                    <tbody>
                      {positions.map((p, i) => (
                        <tr key={i}>
                          <PTd style={{ fontFamily: 'monospace' }}>{p.instrumentName}</PTd>
                          <PTd><DirTag $dir={p.direction}>{p.direction.toUpperCase()}</DirTag></PTd>
                          <PTd>{p.size}</PTd>
                          <PTd>{p.averagePrice.toFixed(2)}</PTd>
                          <PTd>{p.markPrice.toFixed(2)}</PTd>
                          <PTd>{p.markIv > 0 ? (p.markIv * 100).toFixed(1) + '%' : '-'}</PTd>
                          <PTd style={{ color: p.unrealizedPnl >= 0 ? '#4ade80' : '#e05252' }}>
                            {p.unrealizedPnl >= 0 ? '+' : ''}{p.unrealizedPnl.toFixed(4)}
                          </PTd>
                          <PTd>{p.delta.toFixed(4)}</PTd>
                          <PTd>{p.gamma.toFixed(6)}</PTd>
                          <PTd>{p.theta.toFixed(4)}</PTd>
                          <PTd>{p.vega.toFixed(4)}</PTd>
                        </tr>
                      ))}
                    </tbody>
                  </PosTable>
                </div>
              )
            )}
          </>
        )}
        {!summary && !loading && !error && (
          <LoadingBlock style={{ color: '#4a5568' }}>No data — click ↺ to load</LoadingBlock>
        )}
      </CardBody>
    </Card>
  );
};

// ── Main Component ──────────────────────────────────────────────────────────

const AccountSummaryPanel: FunctionComponent = () => {
  const accounts = useAppSelector(selectAccounts);
  const general  = useAppSelector(selectGeneral);
  const [rows, setRows]           = useState<AccountRow[]>([]);
  const [currencies, setCurrencies] = useState<Record<string, string>>({});
  const [allLoading, setAllLoading] = useState(false);

  const watchedSet = useMemo<Set<string>>(() => {
    if (!general.watchedCoins) return new Set();
    return new Set(general.watchedCoins.split(',').map(s => s.trim()).filter(Boolean));
  }, [general.watchedCoins]);

  const getCurrenciesFor = (exchange: string): string[] => {
    const all = CURRENCIES_BY_EXCHANGE[exchange] ?? ['USDT'];
    if (watchedSet.size === 0) return all;
    const filtered = all.filter(c => watchedSet.has(c));
    return filtered.length > 0 ? filtered : all;
  };

  useEffect(() => {
    setRows(accounts.map(a => ({
      account: a, summary: null, positions: [],
      loading: false, posLoading: false, error: null, showPositions: false,
    })));
    setCurrencies(prev => {
      const next = { ...prev };
      for (const a of accounts) {
        if (!next[a.id]) next[a.id] = getCurrenciesFor(a.exchange)[0];
      }
      return next;
    });
  }, [accounts]); // eslint-disable-line react-hooks/exhaustive-deps

  const loadPositions = async (accountId: string, currency: string) => {
    setRows(prev => prev.map(r =>
      r.account.id === accountId ? { ...r, posLoading: true } : r
    ));
    try {
      const positions = await invoke<Position[]>('get_positions', { accountId, currency });
      setRows(prev => prev.map(r =>
        r.account.id === accountId ? { ...r, positions, posLoading: false } : r
      ));
    } catch {
      setRows(prev => prev.map(r =>
        r.account.id === accountId ? { ...r, posLoading: false } : r
      ));
    }
  };

  const loadOne = async (accountId: string, currency: string) => {
    setRows(prev => prev.map(r =>
      r.account.id === accountId ? { ...r, loading: true, error: null } : r
    ));
    try {
      const summary = await invoke<AccountSummary>('get_account_summary', { accountId, currency });
      setRows(prev => prev.map(r =>
        r.account.id === accountId ? { ...r, summary, loading: false } : r
      ));
    } catch (e: any) {
      setRows(prev => prev.map(r =>
        r.account.id === accountId ? { ...r, loading: false, error: String(e) } : r
      ));
    }
    // Load positions in parallel
    loadPositions(accountId, currency);
  };

  const loadAll = async (currMap: Record<string, string>) => {
    if (accounts.length === 0) return;
    setAllLoading(true);
    await Promise.allSettled(accounts.map(a => loadOne(a.id, currMap[a.id] ?? getCurrenciesFor(a.exchange)[0])));
    setAllLoading(false);
  };

  const didAutoLoad = useRef(false);
  useEffect(() => {
    if (accounts.length === 0 || Object.keys(currencies).length === 0) return;
    if (didAutoLoad.current) return;
    didAutoLoad.current = true;
    loadAll(currencies);
  }, [accounts, currencies]); // eslint-disable-line react-hooks/exhaustive-deps

  const handleCurrencyChange = (accountId: string, currency: string) => {
    setCurrencies(prev => {
      const next = { ...prev, [accountId]: currency };
      loadOne(accountId, currency);
      return next;
    });
  };

  const handleRefreshOne = (accountId: string) => {
    const acct = accounts.find(a => a.id === accountId);
    loadOne(accountId, currencies[accountId] ?? getCurrenciesFor(acct?.exchange ?? '')[0]);
  };

  const handleTogglePositions = (accountId: string) => {
    setRows(prev => prev.map(r =>
      r.account.id === accountId ? { ...r, showPositions: !r.showPositions } : r
    ));
  };

  if (accounts.length === 0) {
    return (
      <Wrapper>
        <Empty style={{ margin: 'auto' }}>
          <strong>No exchange accounts configured</strong>
          <span>Go to ⚙ Settings → Exchange to add an account</span>
        </Empty>
      </Wrapper>
    );
  }

  return (
    <Wrapper>
      <TopBar>
        <Title>Account Summary</Title>
        <Btn $variant="primary" onClick={() => loadAll(currencies)} disabled={allLoading}>
          {allLoading ? '⟳ Refreshing…' : '↺ Refresh All'}
        </Btn>
        <span style={{ marginLeft: 'auto', fontSize: '0.75rem', color: '#4a5568' }}>
          {accounts.length} account{accounts.length !== 1 ? 's' : ''}
        </span>
      </TopBar>

      <Grid>
        {rows.map(row => (
          <AccountCard
            key={row.account.id}
            row={row}
            currency={currencies[row.account.id] ?? 'USDT'}
            availableCurrencies={getCurrenciesFor(row.account.exchange)}
            onCurrencyChange={handleCurrencyChange}
            onRefresh={handleRefreshOne}
            onTogglePositions={handleTogglePositions}
          />
        ))}
      </Grid>
    </Wrapper>
  );
};

export default AccountSummaryPanel;
