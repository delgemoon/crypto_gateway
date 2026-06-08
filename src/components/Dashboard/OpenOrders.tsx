import { FunctionComponent, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useAppDispatch, useAppSelector } from '../../hooks';
import { selectOpenOrders, setOpenOrders, removeOpenOrder, Order } from './ordersSlice';
import { selectSelectedInstrument } from './instrumentsSlice';
import { selectActiveAccountId } from '../Settings/settingsSlice';
import styled from 'styled-components';

const Panel = styled.div`
  background: #141a28;
  border: 1px solid #1e2738;
  display: flex;
  flex-direction: column;
`;

const PanelHeader = styled.div`
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.5rem 0.75rem;
  border-bottom: 1px solid #1e2738;
  color: #d9dde4;
  font-size: 0.85rem;

  .count {
    background: #1e2f4a;
    color: #5087f2;
    border-radius: 10px;
    padding: 0.05rem 0.45rem;
    font-size: 0.75rem;
    margin-left: 0.4rem;
  }
`;

const Table = styled.table`
  width: 100%;
  border-collapse: collapse;
  font-size: 0.8rem;

  th, td {
    padding: 0.35rem 0.6rem;
    text-align: right;
    border-bottom: 1px solid #1a2233;
    white-space: nowrap;
  }

  th {
    color: #4a5568;
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    background: #0f1522;
  }

  td:first-child, th:first-child { text-align: left; }

  .buy { color: #33b48f; }
  .sell { color: #d0616e; }
`;

const CancelBtn = styled.button`
  background: transparent;
  border: 1px solid #4a2020;
  color: #d0616e;
  font-size: 0.72rem;
  padding: 0.2rem 0.5rem;
  border-radius: 3px;
  cursor: pointer;
  &:hover { background: #3a1010; }
`;

const Empty = styled.div`
  color: #4a5568;
  font-size: 0.82rem;
  text-align: center;
  padding: 1.5rem;
`;

const RefreshBtn = styled.button`
  background: transparent;
  border: none;
  color: #4a5568;
  font-size: 0.75rem;
  cursor: pointer;
  &:hover { color: #7e8b99; }
`;

const fmt = (n?: number) =>
  n == null ? '—' : n.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

const OpenOrders: FunctionComponent = () => {
  const dispatch = useAppDispatch();
  const orders = useAppSelector(selectOpenOrders);
  const instrument = useAppSelector(selectSelectedInstrument);
  const activeAccountId = useAppSelector(selectActiveAccountId);

  const refresh = useCallback(() => {
    if (!activeAccountId || !instrument) return;
    invoke<Order[]>('get_open_orders', {
      accountId: activeAccountId,
      instrumentName: instrument,
    })
      .then((list) => dispatch(setOpenOrders(list)))
      .catch(console.error);
  }, [activeAccountId, instrument]);

  useEffect(() => {
    dispatch(setOpenOrders([]));
    refresh();
    const interval = setInterval(refresh, 5000);
    return () => clearInterval(interval);
  }, [refresh]);

  const handleCancel = async (orderId: string, instrumentName: string) => {
    if (!activeAccountId) return;
    try {
      await invoke('cancel_order', { accountId: activeAccountId, orderId, instrumentName });
      dispatch(removeOpenOrder(orderId));
    } catch (err) {
      console.error('Cancel failed:', err);
    }
  };

  return (
    <Panel>
      <PanelHeader>
        <span>
          Open Orders
          {orders.length > 0 && <span className="count">{orders.length}</span>}
        </span>
        <RefreshBtn onClick={refresh}>↻ Refresh</RefreshBtn>
      </PanelHeader>

      {orders.length === 0 ? (
        <Empty>
          {activeAccountId ? 'No open orders' : 'Select an account to view orders'}
        </Empty>
      ) : (
        <Table>
          <thead>
            <tr>
              <th>Instrument</th>
              <th>Side</th>
              <th>Type</th>
              <th>Price</th>
              <th>Amount</th>
              <th>Filled</th>
              <th>TIF</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {orders.map((o) => (
              <tr key={o.order_id}>
                <td>{o.instrument_name}</td>
                <td className={o.direction}>{o.direction.toUpperCase()}</td>
                <td>{o.order_type}</td>
                <td>{fmt(o.price)}</td>
                <td>{o.amount}</td>
                <td>{o.filled_amount}</td>
                <td>{o.time_in_force.replace('good_til_cancelled', 'GTC').replace('immediate_or_cancel', 'IOC').replace('fill_or_kill', 'FOK')}</td>
                <td>
                  <CancelBtn onClick={() => handleCancel(o.order_id, o.instrument_name)}>Cancel</CancelBtn>
                </td>
              </tr>
            ))}
          </tbody>
        </Table>
      )}
    </Panel>
  );
};

export default OpenOrders;
