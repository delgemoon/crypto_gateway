import { FunctionComponent, useState, useRef, useEffect } from 'react';
import { createPortal } from 'react-dom';
import styled from 'styled-components';
import { useAppDispatch, useAppSelector } from '../../hooks';
import { selectAccounts } from '../Settings/settingsSlice';
import { selectGeneral } from '../Settings/settingsSlice';
import { selectWidgets, addWidget, removeWidget, updateWidgetAccount } from './dashboardSlice';
import ExchangePanel from './ExchangePanel';
import OrderForm from './OrderForm';
import OpenOrders from './OpenOrders';

// ── Styled ─────────────────────────────────────────────────────────────────

const Wrapper = styled.div`
  display: flex;
  height: 100%;
  overflow: hidden;
  background: #0d1117;
`;

const LeftArea = styled.div`
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  overflow: hidden;
`;

const Toolbar = styled.div`
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.3rem 0.5rem;
  background: #0f1522;
  border-bottom: 1px solid #1e2738;
  flex-shrink: 0;
`;

const Spacer = styled.div` flex: 1; `;

const WidgetCount = styled.span`
  color: #4a5568;
  font-size: 0.67rem;
`;

const AddBtn = styled.button`
  display: flex;
  align-items: center;
  gap: 0.3rem;
  padding: 0.22rem 0.6rem;
  background: #1e3a6e;
  color: #5087f2;
  border: 1px solid #2a4a8a;
  border-radius: 3px;
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
  white-space: nowrap;
  transition: opacity 0.15s;
  &:hover { opacity: 0.82; }
  &:disabled { opacity: 0.38; cursor: not-allowed; }
`;

const PanelsArea = styled.div`
  flex: 1;
  display: flex;
  gap: 6px;
  padding: 6px;
  overflow-x: auto;
  overflow-y: hidden;
  min-width: 0;
  &::-webkit-scrollbar { height: 4px; }
  &::-webkit-scrollbar-track { background: transparent; }
  &::-webkit-scrollbar-thumb { background: #2a3a52; border-radius: 2px; }
`;

const RightPane = styled.div`
  width: 300px;
  min-width: 300px;
  display: flex;
  flex-direction: column;
  border-left: 1px solid #1e2738;
  overflow: hidden;
  flex-shrink: 0;
`;

const OrderFormWrapper = styled.div`
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  min-height: 0;
`;

const OpenOrdersWrapper = styled.div`
  height: 220px;
  border-top: 1px solid #1e2738;
  overflow: hidden;
  flex-shrink: 0;
`;

const NoAccounts = styled.div`
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: #4a5568;
  gap: 0.75rem;
  font-size: 0.9rem;
  strong { color: #7e8b99; }
  span { font-size: 0.8rem; }
`;

const EXCHANGE_COLORS: Record<string, string> = {
  deribit:  '#5087f2',
  okx:      '#e0b94a',
  bybit:    '#f7a600',
  coincall: '#33b48f',
};

const ExBadge = styled.span<{ $exchange: string }>`
  padding: 0.08rem 0.3rem;
  border-radius: 3px;
  font-size: 0.64rem;
  font-weight: 700;
  text-transform: uppercase;
  color: ${p => EXCHANGE_COLORS[p.$exchange] ?? '#5087f2'};
  background: ${p => EXCHANGE_COLORS[p.$exchange] ?? '#5087f2'}22;
  flex-shrink: 0;
`;

const DropdownItem = styled.div`
  display: flex;
  align-items: center;
  gap: 0.4rem;
  padding: 0.3rem 0.6rem;
  cursor: pointer;
  font-size: 0.78rem;
  color: #e8edf4;
  &:hover { background: #1e2a3a; }
`;

// ── Component ───────────────────────────────────────────────────────────────

const Dashboard: FunctionComponent = () => {
  const dispatch   = useAppDispatch();
  const accounts   = useAppSelector(selectAccounts);
  const widgets    = useAppSelector(selectWidgets);
  const general    = useAppSelector(selectGeneral);
  const maxWidgets = general.maxDashboardWidgets ?? 4;

  const [dropOpen, setDropOpen] = useState(false);
  const [dropPos, setDropPos]   = useState({ top: 0, left: 0 });
  const addBtnRef = useRef<HTMLButtonElement>(null);

  // Unique exchanges that have at least one account
  const availableExchanges = [...new Set(accounts.map(a => a.exchange))];
  const atMax = widgets.length >= maxWidgets;

  const openDropdown = () => {
    if (addBtnRef.current) {
      const r = addBtnRef.current.getBoundingClientRect();
      setDropPos({ top: r.bottom + 4, left: r.left });
    }
    setDropOpen(true);
  };

  useEffect(() => {
    if (!dropOpen) return;
    const handler = (e: MouseEvent) => {
      if (!addBtnRef.current?.contains(e.target as Node)) setDropOpen(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [dropOpen]);

  const handleAdd = (exchange: string) => {
    const firstAccount = accounts.find(a => a.exchange === exchange);
    if (!firstAccount) return;
    dispatch(addWidget({ exchange, accountId: firstAccount.id, maxWidgets }));
    setDropOpen(false);
  };

  return (
    <Wrapper>
      <LeftArea>
        {/* Toolbar */}
        <Toolbar>
          <Spacer />
          <WidgetCount>{widgets.length}/{maxWidgets} widgets</WidgetCount>
          <AddBtn
            ref={addBtnRef}
            disabled={availableExchanges.length === 0}
            onClick={() => availableExchanges.length > 0 && (dropOpen ? setDropOpen(false) : openDropdown())}
          >
            + Add Widget
          </AddBtn>
        </Toolbar>

        {/* Panels */}
        <PanelsArea>
          {accounts.length === 0 ? (
            <NoAccounts>
              <strong>No exchange accounts configured</strong>
              <span>Go to ⚙ Settings → Exchange to add an account</span>
            </NoAccounts>
          ) : widgets.length === 0 ? (
            <NoAccounts>
              <strong>No widgets visible</strong>
              <span>Click "+ Add Widget" to display an orderbook</span>
            </NoAccounts>
          ) : (
            widgets.map(widget => {
              const account = accounts.find(a => a.id === widget.accountId);
              if (!account) return null;
              const exchangeAccounts = accounts.filter(a => a.exchange === widget.exchange);
              return (
                <ExchangePanel
                  key={widget.id}
                  widgetId={widget.id}
                  account={account}
                  exchangeAccounts={exchangeAccounts}
                  onRemove={() => dispatch(removeWidget(widget.id))}
                  onAccountChange={(accountId) => dispatch(updateWidgetAccount({ widgetId: widget.id, accountId }))}
                />
              );
            })
          )}
        </PanelsArea>
      </LeftArea>

      <RightPane>
        <OrderFormWrapper><OrderForm /></OrderFormWrapper>
        <OpenOrdersWrapper><OpenOrders /></OpenOrdersWrapper>
      </RightPane>

      {/* Add-widget dropdown portal */}
      {dropOpen && availableExchanges.length > 0 && createPortal(
        <div
          style={{
            position: 'fixed',
            top: dropPos.top,
            left: dropPos.left,
            minWidth: 180,
            zIndex: 99999,
            background: '#141a28',
            border: '1px solid #5087f2',
            borderRadius: '4px',
            boxShadow: '0 4px 20px rgba(0,0,0,0.7)',
            fontFamily: 'inherit',
            overflow: 'hidden',
          }}
        >
          {atMax && (
            <div style={{ padding: '0.3rem 0.6rem', color: '#e0b94a', fontSize: '0.68rem', borderBottom: '1px solid #1e2738' }}>
              At max ({maxWidgets}). Adding will replace the last widget.
            </div>
          )}
          {availableExchanges.map(exchange => (
            <DropdownItem key={exchange} onMouseDown={e => { e.preventDefault(); handleAdd(exchange); }}>
              <ExBadge $exchange={exchange}>{exchange}</ExBadge>
              {accounts.filter(a => a.exchange === exchange).length} account(s)
            </DropdownItem>
          ))}
        </div>,
        document.body
      )}
    </Wrapper>
  );
};

export default Dashboard;

