import { FunctionComponent } from 'react';
import styled from 'styled-components';
import { useAppSelector } from '../../hooks';
import { selectAccounts } from '../Settings/settingsSlice';
import ExchangePanel from './ExchangePanel';
import OrderForm from './OrderForm';
import OpenOrders from './OpenOrders';

const Wrapper = styled.div`
  display: flex;
  height: 100%;
  overflow: hidden;
  background: #0d1117;
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

const Dashboard: FunctionComponent = () => {
  const accounts = useAppSelector(selectAccounts);

  return (
    <Wrapper>
      <PanelsArea>
        {accounts.length === 0 ? (
          <NoAccounts>
            <strong>No exchange accounts configured</strong>
            <span>Go to ⚙ Settings → Exchange to add an account</span>
          </NoAccounts>
        ) : (
          accounts.map(account => (
            <ExchangePanel key={account.id} account={account} />
          ))
        )}
      </PanelsArea>

      <RightPane>
        <OrderFormWrapper><OrderForm /></OrderFormWrapper>
        <OpenOrdersWrapper><OpenOrders /></OpenOrdersWrapper>
      </RightPane>
    </Wrapper>
  );
};

export default Dashboard;
