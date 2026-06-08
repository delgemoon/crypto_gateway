import { FunctionComponent } from 'react';
import { NavBar, Brand, NavTab } from './styles';

type View = 'trading' | 'rfq' | 'portfolio' | 'orders' | 'telegram' | 'settings' | 'aggbook';

interface NavigationProps {
  currentView: View;
  onNavigate: (view: View) => void;
}

const Navigation: FunctionComponent<NavigationProps> = ({ currentView, onNavigate }) => {
  return (
    <NavBar>
      <Brand>
        📊 <span className="exchange">Trading</span>Dashboard
      </Brand>
      <NavTab $active={currentView === 'trading'} onClick={() => onNavigate('trading')}>
        📊 Trading
      </NavTab>
      <NavTab $active={currentView === 'rfq'} onClick={() => onNavigate('rfq')}>
        🔄 RFQ
      </NavTab>
      <NavTab $active={currentView === 'portfolio'} onClick={() => onNavigate('portfolio')}>
        📈 Portfolio
      </NavTab>
      <NavTab $active={currentView === 'orders'} onClick={() => onNavigate('orders')}>
        📋 Orders
      </NavTab>
      <NavTab $active={currentView === 'aggbook'} onClick={() => onNavigate('aggbook')}>
        📚 Agg Book
      </NavTab>
      <NavTab $active={currentView === 'telegram'} onClick={() => onNavigate('telegram')}>
        🤖 Telegram
      </NavTab>
      <NavTab $active={currentView === 'settings'} onClick={() => onNavigate('settings')}>
        ⚙ Settings
      </NavTab>
    </NavBar>
  );
};

export default Navigation;
