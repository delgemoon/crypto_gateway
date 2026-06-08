import styled from 'styled-components';

export const SettingsContainer = styled.div`
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
  background: #0d1117;
`;

export const TabBar = styled.div`
  display: flex;
  background: #0d1117;
  border-bottom: 1px solid #1e2738;
  padding: 0 1.5rem;
  flex-shrink: 0;
`;

export const TabBtn = styled.button<{ $active?: boolean }>`
  background: transparent;
  border: none;
  border-bottom: 2px solid ${(p) => (p.$active ? '#5087f2' : 'transparent')};
  color: ${(p) => (p.$active ? '#e8edf4' : '#7e8b99')};
  padding: 0.75rem 1.25rem;
  font-size: 0.88rem;
  cursor: pointer;
  transition: all 0.15s;

  &:hover { color: #e8edf4; }
`;

export const TabContent = styled.div`
  flex: 1;
  overflow-y: auto;
  padding: 1.5rem;
`;

export const SectionCard = styled.div`
  background: #141a28;
  border: 1px solid #1e2738;
  border-radius: 4px;
  margin-bottom: 1.25rem;
`;

export const SectionHeader = styled.div`
  padding: 0.75rem 1rem;
  border-bottom: 1px solid #1e2738;
  display: flex;
  align-items: center;
  justify-content: space-between;

  h3 {
    color: #d9dde4;
    font-size: 0.92rem;
    margin: 0;
  }

  span.desc {
    color: #4a5568;
    font-size: 0.78rem;
    margin-left: 0.5rem;
  }
`;

export const SectionBody = styled.div`
  padding: 1rem;
`;

export const FormGrid = styled.div<{ cols?: number }>`
  display: grid;
  grid-template-columns: repeat(${(p) => p.cols ?? 2}, minmax(0, 1fr));
  gap: 0.75rem;

  @media (max-width: 650px) {
    grid-template-columns: 1fr;
  }
`;

export const FormGroup = styled.div<{ $span?: number }>`
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  grid-column: span ${(p) => p.$span ?? 1};
`;

export const Label = styled.div`
  color: #7e8b99;
  font-size: 0.75rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
`;

export const Input = styled.input`
  background: #0f1522;
  border: 1px solid #29303e;
  color: #e8edf4;
  padding: 0.45rem 0.65rem;
  border-radius: 3px;
  font-size: 0.88rem;

  &:focus { border-color: #5087f2; outline: none; }
  &::placeholder { color: #3a4558; }
`;

export const Textarea = styled.textarea`
  background: #0f1522;
  border: 1px solid #29303e;
  color: #e8edf4;
  padding: 0.45rem 0.65rem;
  border-radius: 3px;
  font-size: 0.88rem;
  resize: vertical;
  min-height: 72px;

  &:focus { border-color: #5087f2; outline: none; }
  &::placeholder { color: #3a4558; }
`;

export const Select = styled.select`
  background: #0f1522;
  border: 1px solid #29303e;
  color: #e8edf4;
  padding: 0.45rem 0.65rem;
  border-radius: 3px;
  font-size: 0.88rem;

  &:focus { border-color: #5087f2; outline: none; }
`;

export const CheckboxGroup = styled.label`
  display: flex;
  align-items: center;
  gap: 0.5rem;
  color: #bfc1c8;
  font-size: 0.85rem;
  cursor: pointer;
  margin-top: 0.25rem;

  input[type='checkbox'] { accent-color: #5087f2; width: 14px; height: 14px; }
`;

export const ButtonRow = styled.div`
  display: flex;
  gap: 0.5rem;
  margin-top: 1rem;
  justify-content: flex-end;
`;

export const Btn = styled.button<{ $variant?: 'primary' | 'danger' | 'ghost' }>`
  padding: 0.45rem 1.1rem;
  border-radius: 3px;
  font-size: 0.85rem;
  cursor: pointer;
  transition: opacity 0.15s;
  border: 1px solid;

  background: ${({ $variant }) =>
    $variant === 'danger' ? '#7a1c1c' :
    $variant === 'ghost' ? 'transparent' : '#1e3a6e'};
  color: ${({ $variant }) =>
    $variant === 'danger' ? '#f4a0a0' :
    $variant === 'ghost' ? '#7e8b99' : '#5087f2'};
  border-color: ${({ $variant }) =>
    $variant === 'danger' ? '#a03030' :
    $variant === 'ghost' ? '#29303e' : '#2a4a8a'};

  &:hover { opacity: 0.82; }
  &:disabled { opacity: 0.4; cursor: not-allowed; }
`;

// Exchange account card
export const AccountList = styled.div`
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
`;

export const AccountCard = styled.div`
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: #0f1522;
  border: 1px solid #1a2233;
  padding: 0.65rem 0.85rem;
  border-radius: 4px;

  .info { display: flex; flex-direction: column; gap: 0.2rem; }
  .name { color: #e8edf4; font-size: 0.9rem; }
  .meta { display: flex; gap: 0.6rem; flex-wrap: wrap; align-items: center; }

  .badge {
    padding: 0.08rem 0.4rem;
    border-radius: 3px;
    font-size: 0.7rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .exchange-badge { background: #1e2f4a; color: #5087f2; }
  .testnet-badge  { background: #2a1e1e; color: #d0616e; }
  .key-preview    { color: #4a5568; font-size: 0.78rem; }
  .risk-badge     { background: #1a2a1a; color: #33b48f; font-size: 0.72rem; }

  .actions { display: flex; gap: 0.4rem; flex-shrink: 0; }
`;

export const EmptyState = styled.p`
  color: #4a5568;
  font-size: 0.85rem;
  text-align: center;
  padding: 1.25rem;
`;

export const Divider = styled.hr`
  border: none;
  border-top: 1px solid #1e2738;
  margin: 0.75rem 0;
`;

export const TagInput = styled.div`
  background: #0f1522;
  border: 1px solid #29303e;
  border-radius: 3px;
  padding: 0.35rem 0.55rem;
  min-height: 38px;
  display: flex;
  flex-wrap: wrap;
  gap: 0.3rem;
  align-items: center;

  input {
    background: transparent;
    border: none;
    color: #e8edf4;
    font-size: 0.88rem;
    flex: 1;
    min-width: 80px;
    &:focus { outline: none; }
  }
`;

export const Tag = styled.span`
  background: #1e2f4a;
  color: #5087f2;
  border-radius: 3px;
  padding: 0.1rem 0.5rem;
  font-size: 0.78rem;
  display: flex;
  align-items: center;
  gap: 0.3rem;

  button {
    background: none;
    border: none;
    color: #4a6a9e;
    cursor: pointer;
    padding: 0;
    font-size: 0.75rem;
    line-height: 1;
    &:hover { color: #d0616e; }
  }
`;

export const SaveBanner = styled.div<{ $visible: boolean }>`
  position: fixed;
  bottom: 1.5rem;
  right: 1.5rem;
  background: #0f3320;
  color: #33b48f;
  border: 1px solid #33b48f40;
  border-radius: 4px;
  padding: 0.55rem 1rem;
  font-size: 0.85rem;
  transition: opacity 0.3s;
  opacity: ${(p) => (p.$visible ? 1 : 0)};
  pointer-events: none;
`;
