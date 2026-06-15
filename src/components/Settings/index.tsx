import { FunctionComponent, useEffect, useState, FormEvent } from 'react';
import styled from 'styled-components';
import { invoke } from '@tauri-apps/api/core';
import { useAppDispatch, useAppSelector } from '../../hooks';
import {
  selectAccounts, selectGeneral, selectTelegram,
  selectTags, selectClients, selectRfqSettings,
  setAccounts, upsertAccount, removeAccount, setGeneral, setRfqSettings, setTelegram,
  setTags, upsertTag, removeTag as removeTagAction,
  setClients, upsertClient, removeClient as removeClientAction,
  Account, GeneralSettings, TelegramSettings, RfqSettings,
  Tag as TagModel, Client, ClientTelegramChat,
} from './settingsSlice';
import {
  SettingsContainer, TabBar, TabBtn, TabContent,
  SectionCard, SectionHeader, SectionBody,
  FormGrid, FormGroup, Label, Input, Textarea, Select, CheckboxGroup,
  ButtonRow, Btn, AccountList, AccountCard, EmptyState,
  Divider, SaveBanner,
} from './styles';
import { v4 as uuidv4 } from 'uuid';
import {
  selectAggBookConfigs, setConfigs as setAggConfigs,
  upsertConfig as upsertAggConfig, removeConfig as removeAggConfig,
  AggBookConfig,
} from '../AggBook/aggBookSlice';

// ── Types & constants ──────────────────────────────────────────────────────

type SettingsTab = 'general' | 'exchange' | 'client' | 'telegram' | 'tags' | 'venue' | 'aggbook' | 'rfq';
const EXCHANGES = ['deribit', 'okx', 'bybit', 'coincall', 'binance', 'mexc', 'hyperliquid', 'uniswap', 'bullish'] as const;
const TIF_OPTIONS = [
  { value: 'good_til_cancelled', label: 'GTC — Good Till Cancelled' },
  { value: 'immediate_or_cancel', label: 'IOC — Immediate or Cancel' },
  { value: 'fill_or_kill', label: 'FOK — Fill or Kill' },
];

const RATE_TIERS = [
  { value: 'tier1',       label: 'Tier 1 (Default)' },
  { value: 'tier2',       label: 'Tier 2' },
  { value: 'vip1',        label: 'VIP 1' },
  { value: 'vip2',        label: 'VIP 2' },
  { value: 'vip3',        label: 'VIP 3' },
  { value: 'vip4',        label: 'VIP 4' },
  { value: 'vip5',        label: 'VIP 5' },
  { value: 'market_maker',label: 'Market Maker' },
];

/** Return a short human-readable description of actual order limits for a tier */
function tierOrderRps(exchange: string, tier: string): string {
  const table: Record<string, Record<string, number>> = {
    deribit:  { tier1: 10, tier2: 20, vip1: 50, vip2: 100, vip3: 200, vip4: 500, vip5: 700, market_maker: 2000 },
    bybit:    { tier1: 10, tier2: 20, vip1: 30, vip2: 40,  vip3: 50,  vip4: 60,  vip5: 100, market_maker: 200  },
    okx:      { tier1: 20, tier2: 40, vip1: 60, vip2: 90,  vip3: 150, vip4: 200, vip5: 300, market_maker: 300  },
    coincall: { tier1: 10, tier2: 20, vip1: 30, vip2: 50,  vip3: 80,  vip4: 100, vip5: 150, market_maker: 200  },
    binance:  { tier1: 10, tier2: 20, vip1: 50, vip2: 100, vip3: 150, vip4: 200, vip5: 300, market_maker: 500  },
    mexc:         { tier1:  5, tier2: 10, vip1: 20, vip2:  30, vip3:  50, vip4:  75, vip5: 100, market_maker: 150  },
    hyperliquid:  { tier1:  5, tier2: 10, vip1: 15, vip2:  15, vip3:  15, vip4:  15, vip5:  15, market_maker: 100  },
    uniswap:      { tier1:  2, tier2:  2, vip1:  2, vip2:   2, vip3:   2, vip4:   2, vip5:   2, market_maker:   2  },
    bullish:      { tier1: 10, tier2: 20, vip1: 40, vip2:  60, vip3: 100, vip4: 150, vip5: 200, market_maker: 300  },
  };
  const rps = table[exchange]?.[tier] ?? 10;
  return `${rps} orders/s`;
}

const emptyAccount = (): Partial<Account> => ({
  id: '', name: '', exchange: 'deribit', apiKey: '', apiSecret: '', passphrase: '',
  testnet: false, defaultTif: 'good_til_cancelled', defaultPostOnly: false, riskLimit: 0,
  rateTier: 'tier1',
});

// ── General Tab ────────────────────────────────────────────────────────────

const GeneralTab: FunctionComponent = () => {
  const dispatch = useAppDispatch();
  const general = useAppSelector(selectGeneral);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    invoke<GeneralSettings>('get_general_settings')
      .then((s) => dispatch(setGeneral(s)))
      .catch(console.error);
  }, []);

  const update = (patch: Partial<GeneralSettings>) => dispatch(setGeneral(patch));

  const handleSave = () => {
    invoke('save_general_settings', { settings: general }).catch(() => {});
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  };

  return (
    <>
      <SectionCard>
        <SectionHeader><h3>Display</h3></SectionHeader>
        <SectionBody>
          <FormGrid>
            <FormGroup>
              <Label>Theme</Label>
              <Select value={general.theme} onChange={(e) => update({ theme: e.target.value as 'dark' | 'light' })}>
                <option value="dark">Dark</option>
                <option value="light">Light (coming soon)</option>
              </Select>
            </FormGroup>
            <FormGroup>
              <Label>Number Locale</Label>
              <Select value={general.numberLocale} onChange={(e) => update({ numberLocale: e.target.value })}>
                <option value="en-US">en-US (1,234.56)</option>
                <option value="de-DE">de-DE (1.234,56)</option>
                <option value="fr-FR">fr-FR (1 234,56)</option>
              </Select>
            </FormGroup>
            <FormGroup>
              <Label>Price Decimals</Label>
              <Input type="number" min={0} max={8} value={general.priceDecimals}
                onChange={(e) => update({ priceDecimals: Number(e.target.value) })} />
            </FormGroup>
            <FormGroup>
              <Label>Size Decimals</Label>
              <Input type="number" min={0} max={8} value={general.sizeDecimals}
                onChange={(e) => update({ sizeDecimals: Number(e.target.value) })} />
            </FormGroup>
            <FormGroup>
              <Label>Max Dashboard Widgets</Label>
              <Input
                type="number" min={1} max={12}
                value={general.maxDashboardWidgets ?? 4}
                onChange={(e) => update({ maxDashboardWidgets: Math.max(1, Math.min(12, Number(e.target.value))) })}
              />
            </FormGroup>
          </FormGrid>
        </SectionBody>
      </SectionCard>

      <SectionCard>
        <SectionHeader><h3>Trading Defaults</h3></SectionHeader>
        <SectionBody>
          <FormGrid>
            <FormGroup>
              <Label>Default Currency</Label>
              <Select value={general.defaultCurrency} onChange={(e) => update({ defaultCurrency: e.target.value })}>
                {['BTC', 'ETH', 'SOL', 'USDC', 'USDT'].map((c) => (
                  <option key={c} value={c}>{c}</option>
                ))}
              </Select>
            </FormGroup>
            <FormGroup>
              <Label>Order Confirmation</Label>
              <CheckboxGroup>
                <input type="checkbox" checked={general.confirmOrders}
                  onChange={(e) => update({ confirmOrders: e.target.checked })} />
                Require confirmation before submitting orders
              </CheckboxGroup>
            </FormGroup>
          </FormGrid>
        </SectionBody>
      </SectionCard>

      <SectionCard>
        <SectionHeader><h3>Order ID Settings</h3></SectionHeader>
        <SectionBody>
          <p style={{ fontSize: '0.8rem', color: '#7e8b99', marginBottom: '0.75rem' }}>
            Each order is stamped with a unique system order ID (UTC millisecond timestamp) encoded in the
            client order ID as <code style={{ color: '#58a6ff', fontSize: '0.75rem' }}>{'botId_machineHash_timestampMs'}</code>.
          </p>
          <FormGrid>
            <FormGroup>
              <Label>Bot ID</Label>
              <Input
                type="number" min={1} max={9999}
                value={general.botId ?? 1}
                onChange={(e) => update({ botId: Math.max(1, Math.min(9999, Number(e.target.value))) })}
              />
            </FormGroup>
          </FormGrid>
          <div style={{ fontSize: '0.75rem', color: '#4a5568', marginTop: '0.5rem' }}>
            Bot ID 1–9999. Use different IDs for multiple trading bots/machines to avoid clOrdId conflicts.
          </div>
        </SectionBody>
      </SectionCard>

      <SectionCard>
        <SectionHeader><h3>Market Data Performance</h3></SectionHeader>
        <SectionBody>
          <p style={{ fontSize: '0.8rem', color: '#7e8b99', marginBottom: '0.75rem' }}>
            Controls how often the backend pushes orderbook updates to the UI.
            Lower = more real-time; higher = less CPU/memory pressure. Takes effect on next instrument subscription.
          </p>
          <FormGrid>
            <FormGroup>
              <Label>Book Emit Interval (ms)</Label>
              <Select
                value={general.bookEmitIntervalMs ?? 80}
                onChange={(e) => update({ bookEmitIntervalMs: Number(e.target.value) })}
              >
                <option value={20}>20ms — Ultra fast (high CPU)</option>
                <option value={50}>50ms — Fast</option>
                <option value={80}>80ms — Default (recommended)</option>
                <option value={150}>150ms — Smooth</option>
                <option value={250}>250ms — Low resource</option>
                <option value={500}>500ms — Minimal (very slow machines)</option>
              </Select>
            </FormGroup>
          </FormGrid>
          <div style={{ fontSize: '0.75rem', color: '#4a5568', marginTop: '0.5rem' }}>
            At 80ms the book updates ~12×/sec. The backend always keeps a full book in memory regardless of this setting.
          </div>
        </SectionBody>
      </SectionCard>

      <SectionCard>
        <SectionBody>
          <p style={{ fontSize: '0.8rem', color: '#7e8b99', marginBottom: '0.75rem' }}>
            Select which coins to show in the Account Summary panel. Leave all deselected to show all coins (may use more memory).
          </p>
          <CheckboxGroup style={{ flexWrap: 'wrap', gap: '0.6rem' }}>
            {(['BTC', 'ETH', 'SOL', 'USDT', 'USDC', 'USD'] as const).map((coin) => {
              const watched = general.watchedCoins ? general.watchedCoins.split(',').map((s: string) => s.trim()).filter(Boolean) : [];
              const checked = watched.includes(coin);
              const toggle = () => {
                const next = checked ? watched.filter((c: string) => c !== coin) : [...watched, coin];
                update({ watchedCoins: next.join(',') });
              };
              return (
                <label key={coin} style={{ display: 'flex', alignItems: 'center', gap: '0.3rem', cursor: 'pointer', color: '#c8d6e5', fontSize: '0.85rem' }}>
                  <input type="checkbox" checked={checked} onChange={toggle} />
                  {coin}
                </label>
              );
            })}
          </CheckboxGroup>
          {general.watchedCoins && (
            <div style={{ marginTop: '0.5rem', fontSize: '0.75rem', color: '#4a90d9' }}>
              Filtering: {general.watchedCoins.split(',').filter(Boolean).join(', ')}
            </div>
          )}
        </SectionBody>
      </SectionCard>

      <ButtonRow>
        <Btn $variant="primary" onClick={handleSave}>Save General Settings</Btn>
      </ButtonRow>
      <SaveBanner $visible={saved}>✓ Settings saved</SaveBanner>
    </>
  );
};

// ── Exchange Tab ───────────────────────────────────────────────────────────

const ExchangeTab: FunctionComponent = () => {
  const dispatch = useAppDispatch();
  const accounts = useAppSelector(selectAccounts);
  const [showForm, setShowForm] = useState(false);
  const [form, setForm] = useState<Partial<Account>>(emptyAccount());
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [linkedClientIds, setLinkedClientIds] = useState<string[]>([]);
  const allClients = useAppSelector(selectClients);

  useEffect(() => {
    invoke<Account[]>('get_accounts')
      .then((accs) => dispatch(setAccounts(accs)))
      .catch(console.error);
    invoke<Client[]>('get_clients')
      .then((cs) => dispatch(setClients(cs)))
      .catch(console.error);
  }, []);

  const handleEdit = (acc: Account) => {
    setForm({ ...acc });
    setShowForm(true);
    setError(null);
    if (acc.id) {
      invoke<string[]>('get_account_clients', { accountId: acc.id })
        .then(setLinkedClientIds)
        .catch(console.error);
    } else {
      setLinkedClientIds([]);
    }
  };

  const handleDelete = async (id: string) => {
    if (!confirm('Delete this account?')) return;
    await invoke('delete_account', { id }).catch(console.error);
    dispatch(removeAccount(id));
  };

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (!form.name?.trim()) { setError('Account name is required'); return; }
    if (!form.apiKey?.trim()) { setError('Wallet/API Key is required'); return; }
    if (!form.apiSecret?.trim()) { setError('Private Key/API Secret is required'); return; }
    if (form.exchange === 'okx' && !form.passphrase?.trim()) { setError('Passphrase is required for OKX'); return; }
    if (form.exchange === 'uniswap' && !(form as any).rpcUrl?.trim()) { setError('RPC URL is required for Uniswap'); return; }
    setSaving(true);
    setError(null);
    try {
      const saved = await invoke<Account>('save_account', {
        account: {
          id: form.id ?? '',
          name: form.name,
          exchange: form.exchange ?? 'deribit',
          apiKey: form.apiKey,
          apiSecret: form.apiSecret,
          passphrase: form.passphrase || null,
          testnet: form.testnet ?? false,
          defaultTif: form.defaultTif ?? 'good_til_cancelled',
          defaultPostOnly: form.defaultPostOnly ?? false,
          riskLimit: form.riskLimit ?? 0,
          rateTier: form.rateTier ?? 'tier1',
          rpcUrl: (form as any).rpcUrl || null,
          chainId: (form as any).chainId || null,
        },
      });
      dispatch(upsertAccount(saved));
      // Save linked clients for this account
      await invoke('set_account_clients', { accountId: saved.id, clientIds: linkedClientIds }).catch(console.error);
      setShowForm(false);
      setForm(emptyAccount());
      setLinkedClientIds([]);
    } catch (err: any) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  };

  const upd = (patch: Partial<Account>) => setForm((f) => ({ ...f, ...patch }));

  return (
    <>
      <SectionCard>
        <SectionHeader>
          <h3>Exchange Accounts</h3>
          {!showForm && (
            <Btn $variant="primary" onClick={() => { setForm(emptyAccount()); setShowForm(true); setError(null); }}>
              + Add Account
            </Btn>
          )}
        </SectionHeader>
        <SectionBody>
          <AccountList>
            {accounts.length === 0 && (
              <EmptyState>No accounts yet — add one to start trading.</EmptyState>
            )}
            {accounts.map((acc) => (
              <AccountCard key={acc.id}>
                <div className="info">
                  <span className="name">{acc.name}</span>
                  <div className="meta">
                    <span className="badge exchange-badge">{acc.exchange}</span>
                    {acc.testnet && <span className="badge testnet-badge">Testnet</span>}
                    <span className="key-preview">Key: {acc.apiKey.slice(0, 10)}…</span>
                    {acc.riskLimit > 0 && (
                      <span className="badge risk-badge">Risk ≤ {acc.riskLimit.toLocaleString()}</span>
                    )}
                    <span className="badge" style={{ background: 'rgba(88,166,255,0.12)', color: '#58a6ff' }}>
                      {(acc.rateTier ?? 'tier1').replace('_', ' ').replace(/\b\w/g, c => c.toUpperCase())}
                    </span>
                    <span className="key-preview">TIF: {acc.defaultTif.replace('good_til_cancelled','GTC').replace('immediate_or_cancel','IOC').replace('fill_or_kill','FOK')}</span>
                    {acc.defaultPostOnly && <span className="key-preview">Post-Only</span>}
                  </div>
                </div>
                <div className="actions">
                  <Btn $variant="ghost" onClick={() => handleEdit(acc)}>Edit</Btn>
                  <Btn $variant="danger" onClick={() => handleDelete(acc.id)}>Delete</Btn>
                </div>
              </AccountCard>
            ))}
          </AccountList>
        </SectionBody>
      </SectionCard>

      {showForm && (
        <SectionCard>
          <SectionHeader>
            <h3>{form.id ? 'Edit Account' : 'New Account'}</h3>
          </SectionHeader>
          <SectionBody>
            <form onSubmit={handleSubmit}>
              {/* ── Connection ── */}
              <FormGrid>
                <FormGroup>
                  <Label>Account Name</Label>
                  <Input value={form.name ?? ''} placeholder="My Deribit Main"
                    onChange={(e) => upd({ name: e.target.value })} />
                </FormGroup>
                <FormGroup>
                  <Label>Exchange</Label>
                  <Select value={form.exchange}
                    onChange={(e) => upd({ exchange: e.target.value as Account['exchange'] })}>
                    {EXCHANGES.map((ex) => (
                      <option key={ex} value={ex}>{ex.charAt(0).toUpperCase() + ex.slice(1)}</option>
                    ))}
                  </Select>
                </FormGroup>
                <FormGroup $span={2}>
                  <Label>
                    {(form.exchange === 'hyperliquid' || form.exchange === 'uniswap')
                      ? 'Wallet Address (public key)'
                      : 'API Key (Client ID)'}
                  </Label>
                  <Input value={form.apiKey ?? ''} 
                    placeholder={
                      (form.exchange === 'hyperliquid' || form.exchange === 'uniswap')
                        ? '0x... wallet address'
                        : 'Paste your API key / Client ID'
                    }
                    onChange={(e) => upd({ apiKey: e.target.value })} />
                </FormGroup>
                <FormGroup $span={2}>
                  <Label>
                    {(form.exchange === 'hyperliquid' || form.exchange === 'uniswap')
                      ? 'Private Key (stored encrypted)'
                      : 'API Secret (Client Secret)'}
                  </Label>
                  <Input type="password" value={form.apiSecret ?? ''} 
                    placeholder={
                      (form.exchange === 'hyperliquid' || form.exchange === 'uniswap')
                        ? '0x... private key (hex)'
                        : 'Paste your API secret'
                    }
                    onChange={(e) => upd({ apiSecret: e.target.value })} />
                </FormGroup>
                {form.exchange === 'okx' && (
                  <FormGroup $span={2}>
                    <Label>Passphrase <span style={{ color: '#d0616e' }}>*</span> (OKX required)</Label>
                    <Input type="password" value={form.passphrase ?? ''} placeholder="Your OKX API passphrase"
                      onChange={(e) => upd({ passphrase: e.target.value })} />
                  </FormGroup>
                )}
                {form.exchange === 'bullish' && (
                  <FormGroup $span={2}>
                    <Label>Trading Account ID <span style={{ color: '#aaa' }}>(optional — fetched automatically)</span></Label>
                    <Input value={form.passphrase ?? ''} placeholder="Leave blank to auto-detect from API"
                      onChange={(e) => upd({ passphrase: e.target.value })} />
                  </FormGroup>
                )}
                {form.exchange === 'uniswap' && (
                  <>
                    <FormGroup $span={2}>
                      <Label>RPC URL <span style={{ color: '#d0616e' }}>*</span> (Infura / Alchemy endpoint)</Label>
                      <Input value={(form as any).rpcUrl ?? ''} placeholder="https://mainnet.infura.io/v3/YOUR_KEY"
                        onChange={(e) => upd({ rpcUrl: e.target.value } as any)} />
                    </FormGroup>
                    <FormGroup $span={2}>
                      <Label>Chain</Label>
                      <Select value={(form as any).chainId ?? 1} onChange={(e) => upd({ chainId: Number(e.target.value) } as any)}>
                        <option value={1}>Ethereum Mainnet (1)</option>
                        <option value={42161}>Arbitrum One (42161)</option>
                        <option value={8453}>Base (8453)</option>
                        <option value={10}>Optimism (10)</option>
                        <option value={137}>Polygon (137)</option>
                      </Select>
                    </FormGroup>
                  </>
                )}
                <FormGroup $span={2}>
                  <CheckboxGroup>
                    <input type="checkbox" checked={form.testnet ?? false}
                      onChange={(e) => upd({ testnet: e.target.checked })} />
                    Use Testnet / Paper Trading
                  </CheckboxGroup>
                </FormGroup>
              </FormGrid>

              <Divider />

              {/* ── Trading Defaults ── */}
              <p style={{ color: '#7e8b99', fontSize: '0.78rem', marginBottom: '0.65rem', textTransform: 'uppercase', letterSpacing: '0.04em' }}>
                Trading Defaults for this key
              </p>
              <FormGrid>
                <FormGroup>
                  <Label>Default Time In Force</Label>
                  <Select value={form.defaultTif}
                    onChange={(e) => upd({ defaultTif: e.target.value as Account['defaultTif'] })}>
                    {TIF_OPTIONS.map((t) => (
                      <option key={t.value} value={t.value}>{t.label}</option>
                    ))}
                  </Select>
                </FormGroup>
                <FormGroup>
                  <Label>Risk Limit (max notional per order, 0 = no limit)</Label>
                  <Input type="number" min={0} step={1000} value={form.riskLimit ?? 0}
                    placeholder="0"
                    onChange={(e) => upd({ riskLimit: Number(e.target.value) })} />
                </FormGroup>
                <FormGroup>
                  <Label>Account Tier (rate limit)</Label>
                  <Select value={form.rateTier ?? 'tier1'}
                    onChange={(e) => upd({ rateTier: e.target.value })}>
                    {RATE_TIERS.map((t) => (
                      <option key={t.value} value={t.value}>{t.label}</option>
                    ))}
                  </Select>
                </FormGroup>
                <FormGroup>
                  <Label>Order Rate Limit</Label>
                  <Input
                    readOnly
                    value={tierOrderRps(form.exchange ?? 'deribit', form.rateTier ?? 'tier1')}
                    style={{ color: '#3fb950', background: 'rgba(63,185,80,0.08)', cursor: 'default' }}
                  />
                </FormGroup>
                <FormGroup $span={2}>
                  <CheckboxGroup>
                    <input type="checkbox" checked={form.defaultPostOnly ?? false}
                      onChange={(e) => upd({ defaultPostOnly: e.target.checked })} />
                    Default to Post-Only (Maker) orders
                  </CheckboxGroup>
                </FormGroup>
              </FormGrid>

              {error && (
                <p style={{ color: '#d0616e', fontSize: '0.82rem', marginTop: '0.5rem' }}>{error}</p>
              )}

              {/* ── Linked Clients ── */}
              {allClients.length > 0 && (
                <>
                  <Divider />
                  <p style={{ color: '#7e8b99', fontSize: '0.78rem', marginBottom: '0.65rem', textTransform: 'uppercase', letterSpacing: '0.04em' }}>
                    Linked Clients (receive trade alerts)
                  </p>
                  <div style={{ display: 'flex', flexWrap: 'wrap', gap: '0.5rem' }}>
                    {allClients.map((c) => {
                      const checked = linkedClientIds.includes(c.id);
                      return (
                        <label key={c.id} style={{
                          display: 'flex', alignItems: 'center', gap: '0.35rem', cursor: 'pointer',
                          background: checked ? 'rgba(42,171,238,0.12)' : 'rgba(255,255,255,0.04)',
                          border: `1px solid ${checked ? '#2aabee' : 'rgba(255,255,255,0.1)'}`,
                          borderRadius: '6px', padding: '0.3rem 0.6rem', fontSize: '0.82rem', color: '#c8d2dc',
                        }}>
                          <input type="checkbox" checked={checked} onChange={(e) => {
                            setLinkedClientIds(prev =>
                              e.target.checked ? [...prev, c.id] : prev.filter(id => id !== c.id)
                            );
                          }} />
                          {c.companyName || c.contactName || c.id}
                        </label>
                      );
                    })}
                  </div>
                </>
              )}

              <ButtonRow>
                <Btn type="button" $variant="ghost"
                  onClick={() => { setShowForm(false); setForm(emptyAccount()); setLinkedClientIds([]); }}>
                  Cancel
                </Btn>
                <Btn type="submit" $variant="primary" disabled={saving}>
                  {saving ? 'Saving…' : 'Save Account'}
                </Btn>
              </ButtonRow>
            </form>
          </SectionBody>
        </SectionCard>
      )}
    </>
  );
};

// ── Tags Tab ───────────────────────────────────────────────────────────────

const TagsTab: FunctionComponent = () => {
  const dispatch = useAppDispatch();
  const tags = useAppSelector(selectTags);
  const [form, setForm] = useState<Partial<TagModel> | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    invoke<TagModel[]>('get_tags').then(ts => dispatch(setTags(ts))).catch(console.error);
  }, []);

  const handleSave = async () => {
    if (!form || !form.name?.trim()) return;
    setSaving(true);
    try {
      const tag: TagModel = { id: form.id ?? uuidv4(), name: form.name.trim(), color: form.color ?? '#2aabee' };
      await invoke('save_tag', { tag });
      dispatch(upsertTag(tag));
      setForm(null);
    } catch (e) { console.error(e); }
    finally { setSaving(false); }
  };

  const handleDelete = async (id: string) => {
    if (!confirm('Delete this tag?')) return;
    await invoke('delete_tag', { id }).catch(console.error);
    dispatch(removeTagAction(id));
  };

  return (
    <>
      <SectionCard>
        <SectionHeader>
          <h3>Custom Tags</h3>
          {!form && (
            <Btn $variant="primary" onClick={() => setForm({ id: '', name: '', color: '#2aabee' })}>
              + New Tag
            </Btn>
          )}
        </SectionHeader>
        <SectionBody>
          {tags.length === 0 && !form && (
            <EmptyState>No tags yet — create tags to categorise clients.</EmptyState>
          )}
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: '0.5rem', marginBottom: tags.length ? '1rem' : 0 }}>
            {tags.map(t => (
              <span key={t.id} style={{
                display: 'inline-flex', alignItems: 'center', gap: '0.4rem',
                background: `${t.color}22`, border: `1px solid ${t.color}`, borderRadius: '6px',
                padding: '0.25rem 0.6rem', fontSize: '0.82rem', color: t.color,
              }}>
                <span style={{ width: 10, height: 10, borderRadius: '50%', background: t.color, display: 'inline-block' }} />
                {t.name}
                <button type="button" onClick={() => setForm({ ...t })} style={{ background: 'none', border: 'none', color: t.color, cursor: 'pointer', padding: 0, fontSize: '0.75rem' }}>✎</button>
                <button type="button" onClick={() => handleDelete(t.id)} style={{ background: 'none', border: 'none', color: t.color, cursor: 'pointer', padding: 0 }}>×</button>
              </span>
            ))}
          </div>
          {form && (
            <div style={{ display: 'flex', gap: '0.5rem', alignItems: 'center', marginTop: '0.5rem' }}>
              <Input value={form.name ?? ''} placeholder="Tag name"
                onChange={e => setForm(f => ({ ...f, name: e.target.value }))}
                style={{ flex: 1 }} />
              <label style={{ display: 'flex', alignItems: 'center', gap: '0.3rem', color: '#c8d2dc', fontSize: '0.82rem' }}>
                Colour
                <input type="color" value={form.color ?? '#2aabee'}
                  onChange={e => setForm(f => ({ ...f, color: e.target.value }))}
                  style={{ width: 32, height: 28, cursor: 'pointer', border: 'none', background: 'none', padding: 0 }} />
              </label>
              <Btn $variant="primary" onClick={handleSave} disabled={saving || !form.name?.trim()}>
                {saving ? '…' : form.id ? 'Update' : 'Add'}
              </Btn>
              <Btn $variant="ghost" onClick={() => setForm(null)}>Cancel</Btn>
            </div>
          )}
        </SectionBody>
      </SectionCard>
    </>
  );
};

// ── Client Tab (multi-client) ──────────────────────────────────────────────

const emptyClient = (): Client => ({
  id: '', companyName: '', contactName: '', phone: '', email: '', tagIds: '', notes: '',
});

const ClientTab: FunctionComponent = () => {
  const dispatch = useAppDispatch();
  const clients = useAppSelector(selectClients);
  const tags = useAppSelector(selectTags);
  const [selId, setSelId] = useState<string | null>(null);
  const [form, setForm] = useState<Client | null>(null);
  const [chats, setChats] = useState<ClientTelegramChat[]>([]);
  const [chatForm, setChatForm] = useState({ chatId: '', label: '' });
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [chatSaving, setChatSaving] = useState(false);
  const [knownTgChats, setKnownTgChats] = useState<{ id: number; kind: string; title?: string | null; username?: string | null }[]>([]);

  useEffect(() => {
    invoke<Client[]>('get_clients').then(cs => dispatch(setClients(cs))).catch(console.error);
    invoke<TagModel[]>('get_tags').then(ts => dispatch(setTags(ts))).catch(console.error);
    invoke<{ id: number; kind: string; title?: string | null; username?: string | null }[]>('telegram_get_known_chats')
      .then(setKnownTgChats)
      .catch(() => {});
  }, []);

  const selectClient = (id: string) => {
    const c = clients.find(x => x.id === id);
    if (!c) return;
    setSelId(id);
    setForm({ ...c });
    invoke<ClientTelegramChat[]>('get_client_chats', { clientId: id })
      .then(setChats).catch(console.error);
  };

  const handleNew = () => {
    const c = emptyClient();
    setSelId('__new__');
    setForm(c);
    setChats([]);
  };

  const handleSave = async () => {
    if (!form) return;
    setSaving(true);
    try {
      const toSave: Client = { ...form, id: form.id || uuidv4() };
      await invoke('save_client', { client: toSave });
      dispatch(upsertClient(toSave));
      setSelId(toSave.id);
      setForm(toSave);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) { console.error(e); }
    finally { setSaving(false); }
  };

  const handleDelete = async (id: string) => {
    if (!confirm('Delete this client?')) return;
    await invoke('delete_client', { id }).catch(console.error);
    dispatch(removeClientAction(id));
    setSelId(null);
    setForm(null);
    setChats([]);
  };

  const handleAddChat = async () => {
    if (!chatForm.chatId.trim() || !selId || selId === '__new__') return;
    const chat: ClientTelegramChat = { id: uuidv4(), clientId: selId, chatId: chatForm.chatId.trim(), label: chatForm.label.trim() };
    setChatSaving(true);
    try {
      await invoke('save_client_chat', { chat });
      setChats(prev => [...prev, chat]);
      setChatForm({ chatId: '', label: '' });
    } catch (e) { console.error(e); }
    finally { setChatSaving(false); }
  };

  const handleDeleteChat = async (id: string) => {
    await invoke('delete_client_chat', { id }).catch(console.error);
    setChats(prev => prev.filter(c => c.id !== id));
  };

  const upd = (patch: Partial<Client>) => setForm(f => f ? { ...f, ...patch } : f);

  const clientTagIds = form?.tagIds ? form.tagIds.split(',').map(t => t.trim()).filter(Boolean) : [];

  const toggleTag = (tagId: string) => {
    const next = clientTagIds.includes(tagId)
      ? clientTagIds.filter(id => id !== tagId)
      : [...clientTagIds, tagId];
    upd({ tagIds: next.join(',') });
  };

  return (
    <div style={{ display: 'flex', gap: '1rem', height: '100%' }}>
      {/* Left: client list */}
      <div style={{ width: 200, flexShrink: 0 }}>
        <SectionCard style={{ height: '100%' }}>
          <SectionHeader><h3 style={{ fontSize: '0.82rem' }}>Clients</h3></SectionHeader>
          <SectionBody style={{ padding: '0.4rem' }}>
            <Btn $variant="primary" style={{ width: '100%', marginBottom: '0.5rem', fontSize: '0.78rem' }} onClick={handleNew}>
              + New Client
            </Btn>
            {clients.length === 0 && (
              <EmptyState style={{ fontSize: '0.78rem' }}>No clients yet.</EmptyState>
            )}
            {clients.map(c => (
              <div key={c.id}
                onClick={() => selectClient(c.id)}
                style={{
                  padding: '0.45rem 0.6rem', cursor: 'pointer', borderRadius: '6px', marginBottom: '0.2rem',
                  background: selId === c.id ? 'rgba(42,171,238,0.15)' : 'transparent',
                  border: `1px solid ${selId === c.id ? '#2aabee' : 'transparent'}`,
                  fontSize: '0.82rem', color: '#c8d2dc',
                }}>
                <div style={{ fontWeight: 600 }}>{c.companyName || '(no company)'}</div>
                {c.contactName && <div style={{ color: '#7e8b99', fontSize: '0.75rem' }}>{c.contactName}</div>}
              </div>
            ))}
          </SectionBody>
        </SectionCard>
      </div>

      {/* Right: edit form */}
      <div style={{ flex: 1, minWidth: 0 }}>
        {!form ? (
          <SectionCard>
            <SectionBody>
              <EmptyState>Select a client or create a new one.</EmptyState>
            </SectionBody>
          </SectionCard>
        ) : (
          <>
            <SectionCard>
              <SectionHeader>
                <h3>{form.id ? 'Edit Client' : 'New Client'}</h3>
                {form.id && (
                  <Btn $variant="danger" onClick={() => handleDelete(form.id)}>Delete</Btn>
                )}
              </SectionHeader>
              <SectionBody>
                <FormGrid>
                  <FormGroup>
                    <Label>Company Name</Label>
                    <Input value={form.companyName} placeholder="Acme Trading Ltd."
                      onChange={e => upd({ companyName: e.target.value })} />
                  </FormGroup>
                  <FormGroup>
                    <Label>Contact Name</Label>
                    <Input value={form.contactName} placeholder="John Doe"
                      onChange={e => upd({ contactName: e.target.value })} />
                  </FormGroup>
                  <FormGroup>
                    <Label>Phone</Label>
                    <Input value={form.phone} placeholder="+1 555 000 0000"
                      onChange={e => upd({ phone: e.target.value })} />
                  </FormGroup>
                  <FormGroup>
                    <Label>Email</Label>
                    <Input type="email" value={form.email} placeholder="trading@acme.com"
                      onChange={e => upd({ email: e.target.value })} />
                  </FormGroup>
                </FormGrid>

                {tags.length > 0 && (
                  <>
                    <Divider />
                    <Label style={{ marginBottom: '0.4rem' }}>Tags</Label>
                    <div style={{ display: 'flex', flexWrap: 'wrap', gap: '0.4rem', marginBottom: '0.8rem' }}>
                      {tags.map(t => {
                        const active = clientTagIds.includes(t.id);
                        return (
                          <label key={t.id} style={{
                            display: 'flex', alignItems: 'center', gap: '0.3rem', cursor: 'pointer',
                            background: active ? `${t.color}22` : 'rgba(255,255,255,0.04)',
                            border: `1px solid ${active ? t.color : 'rgba(255,255,255,0.1)'}`,
                            borderRadius: '6px', padding: '0.2rem 0.5rem', fontSize: '0.78rem',
                            color: active ? t.color : '#7e8b99',
                          }}>
                            <input type="checkbox" checked={active} onChange={() => toggleTag(t.id)} />
                            <span style={{ width: 8, height: 8, borderRadius: '50%', background: t.color, display: 'inline-block' }} />
                            {t.name}
                          </label>
                        );
                      })}
                    </div>
                  </>
                )}

                <Divider />
                <Label>Notes</Label>
                <Textarea value={form.notes} placeholder="Internal notes…"
                  onChange={e => upd({ notes: e.target.value })} style={{ width: '100%' }} />

                <ButtonRow>
                  <Btn $variant="primary" onClick={handleSave} disabled={saving}>
                    {saving ? 'Saving…' : 'Save Client'}
                  </Btn>
                </ButtonRow>
                <SaveBanner $visible={saved}>✓ Saved</SaveBanner>
              </SectionBody>
            </SectionCard>

            {/* Telegram chats (only if client is saved) */}
            {form.id && (
              <SectionCard style={{ marginTop: '1rem' }}>
                <SectionHeader><h3>Telegram Chat IDs</h3></SectionHeader>
                <SectionBody>
                  {chats.length === 0 && (
                    <EmptyState>No chats linked — add a chat ID to receive trade alerts.</EmptyState>
                  )}
                  {chats.map(chat => (
                    <div key={chat.id} style={{
                      display: 'flex', alignItems: 'center', gap: '0.5rem', marginBottom: '0.4rem',
                      background: 'rgba(255,255,255,0.04)', borderRadius: '6px', padding: '0.4rem 0.6rem',
                    }}>
                      <span style={{ flex: 1, fontSize: '0.82rem', color: '#c8d2dc' }}>
                        <strong>{chat.label || 'Unnamed'}</strong>
                        <span style={{ color: '#7e8b99', marginLeft: '0.5rem' }}>{chat.chatId}</span>
                      </span>
                      <Btn $variant="danger" onClick={() => handleDeleteChat(chat.id)} style={{ padding: '0.2rem 0.5rem', fontSize: '0.78rem' }}>×</Btn>
                    </div>
                  ))}
                  <div style={{ display: 'flex', gap: '0.4rem', marginTop: '0.6rem', flexDirection: 'column' }}>
                    {knownTgChats.length > 0 && (
                      <div>
                        <Label style={{ marginBottom: '0.25rem' }}>Pick a known channel</Label>
                        <Select
                          value=""
                          onChange={e => {
                            const val = e.target.value;
                            if (!val) return;
                            const [id, ...rest] = val.split('|');
                            setChatForm({ chatId: id, label: rest.join('|') });
                          }}
                          style={{ width: '100%' }}
                        >
                          <option value="">— select known chat —</option>
                          {knownTgChats.map(tc => {
                            const raw = tc.title || tc.username || String(tc.id);
                            const label = raw.length > 28 ? raw.slice(0, 26) + '…' : raw;
                            return (
                              <option key={tc.id} value={`${tc.id}|${raw}`}>
                                {label} ({tc.kind}) · {tc.id}
                              </option>
                            );
                          })}
                        </Select>
                      </div>
                    )}
                    <div style={{ display: 'flex', gap: '0.4rem', alignItems: 'flex-end' }}>
                      <div style={{ flex: 1.5 }}>
                        <Label style={{ marginBottom: '0.25rem' }}>Chat ID / @channel</Label>
                        <Input value={chatForm.chatId} placeholder="-100123456789"
                          onChange={e => setChatForm(f => ({ ...f, chatId: e.target.value }))} />
                      </div>
                      <div style={{ flex: 1 }}>
                        <Label style={{ marginBottom: '0.25rem' }}>Label</Label>
                        <Input value={chatForm.label} placeholder="e.g. Trading Alerts"
                          onChange={e => setChatForm(f => ({ ...f, label: e.target.value }))} />
                      </div>
                      <Btn $variant="primary" onClick={handleAddChat} disabled={chatSaving || !chatForm.chatId.trim()}>
                        {chatSaving ? '…' : '+ Add'}
                      </Btn>
                    </div>
                  </div>
                </SectionBody>
              </SectionCard>
            )}
          </>
        )}
      </div>
    </div>
  );
};

// ── Telegram Tab ───────────────────────────────────────────────────────────

interface TgChat {
  id: number;
  kind: string;
  title?: string | null;
  username?: string | null;
}

const TelegramTab: FunctionComponent = () => {
  const dispatch = useAppDispatch();
  const tg = useAppSelector(selectTelegram);
  const [saved, setSaved]           = useState(false);
  const [validating, setValidating] = useState(false);
  const [botName, setBotName]       = useState('');
  const [valError, setValError]     = useState('');

  const [showToken, setShowToken]   = useState(false);
  const [chats, setChats]           = useState<TgChat[]>([]);
  const [syncing, setSyncing]       = useState(false);
  const [resolveRef, setResolveRef] = useState('');
  const [syncMsg, setSyncMsg]       = useState<{ ok: boolean; msg: string } | null>(null);

  useEffect(() => {
    invoke<TelegramSettings>('get_telegram_settings')
      .then((s) => dispatch(setTelegram(s)))
      .catch(console.error);
    // Load known chats from DB
    invoke<TgChat[]>('telegram_get_known_chats')
      .then(setChats)
      .catch(console.error);
  }, []);

  const upd = (patch: Partial<TelegramSettings>) => dispatch(setTelegram(patch));

  const handleSave = () => {
    invoke('save_telegram_settings', { settings: tg }).catch(() => {});
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  };

  // Always persist current settings before any network call so the backend
  // uses the token the user has typed, not the previously saved one.
  const saveFirst = () => invoke('save_telegram_settings', { settings: tg }).catch(() => {});

  const handleValidate = async () => {
    setBotName(''); setValError('');
    setValidating(true);
    try {
      await saveFirst();
      const name = await invoke<string>('telegram_validate');
      setBotName(`@${name}`);
    } catch (e: any) {
      setValError(String(e));
    } finally {
      setValidating(false);
    }
  };

  const handleSync = async () => {
    setSyncing(true); setSyncMsg(null);
    try {
      await saveFirst();
      const list = await invoke<TgChat[]>('telegram_sync_chats');
      setChats(list);
      setSyncMsg({ ok: true, msg: `${list.length} chat${list.length !== 1 ? 's' : ''} found` });
    } catch (e: any) {
      setSyncMsg({ ok: false, msg: String(e) });
    } finally {
      setSyncing(false);
    }
  };

  const handleResolve = async () => {
    if (!resolveRef.trim()) return;
    try {
      await saveFirst();
      const chat = await invoke<TgChat>('telegram_resolve_chat', { chatRef: resolveRef.trim() });
      setChats(prev => prev.find(c => c.id === chat.id) ? prev.map(c => c.id === chat.id ? chat : c) : [...prev, chat]);
      setResolveRef('');
      setSyncMsg({ ok: true, msg: `Added: ${chat.title ?? chat.username ?? chat.id}` });
    } catch (e: any) {
      setSyncMsg({ ok: false, msg: String(e) });
    }
  };

  const handleDeleteChat = async (id: number) => {
    try {
      await invoke('telegram_delete_known_chat', { chatId: id });
      setChats(prev => prev.filter(c => c.id !== id));
    } catch {}
  };

  const chatIcon = (kind: string) => kind === 'channel' ? '📢' : kind === 'supergroup' || kind === 'group' ? '👥' : '💬';

  return (
    <>
      <SectionCard>
        <SectionHeader><h3>Bot Configuration</h3></SectionHeader>
        <SectionBody>
          <FormGrid>
            <FormGroup $span={2}>
              <Label>Bot Token</Label>
              <div style={{ display: 'flex', gap: '0.4rem' }}>
                <Input
                  type={showToken ? 'text' : 'password'}
                  value={tg.botToken}
                  placeholder="123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11"
                  onChange={(e) => upd({ botToken: e.target.value })}
                  style={{ flex: 1 }}
                />
                <Btn type="button" $variant="ghost" onClick={() => setShowToken(v => !v)} title={showToken ? 'Hide token' : 'Show token'}>
                  {showToken ? '🙈' : '👁'}
                </Btn>
                <Btn type="button" $variant="ghost" onClick={handleValidate} disabled={validating || !tg.botToken}>
                  {validating ? '…' : 'Test'}
                </Btn>
              </div>
              {botName && <p style={{ color: '#33b48f', fontSize: '0.78rem', margin: '0.3rem 0 0' }}>✓ Connected as {botName}</p>}
              {valError && <p style={{ color: '#d0616e', fontSize: '0.78rem', margin: '0.3rem 0 0' }}>✗ {valError}</p>}
              <p style={{ color: '#4a5568', fontSize: '0.75rem', margin: '0.3rem 0 0' }}>
                Create a bot via <a href="https://t.me/BotFather" target="_blank" rel="noreferrer" style={{ color: '#2aabee' }}>@BotFather</a> and paste the token here. The token is encrypted in the local database.
              </p>
            </FormGroup>
            <FormGroup $span={2}>
              <Label>Default Chat ID</Label>
              <Input
                value={tg.defaultChatId}
                placeholder="-1001234567890 or @channelname"
                onChange={(e) => upd({ defaultChatId: e.target.value })}
              />
              <p style={{ color: '#4a5568', fontSize: '0.75rem', margin: '0.3rem 0 0' }}>
                Pre-fills the target chat in the Telegram composer.
              </p>
            </FormGroup>
          </FormGrid>
        </SectionBody>
      </SectionCard>

      {/* ── Known Channels / Chats ── */}
      <SectionCard>
        <SectionHeader>
          <h3>Known Channels &amp; Chats</h3>
          <div style={{ display: 'flex', gap: '0.4rem', marginLeft: 'auto' }}>
            <Btn type="button" $variant="ghost" onClick={handleSync} disabled={syncing || !tg.botToken}>
              {syncing ? '⟳ Syncing…' : '⟳ Sync from Telegram'}
            </Btn>
          </div>
        </SectionHeader>
        <SectionBody>
          <p style={{ color: '#4a5568', fontSize: '0.75rem', marginBottom: '0.6rem', marginTop: 0 }}>
            Click <strong>Sync</strong> to pull recent chats from <code>getUpdates</code> (bot must have received a message from each chat). Use <strong>Lookup</strong> to add a channel by @username or numeric ID.
          </p>

          {/* Lookup / add by @username or ID */}
          <div style={{ display: 'flex', gap: '0.4rem', marginBottom: '0.6rem' }}>
            <Input
              value={resolveRef}
              placeholder="@channel_username or -1001234567890"
              onChange={(e) => setResolveRef(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleResolve()}
              style={{ flex: 1 }}
            />
            <Btn type="button" $variant="ghost" onClick={handleResolve} disabled={!resolveRef.trim() || !tg.botToken}>
              Lookup
            </Btn>
          </div>

          {syncMsg && (
            <p style={{ fontSize: '0.78rem', margin: '0 0 0.6rem', color: syncMsg.ok ? '#33b48f' : '#d0616e' }}>
              {syncMsg.ok ? '✓' : '✗'} {syncMsg.msg}
            </p>
          )}

          {chats.length === 0 ? (
            <p style={{ color: '#4a5568', fontSize: '0.8rem', textAlign: 'center', padding: '1rem' }}>
              No chats found yet. Click Sync or add one by Lookup.
            </p>
          ) : (
            <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: '0.8rem' }}>
              <thead>
                <tr style={{ borderBottom: '1px solid #1e2738' }}>
                  <th style={{ textAlign: 'left', padding: '0.3rem 0.4rem', color: '#4a5568', fontWeight: 500 }}>Type</th>
                  <th style={{ textAlign: 'left', padding: '0.3rem 0.4rem', color: '#4a5568', fontWeight: 500 }}>Name</th>
                  <th style={{ textAlign: 'left', padding: '0.3rem 0.4rem', color: '#4a5568', fontWeight: 500, fontFamily: 'monospace' }}>Chat ID</th>
                  <th style={{ textAlign: 'left', padding: '0.3rem 0.4rem', color: '#4a5568', fontWeight: 500 }}>Username</th>
                  <th style={{ width: 32 }}></th>
                </tr>
              </thead>
              <tbody>
                {chats.map((c) => (
                  <tr key={c.id} style={{ borderBottom: '1px solid #1a2030' }}>
                    <td style={{ padding: '0.3rem 0.4rem', color: '#7e8b99' }}>
                      <span style={{ marginRight: '0.3rem' }}>{chatIcon(c.kind)}</span>
                      <span style={{ fontSize: '0.7rem', color: '#4a5568' }}>{c.kind}</span>
                    </td>
                    <td style={{ padding: '0.3rem 0.4rem', color: '#d9dde4' }}>{c.title ?? '—'}</td>
                    <td style={{ padding: '0.3rem 0.4rem', color: '#2aabee', fontFamily: 'monospace', userSelect: 'all' }}>{c.id}</td>
                    <td style={{ padding: '0.3rem 0.4rem', color: '#7e8b99' }}>{c.username ? `@${c.username}` : '—'}</td>
                    <td style={{ padding: '0.2rem 0.3rem' }}>
                      <Btn
                        type="button"
                        $variant="ghost"
                        onClick={() => handleDeleteChat(c.id)}
                        style={{ padding: '0.1rem 0.3rem', fontSize: '0.7rem', color: '#d0616e', borderColor: '#d0616e33' }}
                      >✕</Btn>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </SectionBody>
      </SectionCard>

      <ButtonRow>
        <Btn $variant="primary" onClick={handleSave}>Save Telegram Settings</Btn>
      </ButtonRow>
      <SaveBanner $visible={saved}>✓ Telegram settings saved</SaveBanner>
    </>
  );
};

// ── Main Settings ──────────────────────────────────────────────────────────

// ── Venue Settings Tab ─────────────────────────────────────────────────────

interface VenueSettings {
  exchange: string;
  instrumentTypes: string[];
  marketFeeds: string[];
  orderFeeds: string[];
  notes: string;
}

const VENUE_INSTRUMENT_TYPES = [
  { id: 'option',     label: 'Option' },
  { id: 'future',     label: 'Future' },
  { id: 'perpetual',  label: 'Perpetual' },
  { id: 'spot',       label: 'Spot' },
];

const VENUE_MARKET_FEEDS = [
  { id: 'orderbook_l3',   label: 'Orderbook L3 (Full Depth)' },
  { id: 'orderbook_l2',   label: 'Orderbook L2' },
  { id: 'top_of_book',    label: 'Top of Book (BBO)' },
  { id: 'market_trades',  label: 'Market Trades' },
  { id: 'reference_data', label: 'Reference Data' },
];

const VENUE_ORDER_FEEDS = [
  { id: 'account_summary',   label: 'Account Summary' },
  { id: 'fund_update',       label: 'Fund / Balance Update' },
  { id: 'transfer_update',   label: 'Transfer Update' },
  { id: 'create_order',      label: 'Create Order' },
  { id: 'modify_order',      label: 'Modify Order' },
  { id: 'cancel_order',      label: 'Cancel Order' },
  { id: 'cancel_all_order',  label: 'Cancel All Orders' },
  { id: 'position_update',   label: 'Position Update' },
];

const VenueCapChip = styled.span<{ $on: boolean }>`
  display: inline-flex; align-items: center; gap: 4px;
  padding: 0.18rem 0.5rem; border-radius: 4px; font-size: 0.72rem;
  background: ${p => p.$on ? 'rgba(42,171,238,0.12)' : 'rgba(255,255,255,0.04)'};
  border: 1px solid ${p => p.$on ? 'rgba(42,171,238,0.35)' : 'rgba(255,255,255,0.08)'};
  color: ${p => p.$on ? '#2aabee' : '#4a5568'};
  cursor: pointer; user-select: none;
  transition: all 0.12s;
  &:hover { border-color: ${p => p.$on ? '#2aabee' : 'rgba(255,255,255,0.2)'}; }
`;

const CapDot = styled.span<{ $on: boolean }>`
  width: 6px; height: 6px; border-radius: 50%;
  background: ${p => p.$on ? '#2aabee' : '#2a3040'};
  border: 1px solid ${p => p.$on ? '#2aabee' : '#3a4555'};
  flex-shrink: 0;
`;

function makeEmptyVenue(exchange: string): VenueSettings {
  return { exchange, instrumentTypes: [], marketFeeds: [], orderFeeds: [], notes: '' };
}

const VenueTab: FunctionComponent = () => {
  const [venues, setVenues] = useState<VenueSettings[]>([]);
  const [selEx, setSelEx] = useState<string | null>(null);
  const [form, setForm] = useState<VenueSettings | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    invoke<VenueSettings[]>('get_venue_settings').then(vs => {
      setVenues(vs);
    }).catch(console.error);
  }, []);

  const selectVenue = (ex: string) => {
    setSelEx(ex);
    const existing = venues.find(v => v.exchange === ex);
    if (existing) {
      setForm({ ...existing });
    } else {
      // Load defaults from backend
      invoke<VenueSettings>('get_venue_settings_for', { exchange: ex })
        .then(v => setForm({ ...v }))
        .catch(() => setForm(makeEmptyVenue(ex)));
    }
  };

  const toggle = (field: 'instrumentTypes' | 'marketFeeds' | 'orderFeeds', id: string) => {
    if (!form) return;
    const arr = form[field];
    const next = arr.includes(id) ? arr.filter(x => x !== id) : [...arr, id];
    setForm({ ...form, [field]: next });
  };

  const handleSave = async () => {
    if (!form) return;
    setSaving(true);
    try {
      await invoke('save_venue_settings', { settings: form });
      setVenues(prev => {
        const idx = prev.findIndex(v => v.exchange === form.exchange);
        return idx >= 0
          ? prev.map((v, i) => i === idx ? { ...form } : v)
          : [...prev, { ...form }];
      });
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) { console.error(e); }
    finally { setSaving(false); }
  };

  const handleReset = async () => {
    if (!selEx) return;
    const defaults = await invoke<VenueSettings>('get_venue_settings_for', { exchange: selEx }).catch(() => makeEmptyVenue(selEx));
    // Reset to backend defaults (which returns default_for() if no row)
    await invoke('delete_venue_settings', { exchange: selEx }).catch(() => {});
    const fresh = await invoke<VenueSettings>('get_venue_settings_for', { exchange: selEx }).catch(() => makeEmptyVenue(selEx));
    setForm({ ...fresh });
    setVenues(prev => prev.filter(v => v.exchange !== selEx));
    setSaved(false);
    void defaults; // suppress warning
  };

  const ChipGroup = ({ title, items, field }: {
    title: string;
    items: { id: string; label: string }[];
    field: 'instrumentTypes' | 'marketFeeds' | 'orderFeeds';
  }) => (
    <div style={{ marginBottom: '0.8rem' }}>
      <div style={{ fontSize: '0.65rem', textTransform: 'uppercase', letterSpacing: '0.06em', color: '#4a5568', marginBottom: '0.35rem' }}>
        {title}
      </div>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '0.35rem' }}>
        {items.map(item => {
          const on = form?.[field].includes(item.id) ?? false;
          return (
            <VenueCapChip key={item.id} $on={on} onClick={() => toggle(field, item.id)}>
              <CapDot $on={on} />
              {item.label}
            </VenueCapChip>
          );
        })}
      </div>
    </div>
  );

  // Summary chips for the list view
  const summarize = (v: VenueSettings) => {
    const instCount = v.instrumentTypes.length;
    const mktCount  = v.marketFeeds.length;
    const ordCount  = v.orderFeeds.length;
    return `${instCount} instr · ${mktCount} mkt feeds · ${ordCount} order feeds`;
  };

  return (
    <div style={{ display: 'flex', gap: '1rem', height: '100%' }}>
      {/* Left: venue list */}
      <div style={{ width: 210, flexShrink: 0 }}>
        <SectionCard style={{ height: '100%' }}>
          <SectionHeader>
            <h3 style={{ fontSize: '0.82rem' }}>Venues</h3>
          </SectionHeader>
          <SectionBody style={{ padding: '0.4rem' }}>
            <div style={{ fontSize: '0.7rem', color: '#4a5568', marginBottom: '0.4rem', padding: '0 0.2rem' }}>
              Select an exchange to configure its market feed and order capabilities.
            </div>
            {EXCHANGES.map(ex => {
              const configured = venues.find(v => v.exchange === ex);
              return (
                <div key={ex}
                  onClick={() => selectVenue(ex)}
                  style={{
                    padding: '0.5rem 0.6rem', cursor: 'pointer', borderRadius: '6px', marginBottom: '0.2rem',
                    background: selEx === ex ? 'rgba(42,171,238,0.12)' : 'transparent',
                    border: `1px solid ${selEx === ex ? '#2aabee44' : 'transparent'}`,
                  }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: '0.4rem' }}>
                    <span style={{ fontSize: '0.82rem', fontWeight: 600, color: '#c8d2dc', textTransform: 'capitalize' }}>{ex}</span>
                    <span style={{ fontSize: '0.6rem', padding: '0.05rem 0.3rem', borderRadius: '3px',
                      background: configured ? 'rgba(51,180,143,0.15)' : 'rgba(255,255,255,0.05)',
                      color: configured ? '#33b48f' : '#4a5568', }}>
                      {configured ? 'configured' : 'default'}
                    </span>
                  </div>
                  {configured && (
                    <div style={{ fontSize: '0.65rem', color: '#4a5568', marginTop: '0.15rem' }}>
                      {summarize(configured)}
                    </div>
                  )}
                </div>
              );
            })}
          </SectionBody>
        </SectionCard>
      </div>

      {/* Right: capability editor */}
      <div style={{ flex: 1, minWidth: 0 }}>
        {!form ? (
          <SectionCard>
            <SectionBody>
              <EmptyState>Select a venue from the left to configure its capabilities.</EmptyState>
            </SectionBody>
          </SectionCard>
        ) : (
          <SectionCard>
            <SectionHeader>
              <h3 style={{ textTransform: 'capitalize' }}>
                {form.exchange} — Venue Capabilities
              </h3>
              <Btn $variant="ghost" onClick={handleReset} style={{ fontSize: '0.75rem' }}>
                Reset to defaults
              </Btn>
            </SectionHeader>
            <SectionBody>
              <ChipGroup title="Instrument Types" items={VENUE_INSTRUMENT_TYPES} field="instrumentTypes" />
              <Divider />
              <ChipGroup title="Market Feed Capabilities" items={VENUE_MARKET_FEEDS} field="marketFeeds" />
              <Divider />
              <ChipGroup title="Order Feed Capabilities" items={VENUE_ORDER_FEEDS} field="orderFeeds" />
              <Divider />
              <div style={{ marginBottom: '0.6rem' }}>
                <Label>Notes</Label>
                <Textarea
                  value={form.notes}
                  placeholder="Any notes about this venue configuration…"
                  onChange={e => setForm(f => f ? { ...f, notes: e.target.value } : f)}
                  style={{ width: '100%', marginTop: '0.25rem' }}
                />
              </div>
              <ButtonRow>
                <Btn $variant="primary" onClick={handleSave} disabled={saving}>
                  {saving ? 'Saving…' : `Save ${form.exchange} Settings`}
                </Btn>
              </ButtonRow>
              <SaveBanner $visible={saved}>✓ Saved</SaveBanner>
            </SectionBody>
          </SectionCard>
        )}
      </div>
    </div>
  );
};

// ── Agg Book Settings Tab ──────────────────────────────────────────────────

const INSTRUMENT_KINDS = [
  { value: 'perpetual_linear',  label: 'Perpetual (Linear / USDT-margined)' },
  { value: 'perpetual_inverse', label: 'Perpetual (Inverse / Coin-margined)' },
  { value: 'future',  label: 'Future' },
  { value: 'spot',    label: 'Spot' },
  { value: 'option',  label: 'Option' },
];

function makeEmptyAggConfig(): AggBookConfig {
  return {
    id: uuidv4(),
    name: '',
    baseSymbol: 'BTC',
    instrumentKind: 'perpetual_linear',
    accountIds: [],
    unifyQuote: false,
    maxLevels: 20,
    tickSize: null,
    pollIntervalMs: 500,
    active: true,
  };
}

const AggBookTab: FunctionComponent = () => {
  const dispatch  = useAppDispatch();
  const configs   = useAppSelector(selectAggBookConfigs);
  const accounts  = useAppSelector(selectAccounts);
  const [selected, setSelected] = useState<AggBookConfig | null>(null);
  const [saving, setSaving]     = useState(false);
  const [saved, setSaved]       = useState(false);
  const [error, setError]       = useState<string | null>(null);

  useEffect(() => {
    invoke<AggBookConfig[]>('get_agg_book_configs')
      .then(cs => dispatch(setAggConfigs(cs)))
      .catch(console.error);
  }, []);

  const handleNew = () => {
    setSelected(makeEmptyAggConfig());
    setError(null);
  };

  const handleSelect = (cfg: AggBookConfig) => {
    setSelected({ ...cfg });
    setError(null);
  };

  const handleSave = async () => {
    if (!selected) return;
    if (!selected.name.trim()) { setError('Name is required'); return; }
    setSaving(true);
    setError(null);
    try {
      const saved = await invoke<AggBookConfig>('save_agg_book_config', { config: selected });
      dispatch(upsertAggConfig(saved));
      setSelected(saved);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e: any) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!selected?.id || !confirm('Delete this Agg Book config?')) return;
    await invoke('delete_agg_book_config', { id: selected.id }).catch(console.error);
    dispatch(removeAggConfig(selected.id));
    setSelected(null);
  };

  const upd = (patch: Partial<AggBookConfig>) =>
    setSelected(s => s ? { ...s, ...patch } : s);

  const toggleAccount = (id: string) => {
    if (!selected) return;
    const ids = selected.accountIds.includes(id)
      ? selected.accountIds.filter(x => x !== id)
      : [...selected.accountIds, id];
    upd({ accountIds: ids });
  };

  return (
    <div style={{ display: 'flex', gap: '1rem', height: '100%' }}>
      {/* Left: config list */}
      <div style={{ width: 220, flexShrink: 0 }}>
        <SectionCard style={{ height: '100%' }}>
          <SectionHeader>
            <h3 style={{ fontSize: '0.82rem' }}>Agg Books</h3>
            <Btn $variant="primary" onClick={handleNew} style={{ fontSize: '0.75rem', padding: '0.2rem 0.6rem' }}>
              + New
            </Btn>
          </SectionHeader>
          <SectionBody style={{ padding: '0.4rem' }}>
            {configs.length === 0 && (
              <EmptyState style={{ padding: '1rem', fontSize: '0.75rem' }}>No configs yet.</EmptyState>
            )}
            {configs.map(cfg => (
              <div
                key={cfg.id}
                onClick={() => handleSelect(cfg)}
                style={{
                  padding: '0.5rem 0.6rem', cursor: 'pointer', borderRadius: '6px', marginBottom: '0.2rem',
                  background: selected?.id === cfg.id ? 'rgba(80,135,242,0.12)' : 'transparent',
                  border: `1px solid ${selected?.id === cfg.id ? '#5087f244' : 'transparent'}`,
                }}
              >
                <div style={{ fontWeight: 600, color: '#c8d2dc', fontSize: '0.82rem' }}>{cfg.name}</div>
                <div style={{ fontSize: '0.68rem', color: '#4a5568', marginTop: '0.1rem' }}>
                  {cfg.baseSymbol} · {cfg.instrumentKind}
                </div>
                {!cfg.active && (
                  <div style={{ fontSize: '0.65rem', color: '#d0616e' }}>inactive</div>
                )}
              </div>
            ))}
          </SectionBody>
        </SectionCard>
      </div>

      {/* Right: form */}
      <div style={{ flex: 1, minWidth: 0 }}>
        {!selected ? (
          <SectionCard>
            <SectionBody>
              <EmptyState>Select a config or click + New to create one.</EmptyState>
            </SectionBody>
          </SectionCard>
        ) : (
          <SectionCard>
            <SectionHeader>
              <h3>{selected.id && configs.some(c => c.id === selected.id) ? 'Edit Config' : 'New Config'}</h3>
            </SectionHeader>
            <SectionBody>
              <FormGrid>
                <FormGroup $span={2}>
                  <Label>Name</Label>
                  <Input value={selected.name} placeholder="e.g. BTC Perps Aggregated"
                    onChange={e => upd({ name: e.target.value })} />
                </FormGroup>

                <FormGroup>
                  <Label>Base Symbol</Label>
                  <Input value={selected.baseSymbol} placeholder="BTC"
                    onChange={e => upd({ baseSymbol: e.target.value.toUpperCase() })} />
                </FormGroup>

                <FormGroup>
                  <Label>Instrument Kind</Label>
                  <Select value={selected.instrumentKind}
                    onChange={e => upd({ instrumentKind: e.target.value })}>
                    {INSTRUMENT_KINDS.map(k => (
                      <option key={k.value} value={k.value}>{k.label}</option>
                    ))}
                  </Select>
                </FormGroup>

                <FormGroup>
                  <Label>Max Levels (1–100)</Label>
                  <Input type="number" min={1} max={100} value={selected.maxLevels}
                    onChange={e => upd({ maxLevels: Math.max(1, Math.min(100, Number(e.target.value))) })} />
                </FormGroup>

                <FormGroup>
                  <Label>Poll Interval (ms, min 200)</Label>
                  <Input type="number" min={200} value={selected.pollIntervalMs}
                    onChange={e => upd({ pollIntervalMs: Math.max(200, Number(e.target.value)) })} />
                </FormGroup>

                <FormGroup $span={2}>
                  <Label>Tick Size (optional price grouping)</Label>
                  <Input type="number" step="any" min={0}
                    value={selected.tickSize ?? ''}
                    placeholder="Leave empty for native tick"
                    onChange={e => upd({ tickSize: e.target.value === '' ? null : Number(e.target.value) })} />
                </FormGroup>

                <FormGroup $span={2}>
                  <CheckboxGroup>
                    <input type="checkbox" checked={selected.unifyQuote}
                      onChange={e => upd({ unifyQuote: e.target.checked })} />
                    Unify Quote — treat USD and USDT as equivalent (for options / futures)
                  </CheckboxGroup>
                </FormGroup>

                <FormGroup $span={2}>
                  <CheckboxGroup>
                    <input type="checkbox" checked={selected.active}
                      onChange={e => upd({ active: e.target.checked })} />
                    Active (start polling when saved)
                  </CheckboxGroup>
                </FormGroup>
              </FormGrid>

              <Divider />

              <Label style={{ marginBottom: '0.5rem', display: 'block' }}>Accounts to aggregate</Label>
              {accounts.length === 0 && (
                <p style={{ fontSize: '0.78rem', color: '#4a5568' }}>No accounts configured.</p>
              )}
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: '0.5rem', marginBottom: '1rem' }}>
                {accounts.map(acc => {
                  const checked = selected.accountIds.includes(acc.id);
                  return (
                    <label key={acc.id} style={{
                      display: 'flex', alignItems: 'center', gap: '0.35rem', cursor: 'pointer',
                      padding: '0.3rem 0.6rem', borderRadius: '5px', fontSize: '0.8rem',
                      background: checked ? 'rgba(80,135,242,0.12)' : 'rgba(255,255,255,0.04)',
                      border: `1px solid ${checked ? '#5087f244' : 'rgba(255,255,255,0.08)'}`,
                      color: checked ? '#c8d6e5' : '#7e8b99',
                    }}>
                      <input type="checkbox" checked={checked} onChange={() => toggleAccount(acc.id)} />
                      {acc.name}
                      <span style={{ fontSize: '0.68rem', color: '#4a5568' }}>({acc.exchange})</span>
                    </label>
                  );
                })}
              </div>

              {error && (
                <p style={{ color: '#d0616e', fontSize: '0.78rem', marginBottom: '0.5rem' }}>✗ {error}</p>
              )}

              <ButtonRow>
                <Btn $variant="primary" onClick={handleSave} disabled={saving}>
                  {saving ? 'Saving…' : 'Save Config'}
                </Btn>
                {configs.some(c => c.id === selected.id) && (
                  <Btn $variant="danger" onClick={handleDelete}>Delete</Btn>
                )}
              </ButtonRow>
              <SaveBanner $visible={saved}>✓ Config saved</SaveBanner>
            </SectionBody>
          </SectionCard>
        )}
      </div>
    </div>
  );
};

// ── RFQ Settings Tab ───────────────────────────────────────────────────────

const PRICER_EXCHANGES = [
  { value: 'deribit',  label: 'Deribit' },
  { value: 'okx',      label: 'OKX' },
  { value: 'bybit',    label: 'Bybit' },
  { value: 'coincall', label: 'CoInCall' },
] as const;

const RfqSettingsTab: FunctionComponent = () => {
  const dispatch = useAppDispatch();
  const rfq      = useAppSelector(selectRfqSettings);
  const [saved, setSaved]   = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    invoke<RfqSettings>('get_rfq_settings')
      .then(s => dispatch(setRfqSettings(s)))
      .catch(console.error);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const upd = (patch: Partial<RfqSettings>) => dispatch(setRfqSettings(patch));

  const handleSave = async () => {
    setSaving(true);
    try {
      await invoke('save_rfq_settings', { settings: rfq });
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) { console.error(e); }
    finally { setSaving(false); }
  };

  return (
    <>
      <SectionCard>
        <SectionHeader><h3>Black-Scholes Defaults</h3></SectionHeader>
        <SectionBody>
          <p style={{ fontSize: '0.8rem', color: '#7e8b99', marginBottom: '0.75rem' }}>
            These values are used in the RFQ Pricer panel.
          </p>
          <FormGrid>
            <FormGroup>
              <Label>Risk-Free Rate (%)</Label>
              <Input
                type="number" step="0.01" min="0" max="50"
                value={(rfq.riskFreeRate * 100).toFixed(2)}
                onChange={e => upd({ riskFreeRate: Math.max(0, parseFloat(e.target.value) || 0) / 100 })}
              />
              <p style={{ fontSize: '0.72rem', color: '#4a5568', margin: '0.2rem 0 0' }}>
                Annual rate (e.g. 5 = 5% p.a.)
              </p>
            </FormGroup>
            <FormGroup>
              <Label>Default Implied Vol (%)</Label>
              <Input
                type="number" step="1" min="1" max="2000"
                value={(rfq.defaultVol * 100).toFixed(0)}
                onChange={e => upd({ defaultVol: Math.max(0.001, parseFloat(e.target.value) || 80) / 100 })}
              />
              <p style={{ fontSize: '0.72rem', color: '#4a5568', margin: '0.2rem 0 0' }}>
                Fallback when market IV is unavailable (e.g. 80 = 80%)
              </p>
            </FormGroup>
          </FormGrid>
        </SectionBody>
      </SectionCard>

      <SectionCard>
        <SectionHeader><h3>Market Data Sources</h3></SectionHeader>
        <SectionBody>
          <p style={{ fontSize: '0.8rem', color: '#7e8b99', marginBottom: '0.75rem' }}>
            Select which exchange to use when auto-fetching spot/index prices and implied volatility in the RFQ Pricer.
            Both spot price and mark IV are pulled from the same exchange's ticker. These use public (unauthenticated) endpoints.
          </p>
          <FormGrid>
            <FormGroup>
              <Label>Pricer Exchange</Label>
              <Select
                value={rfq.pricerExchange}
                onChange={e => upd({ pricerExchange: e.target.value as RfqSettings['pricerExchange'] })}
              >
                {PRICER_EXCHANGES.map(ex => (
                  <option key={ex.value} value={ex.value}>{ex.label}</option>
                ))}
              </Select>
              <p style={{ fontSize: '0.72rem', color: '#4a5568', margin: '0.2rem 0 0' }}>
                Spot and vol are both fetched from this exchange's ticker (e.g. Deribit: BTC-PERPETUAL + mark IV).
              </p>
            </FormGroup>
            <FormGroup>
              <Label>Trading Coin</Label>
              <Select
                value={rfq.tradingCoin ?? 'BTC'}
                onChange={e => upd({ tradingCoin: e.target.value.toUpperCase() })}
              >
                {['BTC', 'ETH', 'SOL', 'XRP', 'BNB'].map(c => (
                  <option key={c} value={c}>{c}</option>
                ))}
              </Select>
              <p style={{ fontSize: '0.72rem', color: '#4a5568', margin: '0.2rem 0 0' }}>
                Only RFQ seeks containing this coin will be shown. Balance delta uses this coin's spot price.
              </p>
            </FormGroup>
          </FormGrid>
        </SectionBody>
      </SectionCard>

      <SectionCard>
        <SectionHeader><h3>Greek-Aware Spread</h3></SectionHeader>
        <SectionBody>
          <p style={{ fontSize: '0.8rem', color: '#7e8b99', marginBottom: '0.75rem' }}>
            Controls how your portfolio Greeks skew quotes. When you're long gamma, the pricer will shade prices higher
            for trades that increase long gamma and lower for trades that reduce it — and vice versa for short gamma.
          </p>
          <FormGrid>
            <FormGroup>
              <Label>Base Spread (%)</Label>
              <Input
                type="number" step="0.1" min="0" max="50"
                value={(rfq.baseSpread * 100).toFixed(2)}
                onChange={e => upd({ baseSpread: Math.max(0, parseFloat(e.target.value) || 1) / 100 })}
              />
              <p style={{ fontSize: '0.72rem', color: '#4a5568', margin: '0.2rem 0 0' }}>
                Minimum half-spread each side of mid (e.g. 1.0 = ±1%)
              </p>
            </FormGroup>
            <FormGroup>
              <Label>Max Skew (%)</Label>
              <Input
                type="number" step="0.5" min="0" max="50"
                value={(rfq.maxSkew * 100).toFixed(1)}
                onChange={e => upd({ maxSkew: Math.max(0, parseFloat(e.target.value) || 5) / 100 })}
              />
              <p style={{ fontSize: '0.72rem', color: '#4a5568', margin: '0.2rem 0 0' }}>
                Cap on Greek-based skew (e.g. 5.0 = ±5% of mid)
              </p>
            </FormGroup>
            <FormGroup>
              <Label>Gamma Sensitivity</Label>
              <Input
                type="number" step="0.05" min="0" max="10"
                value={(rfq.gammaSensitivity).toFixed(3)}
                onChange={e => upd({ gammaSensitivity: Math.max(0, parseFloat(e.target.value) || 0) })}
              />
              <p style={{ fontSize: '0.72rem', color: '#4a5568', margin: '0.2rem 0 0' }}>
                Higher = gamma imbalance skews price more aggressively
              </p>
            </FormGroup>
            <FormGroup>
              <Label>Vega Sensitivity</Label>
              <Input
                type="number" step="0.00005" min="0" max="1"
                value={(rfq.vegaSensitivity).toFixed(4)}
                onChange={e => upd({ vegaSensitivity: Math.max(0, parseFloat(e.target.value) || 0) })}
              />
              <p style={{ fontSize: '0.72rem', color: '#4a5568', margin: '0.2rem 0 0' }}>
                Higher = vega imbalance skews price more aggressively
              </p>
            </FormGroup>
          </FormGrid>
        </SectionBody>
      </SectionCard>

      <SectionCard>
        <SectionHeader><h3>Auto-Quote</h3></SectionHeader>
        <SectionBody>
          <p style={{ fontSize: '0.8rem', color: '#7e8b99', marginBottom: '0.75rem' }}>
            When enabled, the pricer automatically prices and submits a quote for every new
            incoming RFQ seek matching your trading coin. Quotes are auto-cancelled after the timeout.
          </p>
          <FormGrid>
            <FormGroup>
              <Label>Enable Auto-Quote</Label>
              <CheckboxGroup>
                <input
                  type="checkbox"
                  id="autoQuote"
                  checked={rfq.autoQuote ?? false}
                  onChange={e => upd({ autoQuote: e.target.checked })}
                />
                <label htmlFor="autoQuote" style={{ fontSize: '0.85rem', color: '#c8d6e5' }}>
                  Auto-submit quote on new incoming seek
                </label>
              </CheckboxGroup>
            </FormGroup>
            <FormGroup>
              <Label>Quote Timeout (seconds)</Label>
              <Input
                type="number" step="1" min="5" max="300"
                value={rfq.autoQuoteTimeoutSecs ?? 30}
                onChange={e => upd({ autoQuoteTimeoutSecs: Math.max(5, parseInt(e.target.value) || 30) })}
              />
              <p style={{ fontSize: '0.72rem', color: '#4a5568', margin: '0.2rem 0 0' }}>
                Auto-cancel submitted quote after this many seconds
              </p>
            </FormGroup>
          </FormGrid>
        </SectionBody>
      </SectionCard>

      <ButtonRow>
        <Btn $variant="primary" onClick={handleSave} disabled={saving}>
          {saving ? 'Saving…' : 'Save RFQ Settings'}
        </Btn>
      </ButtonRow>
      <SaveBanner $visible={saved}>✓ Saved</SaveBanner>
    </>
  );
};

// ── Root Settings component ────────────────────────────────────────────────
const Settings: FunctionComponent = () => {
  const [tab, setTab] = useState<SettingsTab>('exchange');

  return (
    <SettingsContainer>
      <TabBar>
        <TabBtn $active={tab === 'general'} onClick={() => setTab('general')}>General</TabBtn>
        <TabBtn $active={tab === 'exchange'} onClick={() => setTab('exchange')}>Exchange</TabBtn>
        <TabBtn $active={tab === 'venue'} onClick={() => setTab('venue')}>🏛 Venues</TabBtn>
        <TabBtn $active={tab === 'client'} onClick={() => setTab('client')}>Clients</TabBtn>
        <TabBtn $active={tab === 'tags'} onClick={() => setTab('tags')}>🏷 Tags</TabBtn>
        <TabBtn $active={tab === 'telegram'} onClick={() => setTab('telegram')}>🤖 Telegram</TabBtn>
        <TabBtn $active={tab === 'aggbook'} onClick={() => setTab('aggbook')}>📚 Agg Book</TabBtn>
        <TabBtn $active={tab === 'rfq'} onClick={() => setTab('rfq')}>📊 RFQ Pricer</TabBtn>
      </TabBar>
      <TabContent>
        {tab === 'general'  && <GeneralTab />}
        {tab === 'exchange' && <ExchangeTab />}
        {tab === 'venue'    && <VenueTab />}
        {tab === 'client'   && <ClientTab />}
        {tab === 'tags'     && <TagsTab />}
        {tab === 'telegram' && <TelegramTab />}
        {tab === 'aggbook'  && <AggBookTab />}
        {tab === 'rfq'      && <RfqSettingsTab />}
      </TabContent>
    </SettingsContainer>
  );
};

export default Settings;

