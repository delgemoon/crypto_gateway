import styled from 'styled-components';

export const NavBar = styled.nav`
  display: flex;
  align-items: center;
  background: #0d1117;
  border-bottom: 1px solid #1e2738;
  padding: 0 1rem;
  height: 48px;
  gap: 0.5rem;
`;

export const Brand = styled.div`
  color: #e8edf4;
  font-size: 1rem;
  font-weight: 600;
  letter-spacing: 0.04em;
  margin-right: 1.5rem;
  display: flex;
  align-items: center;
  gap: 0.5rem;

  span.exchange {
    color: #5087f2;
  }
`;

export const NavTab = styled.button<{ $active?: boolean }>`
  background: ${(p) => (p.$active ? '#1e2a3a' : 'transparent')};
  color: ${(p) => (p.$active ? '#e8edf4' : '#7e8b99')};
  border: none;
  border-bottom: 2px solid ${(p) => (p.$active ? '#5087f2' : 'transparent')};
  padding: 0 1rem;
  height: 100%;
  cursor: pointer;
  font-size: 0.9rem;
  transition: all 0.15s;

  &:hover {
    color: #e8edf4;
    background: #1a2233;
  }
`;
