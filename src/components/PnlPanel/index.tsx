import { useState, useCallback, useMemo } from 'react';
import styled from 'styled-components';
import { invoke } from '@tauri-apps/api/core';
import { useAppSelector } from '../../hooks';

// ── Types ───────────────────────────────────────────────────────────────────

interface TransactionLog {
  id: string;
  timestamp: number;
  instrumentName: string;
  transactionType: string;
  category: string;
  side: string;
  amount: number;
  price: number;
  fee: number;
  feeCurrency: string;
  currency: string;
  profitAsCashflow: number;
  balance: number;
  change: number;
  tradeId: string;
  orderId: string;
  info: string;
  markPrice: number;
  indexPrice: number;
  equity: number;
  /** Position size after this entry (Bybit: size, Deribit: position) */
  position: number;
  baseCurrency: string;
  quoteCurrency: string;
  /** Bybit: funding fee in this row (separate from fee); others: 0 */
  funding: number;
}

// ── Styled ───────────────────────────────────────────────────────────────────

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
  padding: 10px 16px;
  background: #161b22;
  border-bottom: 1px solid #30363d;
  flex-shrink: 0;
  flex-wrap: wrap;
`;

const Label = styled.span`
  font-size: 12px;
  color: #8b949e;
  white-space: nowrap;
`;

const Select = styled.select`
  background: #21262d;
  color: #e6edf3;
  border: 1px solid #30363d;
  border-radius: 4px;
  padding: 5px 8px;
  font-size: 13px;
  cursor: pointer;
`;

const DateInput = styled.input`
  background: #21262d;
  color: #e6edf3;
  border: 1px solid #30363d;
  border-radius: 4px;
  padding: 5px 8px;
  font-size: 13px;
`;

const FetchBtn = styled.button`
  padding: 6px 14px;
  background: #238636;
  color: #fff;
  border: none;
  border-radius: 4px;
  font-size: 13px;
  cursor: pointer;
  white-space: nowrap;
  &:hover { background: #2ea043; }
  &:disabled { background: #21262d; color: #8b949e; cursor: not-allowed; }
`;

const ErrorMsg = styled.div`
  padding: 8px 16px;
  background: #3d1c1c;
  color: #f85149;
  font-size: 12px;
  flex-shrink: 0;
`;

const SummaryBar = styled.div`
  display: flex;
  gap: 20px;
  padding: 8px 16px;
  background: #161b22;
  border-bottom: 1px solid #30363d;
  flex-shrink: 0;
  font-size: 12px;
  flex-wrap: wrap;
`;

const StatLabel = styled.span`color: #8b949e;`;
const StatValue = styled.span<{ $positive?: boolean; $negative?: boolean }>`
  color: ${p => p.$positive ? '#3fb950' : p.$negative ? '#f85149' : '#e6edf3'};
  font-weight: 600;
  margin-left: 4px;
`;

const PagerBar = styled.div`
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 16px;
  background: #161b22;
  border-top: 1px solid #30363d;
  flex-shrink: 0;
  font-size: 12px;
`;

const PageBtn = styled.button<{ $active?: boolean }>`
  padding: 4px 10px;
  background: ${p => p.$active ? '#58a6ff' : '#21262d'};
  color: ${p => p.$active ? '#0d1117' : '#e6edf3'};
  border: 1px solid #30363d;
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  &:hover:not(:disabled) { background: #30363d; }
  &:disabled { opacity: 0.4; cursor: not-allowed; }
`;

const Body = styled.div`
  flex: 1;
  overflow: auto;
  padding: 0 16px 4px;
`;

const Table = styled.table`
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
  margin-top: 4px;
`;

const Th = styled.th`
  text-align: right;
  padding: 7px 8px;
  background: #161b22;
  color: #8b949e;
  font-weight: 500;
  border-bottom: 1px solid #30363d;
  position: sticky;
  top: 0;
  z-index: 1;
  white-space: nowrap;
  &:first-child { text-align: left; }
  &:nth-child(2) { text-align: left; }
  &:nth-child(3) { text-align: left; }
`;

const Td = styled.td<{ $align?: string; $color?: string }>`
  padding: 5px 8px;
  border-bottom: 1px solid #161b22;
  color: ${p => p.$color ?? '#e6edf3'};
  text-align: ${p => p.$align ?? 'right'};
  white-space: nowrap;
`;

const Empty = styled.div`
  text-align: center;
  color: #8b949e;
  padding: 48px;
  font-size: 14px;
`;

// ── Helpers ──────────────────────────────────────────────────────────────────

const SUPPORTED = ['deribit', 'bybit', 'coincall', 'bullish'];

const TYPE_LABELS: Record<string, string> = {
  trade: 'Trade', delivery: 'Delivery', settlement: 'Settlement',
  transfer_in: 'Transfer In', transfer_out: 'Transfer Out',
  deposit: 'Deposit', withdrawal: 'Withdrawal',
  fee: 'Fee', funding: 'Funding', option_exercise: 'Exercise', other: 'Other',
};

const PAGE_SIZES = [50, 100, 200, 500];

function fmt(n: number, decimals = 4): string {
  if (n === 0) return '-';
  return n.toLocaleString(undefined, { minimumFractionDigits: 0, maximumFractionDigits: decimals });
}

function fmtTs(ms: number): string {
  if (!ms) return '-';
  return new Date(ms).toLocaleString();
}

function toISODate(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10);
}

function fromISODate(s: string): number {
  return new Date(s + 'T00:00:00Z').getTime();
}

// ── Component ─────────────────────────────────────────────────────────────────

const PnlPanel: React.FC = () => {
  const accounts = useAppSelector(s => s.settings.accounts);
  const supported = accounts.filter(a => SUPPORTED.includes(a.exchange));

  const now = Date.now();
  const thirtyDaysAgo = now - 30 * 24 * 60 * 60 * 1000;

  const [accountId, setAccountId] = useState<string>(supported[0]?.id ?? '');
  const [startDate, setStartDate] = useState<string>(toISODate(thirtyDaysAgo));
  const [endDate, setEndDate]     = useState<string>(toISODate(now));
  const [allLogs, setAllLogs]     = useState<TransactionLog[]>([]);
  const [loading, setLoading]     = useState(false);
  const [error, setError]         = useState<string | null>(null);
  const [currency, setCurrency]   = useState<string>('ALL');
  const [filterBase, setFilterBase]   = useState<string>('ALL');
  const [filterQuote, setFilterQuote] = useState<string>('ALL');
  const [pageSize, setPageSize]   = useState<number>(100);
  const [page, setPage]           = useState<number>(0);

  const fetchLogs = useCallback(async () => {
    if (!accountId) return;
    setLoading(true);
    setError(null);
    setPage(0);
    try {
      const startMs = fromISODate(startDate);
      const endMs   = fromISODate(endDate) + 86400000 - 1;
      const result = await invoke<TransactionLog[]>('get_transaction_log', {
        accountId, startMs, endMs,
      });
      setAllLogs(result);
      setCurrency('ALL');
      setFilterBase('ALL');
      setFilterQuote('ALL');
    } catch (e: any) {
      setError(String(e));
      setAllLogs([]);
    } finally {
      setLoading(false);
    }
  }, [accountId, startDate, endDate]);

  // Unique currencies from loaded data
  const currencies = useMemo(() => {
    const seen = new Set<string>();
    allLogs.forEach(l => { if (l.currency) seen.add(l.currency); });
    return ['ALL', ...Array.from(seen).sort()];
  }, [allLogs]);

  const bases = useMemo(() => {
    const seen = new Set<string>();
    allLogs.forEach(l => { if (l.baseCurrency) seen.add(l.baseCurrency); });
    return ['ALL', ...Array.from(seen).sort()];
  }, [allLogs]);

  const quotes = useMemo(() => {
    const seen = new Set<string>();
    allLogs.forEach(l => { if (l.quoteCurrency) seen.add(l.quoteCurrency); });
    return ['ALL', ...Array.from(seen).sort()];
  }, [allLogs]);

  // Filtered logs
  const filtered = useMemo(() => allLogs.filter(l =>
    (currency    === 'ALL' || l.currency     === currency) &&
    (filterBase  === 'ALL' || l.baseCurrency === filterBase) &&
    (filterQuote === 'ALL' || l.quoteCurrency === filterQuote)
  ), [allLogs, currency, filterBase, filterQuote]);

  // Summary over filtered (all pages)
  const totalPnl  = filtered.reduce((s, l) => s + l.profitAsCashflow, 0);
  const totalFees = filtered.reduce((s, l) => s + l.fee, 0);
  const trades    = filtered.filter(l => l.transactionType === 'trade').length;
  // Most recent row (sorted newest-first) carries the current equity seed
  const currentEquity = filtered.length > 0 ? filtered[0].equity : 0;
  const currentBalance = filtered.length > 0 ? filtered[0].balance : 0;

  // Pagination
  const totalPages = Math.max(1, Math.ceil(filtered.length / pageSize));
  const safePage   = Math.min(page, totalPages - 1);
  const pageData   = filtered.slice(safePage * pageSize, (safePage + 1) * pageSize);

  const goPage = (n: number) => setPage(Math.max(0, Math.min(n, totalPages - 1)));

  return (
    <Wrap>
      <Toolbar>
        <Label>Account</Label>
        <Select value={accountId} onChange={e => setAccountId(e.target.value)}>
          {supported.map(a => (
            <option key={a.id} value={a.id}>{a.name} ({a.exchange})</option>
          ))}
        </Select>

        <Label>From</Label>
        <DateInput type="date" value={startDate} onChange={e => setStartDate(e.target.value)} />

        <Label>To</Label>
        <DateInput type="date" value={endDate} onChange={e => setEndDate(e.target.value)} />

        <FetchBtn onClick={fetchLogs} disabled={!accountId || loading}>
          {loading ? 'Loading…' : '🔍 Fetch'}
        </FetchBtn>

        {allLogs.length > 0 && (
          <>
            <Label style={{ marginLeft: 4 }}>Currency</Label>
            <Select value={currency} onChange={e => { setCurrency(e.target.value); setPage(0); }}>
              {currencies.map(c => <option key={c} value={c}>{c}</option>)}
            </Select>

            <Label>Base</Label>
            <Select value={filterBase} onChange={e => { setFilterBase(e.target.value); setPage(0); }}>
              {bases.map(b => <option key={b} value={b}>{b}</option>)}
            </Select>

            <Label>Quote</Label>
            <Select value={filterQuote} onChange={e => { setFilterQuote(e.target.value); setPage(0); }}>
              {quotes.map(q => <option key={q} value={q}>{q}</option>)}
            </Select>

            <Label>Show</Label>
            <Select value={pageSize} onChange={e => { setPageSize(Number(e.target.value)); setPage(0); }}>
              {PAGE_SIZES.map(s => <option key={s} value={s}>{s} / page</option>)}
            </Select>

            <Label style={{ color: '#e6edf3' }}>{filtered.length} entries</Label>
          </>
        )}
      </Toolbar>

      {error && <ErrorMsg>⚠ {error}</ErrorMsg>}

      {filtered.length > 0 && (
        <SummaryBar>
          <span>
            <StatLabel>Total PnL</StatLabel>
            <StatValue $positive={totalPnl > 0} $negative={totalPnl < 0}>{fmt(totalPnl, 6)}</StatValue>
          </span>
          <span>
            <StatLabel>Total Fees</StatLabel>
            <StatValue $negative={totalFees !== 0}>{fmt(totalFees, 6)}</StatValue>
          </span>
          <span>
            <StatLabel>Trades</StatLabel>
            <StatValue>{trades}</StatValue>
          </span>
          {currentEquity > 0 && (
            <span>
              <StatLabel>Equity</StatLabel>
              <StatValue $positive>{fmt(currentEquity, 4)}</StatValue>
            </span>
          )}
          {currentBalance > 0 && (
            <span>
              <StatLabel>Balance</StatLabel>
              <StatValue>{fmt(currentBalance, 4)}</StatValue>
            </span>
          )}
          <span>
            <StatLabel>Page</StatLabel>
            <StatValue>{safePage + 1} / {totalPages}</StatValue>
          </span>
        </SummaryBar>
      )}

      <Body>
        {allLogs.length === 0 && !loading && !error && (
          <Empty>Select an account and date range, then click Fetch.</Empty>
        )}
        {pageData.length > 0 && (
          <Table>
            <thead>
              <tr>
                <Th>Time</Th>
                <Th>Type</Th>
                <Th>Instrument</Th>
                <Th>Category</Th>
                <Th>Base</Th>
                <Th>Quote</Th>
                <Th>Side</Th>
                <Th>Amount</Th>
                <Th>Price</Th>
                <Th>Size</Th>
                <Th>Mark</Th>
                <Th>Index</Th>
                <Th>Fee</Th>
                <Th>Funding</Th>
                <Th>P&amp;L</Th>
                <Th>Change</Th>
                <Th>Balance</Th>
                <Th>Equity</Th>
                <Th>Ccy</Th>
              </tr>
            </thead>
            <tbody>
              {pageData.map((l, i) => {
                const pnlColor  = l.profitAsCashflow > 0 ? '#3fb950' : l.profitAsCashflow < 0 ? '#f85149' : undefined;
                const sideColor = l.side === 'buy' ? '#3fb950' : l.side === 'sell' ? '#f85149' : '#8b949e';
                return (
                  <tr key={l.id || i}>
                    <Td $align="left">{fmtTs(l.timestamp)}</Td>
                    <Td $align="left" $color="#8b949e">{TYPE_LABELS[l.transactionType] ?? l.transactionType}</Td>
                    <Td $align="left">{l.instrumentName || l.currency || '-'}</Td>
                    <Td $align="left" $color="#8b949e">{l.category || '-'}</Td>
                    <Td $align="left" $color="#8b949e">{l.baseCurrency || '-'}</Td>
                    <Td $align="left" $color="#8b949e">{l.quoteCurrency || '-'}</Td>
                    <Td $color={sideColor}>{l.side ? l.side.toUpperCase() : '-'}</Td>
                    <Td>{fmt(l.amount)}</Td>
                    <Td>{fmt(l.price)}</Td>
                    <Td>{l.position > 0 ? fmt(l.position, 4) : '-'}</Td>
                    <Td>{l.markPrice > 0 ? fmt(l.markPrice) : '-'}</Td>
                    <Td $color="#8b949e">{l.indexPrice > 0 ? fmt(l.indexPrice) : '-'}</Td>
                    <Td $color={l.fee !== 0 ? '#f85149' : undefined}>{fmt(l.fee, 6)}</Td>
                    <Td $color={l.funding !== 0 ? (l.funding > 0 ? '#3fb950' : '#f85149') : undefined}>{l.funding !== 0 ? fmt(l.funding, 6) : '-'}</Td>
                    <Td $color={pnlColor}>{fmt(l.profitAsCashflow, 6)}</Td>
                    <Td $color={l.change > 0 ? '#3fb950' : l.change < 0 ? '#f85149' : undefined}>{fmt(l.change, 6)}</Td>
                    <Td>{fmt(l.balance, 4)}</Td>
                    <Td $color={l.equity > 0 ? '#58a6ff' : undefined}>{l.equity > 0 ? fmt(l.equity, 4) : '-'}</Td>
                    <Td $color="#8b949e">{l.currency || l.feeCurrency}</Td>
                  </tr>
                );
              })}
            </tbody>
          </Table>
        )}
      </Body>

      {totalPages > 1 && (
        <PagerBar>
          <PageBtn disabled={safePage === 0} onClick={() => goPage(0)}>«</PageBtn>
          <PageBtn disabled={safePage === 0} onClick={() => goPage(safePage - 1)}>‹ Prev</PageBtn>
          <Label style={{ color: '#e6edf3' }}>{safePage + 1} / {totalPages}</Label>
          <PageBtn disabled={safePage >= totalPages - 1} onClick={() => goPage(safePage + 1)}>Next ›</PageBtn>
          <PageBtn disabled={safePage >= totalPages - 1} onClick={() => goPage(totalPages - 1)}>»</PageBtn>
        </PagerBar>
      )}
    </Wrap>
  );
};

export default PnlPanel;
