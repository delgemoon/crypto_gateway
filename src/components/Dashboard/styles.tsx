import styled from 'styled-components';

export const Container = styled.section`
  margin: 0.9rem;
  border: 1px solid #29303e;
  background: #141a28;
  color: #bfc1c8;
`;

export const Header = styled.div`
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0.7rem;
  border-bottom: 1px solid #29303e;

  h4 {
    margin: 0;
    color: #d9dde4;
  }

  .controls {
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }

  select {
    border-radius: 3px;
    padding: 0.3rem;
    color: #ffffff;
    border: none;
    background-color: #303947;
  }
`;

export const SummaryGrid = styled.div`
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 0.5rem;
  padding: 0.7rem;
  border-bottom: 1px solid #29303e;

  @media only screen and (min-width: 800px) {
    grid-template-columns: repeat(4, minmax(0, 1fr));
  }

  .stat {
    background: #0f1522;
    border: 1px solid #1e2738;
    padding: 0.55rem;
  }

  .label {
    color: #7e8b99;
    font-size: 0.72rem;
    margin-bottom: 0.2rem;
    display: block;
  }

  .value {
    color: #e8edf4;
    font-size: 0.95rem;
  }
`;

export const ContentGrid = styled.div`
  display: grid;
  grid-template-columns: minmax(0, 1fr);

  @media only screen and (min-width: 1000px) {
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  }
`;

export const Card = styled.div`
  border-top: 1px solid #29303e;
  padding: 0.7rem;

  h5 {
    margin-bottom: 0.5rem;
    color: #d9dde4;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.86rem;
  }

  th,
  td {
    text-align: right;
    padding: 0.25rem;
    border-bottom: 1px solid #1f2838;
  }

  th:first-child,
  td:first-child {
    text-align: left;
  }

  .buy {
    color: #33b48f;
  }

  .sell {
    color: #d0616e;
  }
`;

// Dashboard-specific layout
export const DashboardWrapper = styled.div`
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
  background: #0d1117;
`;

export const MainArea = styled.div`
  display: grid;
  grid-template-columns: 280px 1fr 260px;
  flex: 1;
  overflow: hidden;
  gap: 0;
  border-top: none;

  @media (max-width: 1100px) {
    grid-template-columns: 240px 1fr 220px;
  }

  @media (max-width: 800px) {
    grid-template-columns: 1fr;
    overflow-y: auto;
  }
`;

export const Column = styled.div`
  display: flex;
  flex-direction: column;
  overflow-y: auto;
  border-right: 1px solid #1e2738;

  &:last-child { border-right: none; }
`;

export const BottomArea = styled.div`
  border-top: 1px solid #1e2738;
  height: 200px;
  overflow: hidden;
  flex-shrink: 0;

  @media (max-width: 800px) {
    height: auto;
  }
`;

