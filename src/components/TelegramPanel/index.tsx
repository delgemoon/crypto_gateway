import { FunctionComponent, useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import styled from 'styled-components';
import { useAppSelector } from '../../hooks';
import { selectTelegram, selectTags, selectClients, TelegramChat, Tag, Client } from '../Settings/settingsSlice';

// ── Types ──────────────────────────────────────────────────────────────────

interface BroadcastAttachment { kind: 'photo' | 'document'; filePath: string; caption: string; }
interface BroadcastPart { id: string; broadcastId: string; partType: string; filePath: string; caption: string; sortOrder: number; }
interface BroadcastSend { id: string; broadcastId: string; partId: string; chatId: number; clientName: string; status: 'pending' | 'sent' | 'failed'; errorMsg: string; messageId?: number; attemptCount: number; lastAttempt: number; }
interface Broadcast { id: string; subject: string; textBody: string; parseMode: string; recipType: string; recipValue: string; status: string; createdAt: number; total: number; sent: number; failed: number; }
interface BroadcastDetail { broadcast: Broadcast; parts: BroadcastPart[]; sends: BroadcastSend[]; }

// ── Styled ─────────────────────────────────────────────────────────────────

const Panel = styled.div`
  display: flex;
  flex-direction: column;
  height: 100%;
  background: #0d1117;
  overflow: hidden;
`;

const Header = styled.div`
  padding: 0.6rem 0.8rem;
  border-bottom: 1px solid #1e2738;
  background: #0f1522;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 0.5rem;
  h4 { margin: 0; color: #d9dde4; font-size: 0.9rem; flex: 1; }
`;

const TgBadge = styled.span`
  padding: 0.1rem 0.4rem;
  border-radius: 3px;
  font-size: 0.68rem;
  font-weight: 700;
  color: #2aabee;
  background: #2aabee22;
`;

const Body = styled.div`
  flex: 1;
  overflow-y: auto;
  padding: 0.75rem;
  display: flex;
  flex-direction: column;
  gap: 0.55rem;
  &::-webkit-scrollbar { width: 4px; }
  &::-webkit-scrollbar-thumb { background: #1e2738; border-radius: 2px; }
`;

const Row = styled.div`
  display: flex;
  gap: 0.4rem;
  align-items: flex-end;
`;

const FG = styled.div`
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex: 1;
  min-width: 0;
`;

const Lbl = styled.div`
  color: #4a5568;
  font-size: 0.6rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
`;

const Inp = styled.input`
  background: #141a28;
  border: 1px solid #29303e;
  color: #e8edf4;
  padding: 0.3rem 0.4rem;
  border-radius: 3px;
  font-size: 0.8rem;
  width: 100%;
  &:focus { border-color: #2aabee; outline: none; }
`;

const TA = styled.textarea`
  background: #141a28;
  border: 1px solid #29303e;
  color: #e8edf4;
  padding: 0.35rem 0.4rem;
  border-radius: 3px;
  font-size: 0.8rem;
  width: 100%;
  min-height: 85px;
  resize: vertical;
  font-family: inherit;
  &:focus { border-color: #2aabee; outline: none; }
`;

const Sel = styled.select`
  background: #141a28;
  border: 1px solid #29303e;
  color: #e8edf4;
  padding: 0.3rem 0.35rem;
  border-radius: 3px;
  font-size: 0.78rem;
  width: 100%;
  &:focus { border-color: #2aabee; outline: none; }
`;

const Btn = styled.button<{ $v?: 'primary' | 'ghost' | 'danger' }>`
  padding: 0.32rem 0.6rem;
  border-radius: 3px;
  font-size: 0.78rem;
  font-weight: 600;
  cursor: pointer;
  border: 1px solid transparent;
  white-space: nowrap;
  transition: opacity 0.15s;
  flex-shrink: 0;
  &:hover { opacity: 0.82; }
  &:disabled { opacity: 0.4; cursor: not-allowed; }
  ${p => p.$v === 'primary'  && `background:#2aabee22;border-color:#2aabee55;color:#2aabee;`}
  ${p => p.$v === 'ghost'    && `background:transparent;border-color:#29303e;color:#7e8b99;`}
  ${p => p.$v === 'danger'   && `background:#d0616e22;border-color:#d0616e55;color:#d0616e;`}
  ${p => !p.$v               && `background:#1e3a6e;border-color:#2a4a8a;color:#5087f2;`}
`;

const ChatItem = styled.div<{ $active: boolean }>`
  padding: 0.28rem 0.4rem;
  border-radius: 3px;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 0.35rem;
  background: ${p => p.$active ? '#1a2a44' : 'transparent'};
  border: 1px solid ${p => p.$active ? '#2a4a8a' : 'transparent'};
  &:hover { background: #141a28; }
  .name { color: #d9dde4; font-size: 0.78rem; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cid  { color: #4a5568; font-size: 0.68rem; font-family: monospace; flex-shrink: 0; }
  .typ  { color: #2aabee; font-size: 0.6rem; background: #2aabee18; padding: 0.04rem 0.28rem; border-radius: 2px; flex-shrink: 0; }
`;

const TypeToggle = styled.div`
  display: flex;
  gap: 3px;
`;

const TBtn = styled.button<{ $a: boolean }>`
  flex: 1;
  padding: 0.26rem 0.35rem;
  border-radius: 3px;
  font-size: 0.74rem;
  font-weight: 500;
  cursor: pointer;
  border: 1px solid #29303e;
  background: ${p => p.$a ? '#1e2a3e' : 'transparent'};
  color: ${p => p.$a ? '#2aabee' : '#4a5568'};
  transition: all 0.1s;
`;

const Status = styled.div<{ $ok: boolean }>`
  padding: 0.32rem 0.45rem;
  border-radius: 3px;
  font-size: 0.78rem;
  background: ${p => p.$ok ? '#33b48f22' : '#d0616e22'};
  border: 1px solid ${p => p.$ok ? '#33b48f44' : '#d0616e44'};
  color: ${p => p.$ok ? '#33b48f' : '#d0616e'};
`;

const SecTitle = styled.div`
  color: #4a5568;
  font-size: 0.6rem;
  text-transform: uppercase;
  letter-spacing: 0.06em;
`;

const HR = styled.div`
  border-top: 1px solid #1e2738;
`;

const TabBar = styled.div`display:flex;gap:2px;padding:0.4rem 0.6rem;background:#0f1522;border-bottom:1px solid #1e2738;`;
const Tab = styled.button<{ $a: boolean }>`padding:0.28rem 0.7rem;border-radius:3px;font-size:0.78rem;font-weight:500;cursor:pointer;border:1px solid ${p => p.$a ? '#2aabee44' : '#1e2738'};background:${p => p.$a ? '#1a2a44' : 'transparent'};color:${p => p.$a ? '#2aabee' : '#4a5568'};transition:all 0.1s;&:hover{color:#7e8b99;}`;
const AttachList = styled.div`display:flex;flex-direction:column;gap:4px;`;
const AttachRow = styled.div`display:flex;align-items:center;gap:0.4rem;padding:0.28rem 0.4rem;background:#141a28;border:1px solid #1e2738;border-radius:3px;font-size:0.78rem;.kind{color:#2aabee;font-size:0.68rem;background:#2aabee18;padding:0.05rem 0.25rem;border-radius:2px;}.name{flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:#d9dde4;}`;
const BroadcastCard = styled.div`background:#131c2e;border:1px solid #1e2738;border-radius:5px;overflow:hidden;margin-bottom:0.5rem;`;
const BCardHeader = styled.div`display:flex;align-items:center;gap:0.5rem;padding:0.45rem 0.6rem;background:#0f1522;cursor:pointer;&:hover{background:#141a28;}`;
const BCardBody = styled.div`padding:0.5rem 0.6rem;`;
const Badge = styled.span<{ $c: string }>`font-size:0.68rem;padding:0.05rem 0.35rem;border-radius:2px;background:${p => p.$c}22;color:${p => p.$c};border:1px solid ${p => p.$c}44;`;

const ClientBlock = styled.div<{ $status: 'sent' | 'failed' | 'pending' | 'mixed' }>`
  border-radius: 4px;
  border: 1px solid ${p =>
    p.$status === 'sent'   ? '#33b48f33' :
    p.$status === 'failed' ? '#e0525233' :
    p.$status === 'mixed'  ? '#e0b94a33' :
    '#1e273844'};
  background: ${p =>
    p.$status === 'sent'   ? '#33b48f08' :
    p.$status === 'failed' ? '#e0525208' :
    p.$status === 'mixed'  ? '#e0b94a08' :
    '#0f152208'};
  margin-bottom: 0.35rem;
  overflow: hidden;
`;

const ClientBlockHeader = styled.div`
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.3rem 0.5rem;
  background: rgba(0,0,0,0.18);
`;

const ClientName = styled.span`
  font-size: 0.78rem;
  font-weight: 600;
  color: #d9dde4;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
`;

const ClientStatusBadge = styled.span<{ $s: 'sent' | 'failed' | 'pending' | 'mixed' }>`
  font-size: 0.65rem;
  font-weight: 700;
  padding: 0.1rem 0.35rem;
  border-radius: 3px;
  letter-spacing: 0.04em;
  background: ${p =>
    p.$s === 'sent'   ? '#33b48f22' :
    p.$s === 'failed' ? '#e0525222' :
    p.$s === 'mixed'  ? '#e0b94a22' :
    '#1e273822'};
  color: ${p =>
    p.$s === 'sent'   ? '#33b48f' :
    p.$s === 'failed' ? '#e05252' :
    p.$s === 'mixed'  ? '#e0b94a' :
    '#7e8b99'};
`;

const PartRow = styled.div<{ $s: string }>`
  display: flex;
  align-items: center;
  gap: 0.4rem;
  padding: 0.2rem 0.5rem 0.2rem 1rem;
  font-size: 0.73rem;
  border-top: 1px solid rgba(255,255,255,0.04);
  background: ${p => p.$s === 'failed' ? 'rgba(224,82,82,0.05)' : 'transparent'};
`;

const PartStatusDot = styled.span<{ $s: string }>`
  width: 6px; height: 6px; border-radius: 50%; flex-shrink: 0;
  background: ${p =>
    p.$s === 'sent'    ? '#33b48f' :
    p.$s === 'failed'  ? '#e05252' :
    '#4a5568'};
`;

const PartLabel = styled.span`flex: 1; color: #8ba0b8; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;`;
const PartErr   = styled.span`color: #e05252; font-size: 0.68rem; flex: 2; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;`;
const PartAttempts = styled.span`color: #4a5568; font-size: 0.65rem; flex-shrink: 0;`;

// ── Component ──────────────────────────────────────────────────────────────

type PanelTab = 'quick' | 'compose' | 'history';

const TelegramPanel: FunctionComponent = () => {
  const tgCfg   = useAppSelector(selectTelegram);
  const tags    = useAppSelector(selectTags);
  const clients = useAppSelector(selectClients);
  const [tab, setTab] = useState<PanelTab>('quick');

  const [knownChats, setKnownChats] = useState<TelegramChat[]>([]);

  // Quick Send state
  const [chatId, setChatId]         = useState('');
  const [resolveRef, setResolveRef] = useState('');
  const [syncing, setSyncing]       = useState(false);
  const [text, setText]             = useState('');
  const [parseMode, setParseMode]   = useState('HTML');
  const [noPreview, setNoPreview]   = useState(false);
  const [qAttachments, setQAttachments] = useState<BroadcastAttachment[]>([]);
  const [quickStatus, setQuickStatus] = useState<{ ok: boolean; msg: string } | null>(null);
  const [busy, setBusy]             = useState(false);

  // Compose state
  const [cSubject, setCSubject]         = useState('');
  const [cText, setCText]               = useState('');
  const [cParseMode, setCParseMode]     = useState('HTML');
  const [cAttachments, setCAttachments] = useState<BroadcastAttachment[]>([]);
  const [cRecipType, setCRecipType]     = useState<'group' | 'clients' | 'tag'>('group');
  const [cGroupId, setCGroupId]         = useState('');
  const [cClientIds, setCClientIds]     = useState<string[]>([]);
  const [cTagId, setCTagId]             = useState('');
  const [composeStatus, setComposeStatus] = useState<{ ok: boolean; msg: string } | null>(null);
  const [composeBusy, setComposeBusy]   = useState(false);

  // History state
  const [broadcasts, setBroadcasts]   = useState<Broadcast[]>([]);
  const [expanded, setExpanded]       = useState<string | null>(null);
  const [detail, setDetail]           = useState<Record<string, BroadcastDetail>>({});
  const [histBusy, setHistBusy]       = useState<Record<string, boolean>>({});
  const [histStatus, setHistStatus]   = useState<Record<string, { ok: boolean; msg: string }>>({});

  const configured = !!tgCfg.botToken;

  useEffect(() => {
    if (!configured) return;
    invoke<TelegramChat[]>('telegram_get_known_chats')
      .then(list => {
        setKnownChats(list);
        if (list.length > 0 && !chatId) setChatId(String(list[0].id));
        if (list.length > 0 && !cGroupId) setCGroupId(String(list[0].id));
      })
      .catch(console.error);
    loadBroadcasts();
  }, [configured]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!configured) return;
    invoke<any[]>('get_tags').then(() => {}).catch(() => {});
    invoke<any[]>('get_clients').then(() => {}).catch(() => {});
  }, [configured]); // eslint-disable-line react-hooks/exhaustive-deps

  const loadBroadcasts = async () => {
    try {
      const list = await invoke<Broadcast[]>('telegram_get_broadcasts');
      setBroadcasts(list);
    } catch {}
  };

  const syncChats = async () => {
    setSyncing(true); setQuickStatus(null);
    try {
      const list = await invoke<TelegramChat[]>('telegram_sync_chats');
      setKnownChats(list);
      setQuickStatus({ ok: true, msg: `${list.length} chat${list.length !== 1 ? 's' : ''} synced` });
    } catch (e: any) { setQuickStatus({ ok: false, msg: String(e) }); }
    finally { setSyncing(false); }
  };

  const resolveChat = async () => {
    if (!resolveRef.trim()) return;
    try {
      const chat = await invoke<TelegramChat>('telegram_resolve_chat', { chatRef: resolveRef.trim() });
      setKnownChats(prev => prev.find(c => c.id === chat.id) ? prev : [...prev, chat]);
      setChatId(String(chat.id));
      setResolveRef('');
      setQuickStatus({ ok: true, msg: `Added: ${chat.title ?? chat.username ?? chat.id}` });
    } catch (e: any) { setQuickStatus({ ok: false, msg: String(e) }); }
  };

  const removeKnownChat = async (id: number) => {
    try {
      await invoke('telegram_delete_known_chat', { chatId: id });
      setKnownChats(prev => prev.filter(c => c.id !== id));
    } catch {}
  };

  const qPickAttachment = async (kind: 'photo' | 'document') => {
    const selected = await openDialog({ multiple: false, directory: false }).catch(() => null);
    if (selected && typeof selected === 'string') {
      setQAttachments(prev => [...prev, { kind, filePath: selected, caption: '' }]);
    }
  };

  const qUpdateCaption = (idx: number, cap: string) =>
    setQAttachments(prev => prev.map((a, i) => i === idx ? { ...a, caption: cap } : a));

  const qRemoveAttachment = (idx: number) =>
    setQAttachments(prev => prev.filter((_, i) => i !== idx));

  const handleQuickSend = async () => {
    if (!chatId.trim()) { setQuickStatus({ ok: false, msg: 'Select a chat first' }); return; }
    if (!text.trim() && qAttachments.length === 0) { setQuickStatus({ ok: false, msg: 'Add text or at least one attachment' }); return; }
    setBusy(true); setQuickStatus(null);
    let sent = 0, failed = 0;
    try {
      // Send text first (if any)
      if (text.trim()) {
        const r: any = await invoke('telegram_send_message', { chatId: chatId.trim(), text, parseMode, disablePreview: noPreview });
        r.ok ? sent++ : failed++;
      }
      // Then each attachment in order
      for (const a of qAttachments) {
        const cmd = a.kind === 'photo' ? 'telegram_send_photo' : 'telegram_send_document';
        const r: any = await invoke(cmd, { chatId: chatId.trim(), filePath: a.filePath, caption: a.caption, parseMode });
        r.ok ? sent++ : failed++;
      }
      if (failed === 0) {
        setQuickStatus({ ok: true, msg: `✓ Sent ${sent} part${sent !== 1 ? 's' : ''}` });
        setText(''); setQAttachments([]);
      } else {
        setQuickStatus({ ok: false, msg: `${sent} sent, ${failed} failed` });
      }
    } catch (e: any) { setQuickStatus({ ok: false, msg: String(e) }); }
    finally { setBusy(false); }
  };

  const pickAttachment = async (kind: 'photo' | 'document') => {
    const selected = await openDialog({ multiple: false, directory: false }).catch(() => null);
    if (selected && typeof selected === 'string') {
      setCAttachments(prev => [...prev, { kind, filePath: selected, caption: '' }]);
    }
  };

  const updateAttachCaption = (idx: number, cap: string) => {
    setCAttachments(prev => prev.map((a, i) => i === idx ? { ...a, caption: cap } : a));
  };

  const removeAttachment = (idx: number) => {
    setCAttachments(prev => prev.filter((_, i) => i !== idx));
  };

  const handleCompose = async () => {
    if (!cSubject.trim()) { setComposeStatus({ ok: false, msg: 'Subject is required' }); return; }
    if (!cText.trim() && cAttachments.length === 0) { setComposeStatus({ ok: false, msg: 'Add text or at least one attachment' }); return; }
    const recipValue = cRecipType === 'group' ? cGroupId
      : cRecipType === 'clients' ? cClientIds.join(',')
      : cTagId;
    if (!recipValue) { setComposeStatus({ ok: false, msg: 'Select recipients' }); return; }
    setComposeBusy(true); setComposeStatus(null);
    try {
      const broadcast = await invoke<Broadcast>('telegram_create_broadcast', {
        req: {
          subject: cSubject,
          textBody: cText,
          parseMode: cParseMode,
          recipType: cRecipType,
          recipValue,
          attachments: cAttachments.map(a => ({ kind: a.kind, filePath: a.filePath, caption: a.caption })),
        }
      });
      const [sent, failed] = await invoke<[number, number]>('telegram_send_broadcast', { broadcastId: broadcast.id });
      setComposeStatus({ ok: failed === 0, msg: `Sent ${sent} / ${sent + failed} — ${failed} failed` });
      setCSubject(''); setCText(''); setCAttachments([]); setCClientIds([]);
      await loadBroadcasts();
      setTab('history');
    } catch (e: any) { setComposeStatus({ ok: false, msg: String(e) }); }
    finally { setComposeBusy(false); }
  };

  const loadDetail = async (broadcastId: string) => {
    try {
      const [broadcast, parts, sends] = await invoke<[Broadcast, BroadcastPart[], BroadcastSend[]]>('telegram_get_broadcast_detail', { broadcastId });
      setDetail(prev => ({ ...prev, [broadcastId]: { broadcast, parts, sends } }));
    } catch {}
  };

  const toggleExpand = async (id: string) => {
    if (expanded === id) { setExpanded(null); return; }
    setExpanded(id);
    if (!detail[id]) await loadDetail(id);
  };

  const handleRetry = async (broadcastId: string) => {
    setHistBusy(prev => ({ ...prev, [broadcastId]: true }));
    setHistStatus(prev => ({ ...prev, [broadcastId]: { ok: true, msg: 'Retrying…' } }));
    try {
      const [sent, failed] = await invoke<[number, number]>('telegram_retry_failed', { broadcastId });
      setHistStatus(prev => ({ ...prev, [broadcastId]: { ok: failed === 0, msg: `Sent ${sent} / ${sent + failed} — ${failed} failed` } }));
      await loadBroadcasts();
      await loadDetail(broadcastId);
    } catch (e: any) {
      setHistStatus(prev => ({ ...prev, [broadcastId]: { ok: false, msg: String(e) } }));
    } finally {
      setHistBusy(prev => ({ ...prev, [broadcastId]: false }));
    }
  };

  const handleDeleteBroadcast = async (broadcastId: string) => {
    if (!confirm('Delete this broadcast and all send records?')) return;
    try {
      await invoke('telegram_delete_broadcast', { broadcastId });
      setBroadcasts(prev => prev.filter(b => b.id !== broadcastId));
      setDetail(prev => { const n = { ...prev }; delete n[broadcastId]; return n; });
      if (expanded === broadcastId) setExpanded(null);
    } catch {}
  };

  if (!configured) {
    return (
      <Panel>
        <Header><TgBadge>TG</TgBadge><h4>Telegram</h4></Header>
        <Body style={{ justifyContent: 'center', alignItems: 'center', color: '#4a5568', textAlign: 'center' }}>
          <div style={{ fontSize: '1.5rem' }}>🤖</div>
          <div>Configure your bot token in ⚙ Settings → Telegram</div>
        </Body>
      </Panel>
    );
  }

  const statusColor = (s: string) => s === 'done' ? '#33b48f' : s === 'partial_fail' ? '#e0b94a' : s === 'sending' ? '#2aabee' : '#7e8b99';

  return (
    <Panel>
      <Header><TgBadge>TG</TgBadge><h4>Telegram</h4></Header>
      <TabBar>
        <Tab $a={tab === 'quick'}   onClick={() => setTab('quick')}>✉ Quick</Tab>
        <Tab $a={tab === 'compose'} onClick={() => setTab('compose')}>📢 Compose</Tab>
        <Tab $a={tab === 'history'} onClick={() => setTab('history')}>📋 History {broadcasts.length > 0 ? `(${broadcasts.length})` : ''}</Tab>
      </TabBar>

      {/* ── Quick Send ── */}
      {tab === 'quick' && (
        <Body>
          <SecTitle>Target Chat</SecTitle>
          {knownChats.length > 0 ? (
            <Row>
              <FG>
                <Sel value={chatId} onChange={e => setChatId(e.target.value)}>
                  <option value="">— Select chat —</option>
                  {knownChats.map(c => (
                    <option key={c.id} value={String(c.id)}>
                      {c.kind === 'channel' ? '📢' : '👥'} {c.title ?? c.username ?? String(c.id)} ({c.id})
                    </option>
                  ))}
                </Sel>
              </FG>
              <Btn $v="ghost" onClick={syncChats} disabled={syncing} title="Sync from Telegram">⟳</Btn>
            </Row>
          ) : (
            <Btn $v="primary" onClick={syncChats} disabled={syncing} style={{ width: '100%' }}>
              {syncing ? '⟳ Syncing…' : '⟳ Sync Chats from Telegram'}
            </Btn>
          )}
          <Row>
            <FG>
              <Inp placeholder="Add by @channel or ID…" value={resolveRef} onChange={e => setResolveRef(e.target.value)} onKeyDown={e => e.key === 'Enter' && resolveChat()} />
            </FG>
            <Btn $v="ghost" onClick={resolveChat}>Add</Btn>
          </Row>
          {chatId && knownChats.find(c => String(c.id) === chatId) && (
            <ChatItem $active style={{ cursor: 'default' }}>
              <span className="typ">{knownChats.find(c => String(c.id) === chatId)?.kind}</span>
              <span className="name">{knownChats.find(c => String(c.id) === chatId)?.title ?? '—'}</span>
              <span className="cid">{chatId}</span>
              <Btn $v="danger" onClick={() => removeKnownChat(Number(chatId))} style={{ padding: '0.1rem 0.3rem', fontSize: '0.7rem' }}>✕</Btn>
            </ChatItem>
          )}
          <HR />
          <Row>
            <FG><Lbl>Format</Lbl>
              <Sel value={parseMode} onChange={e => setParseMode(e.target.value)}>
                <option value="HTML">HTML</option>
                <option value="MarkdownV2">MarkdownV2</option>
                <option value="">Plain</option>
              </Sel>
            </FG>
            <div style={{ display: 'flex', alignItems: 'center', gap: '0.3rem', paddingBottom: 2 }}>
              <input type="checkbox" id="np" checked={noPreview} onChange={e => setNoPreview(e.target.checked)} />
              <label htmlFor="np" style={{ color: '#7e8b99', fontSize: '0.72rem', cursor: 'pointer', whiteSpace: 'nowrap' }}>No preview</label>
            </div>
          </Row>
          <FG><Lbl>Message <span style={{ color: '#4a5568', fontWeight: 400 }}>(optional)</span></Lbl>
            <TA placeholder="Message…" value={text} onChange={e => setText(e.target.value)} />
          </FG>
          <SecTitle>Attachments</SecTitle>
          {qAttachments.length > 0 && (
            <AttachList>
              {qAttachments.map((a, i) => (
                <AttachRow key={i}>
                  <span className="kind">{a.kind === 'photo' ? '🖼' : '📎'}</span>
                  <span className="name">{a.filePath.split(/[\\/]/).pop()}</span>
                  <Inp
                    placeholder="Caption…"
                    value={a.caption}
                    onChange={e => qUpdateCaption(i, e.target.value)}
                    style={{ flex: 1, fontSize: '0.72rem', padding: '0.15rem 0.3rem' }}
                  />
                  <Btn $v="danger" onClick={() => qRemoveAttachment(i)} style={{ padding: '0.1rem 0.3rem', fontSize: '0.7rem' }}>✕</Btn>
                </AttachRow>
              ))}
            </AttachList>
          )}
          <Row>
            <Btn $v="ghost" onClick={() => qPickAttachment('photo')} style={{ flex: 1 }}>🖼 Add Photo</Btn>
            <Btn $v="ghost" onClick={() => qPickAttachment('document')} style={{ flex: 1 }}>📎 Add File</Btn>
          </Row>
          {quickStatus && <Status $ok={quickStatus.ok}>{quickStatus.msg}</Status>}
          <Btn $v="primary" onClick={handleQuickSend} disabled={busy || !chatId.trim() || (!text.trim() && qAttachments.length === 0)}>
            {busy ? 'Sending…' : `✈ Send${qAttachments.length > 0 ? ` (${qAttachments.length + (text.trim() ? 1 : 0)} parts)` : ''}`}
          </Btn>
        </Body>
      )}

      {/* ── Compose Broadcast ── */}
      {tab === 'compose' && (
        <Body>
          <SecTitle>Subject</SecTitle>
          <Inp placeholder="Broadcast subject / title…" value={cSubject} onChange={e => setCSubject(e.target.value)} />

          <SecTitle>Message</SecTitle>
          <Row>
            <FG><Lbl>Format</Lbl>
              <Sel value={cParseMode} onChange={e => setCParseMode(e.target.value)}>
                <option value="HTML">HTML</option>
                <option value="MarkdownV2">MarkdownV2</option>
                <option value="">Plain</option>
              </Sel>
            </FG>
          </Row>
          <TA placeholder="Message body (HTML supported)…" value={cText} onChange={e => setCText(e.target.value)} style={{ minHeight: 100 }} />

          <SecTitle>Attachments</SecTitle>
          {cAttachments.length > 0 && (
            <AttachList>
              {cAttachments.map((a, i) => (
                <AttachRow key={i}>
                  <span className="kind">{a.kind}</span>
                  <span className="name">{a.filePath.split(/[\\/]/).pop()}</span>
                  <Inp
                    placeholder="Caption…"
                    value={a.caption}
                    onChange={e => updateAttachCaption(i, e.target.value)}
                    style={{ width: 120, fontSize: '0.72rem', padding: '0.15rem 0.3rem' }}
                  />
                  <Btn $v="danger" onClick={() => removeAttachment(i)} style={{ padding: '0.1rem 0.3rem', fontSize: '0.7rem' }}>✕</Btn>
                </AttachRow>
              ))}
            </AttachList>
          )}
          <Row>
            <Btn $v="ghost" onClick={() => pickAttachment('photo')} style={{ flex: 1 }}>🖼 Add Photo</Btn>
            <Btn $v="ghost" onClick={() => pickAttachment('document')} style={{ flex: 1 }}>📎 Add File</Btn>
          </Row>

          <HR />
          <SecTitle>Recipients</SecTitle>
          <TypeToggle>
            <TBtn $a={cRecipType === 'group'}   onClick={() => setCRecipType('group')}>📢 Group</TBtn>
            <TBtn $a={cRecipType === 'clients'} onClick={() => setCRecipType('clients')}>👤 Clients</TBtn>
            <TBtn $a={cRecipType === 'tag'}     onClick={() => setCRecipType('tag')}>🏷 Tag</TBtn>
          </TypeToggle>

          {cRecipType === 'group' && (
            <FG>
              <Lbl>Select Chat</Lbl>
              <Sel value={cGroupId} onChange={e => setCGroupId(e.target.value)}>
                <option value="">— Select —</option>
                {knownChats.map(c => (
                  <option key={c.id} value={String(c.id)}>
                    {c.kind === 'channel' ? '📢' : '👥'} {c.title ?? c.username ?? String(c.id)}
                  </option>
                ))}
              </Sel>
            </FG>
          )}

          {cRecipType === 'clients' && (
            <FG>
              <Lbl>Select Clients (hold Ctrl/Cmd for multi-select)</Lbl>
              <select
                multiple
                value={cClientIds}
                onChange={e => setCClientIds(Array.from(e.target.selectedOptions, o => o.value))}
                style={{ background: '#141a28', border: '1px solid #29303e', color: '#e8edf4', padding: '0.3rem', borderRadius: 3, fontSize: '0.8rem', minHeight: 80, width: '100%' }}
              >
                {clients.map((c: Client) => (
                  <option key={c.id} value={c.id}>
                    {c.companyName || c.contactName || c.id}
                  </option>
                ))}
              </select>
              {cClientIds.length > 0 && <div style={{ fontSize: '0.72rem', color: '#4a90d9', marginTop: 4 }}>{cClientIds.length} client{cClientIds.length !== 1 ? 's' : ''} selected</div>}
            </FG>
          )}

          {cRecipType === 'tag' && (
            <FG>
              <Lbl>Select Tag</Lbl>
              <Sel value={cTagId} onChange={e => setCTagId(e.target.value)}>
                <option value="">— Select tag —</option>
                {tags.map((t: Tag) => <option key={t.id} value={t.id}>{t.name}</option>)}
              </Sel>
              {cTagId && <div style={{ fontSize: '0.72rem', color: '#7e8b99', marginTop: 4 }}>All clients with this tag will receive the message</div>}
            </FG>
          )}

          {composeStatus && <Status $ok={composeStatus.ok}>{composeStatus.msg}</Status>}

          <Btn $v="primary" onClick={handleCompose} disabled={composeBusy}>
            {composeBusy ? '⟳ Sending…' : '📢 Send Broadcast'}
          </Btn>
        </Body>
      )}

      {/* ── History ── */}
      {tab === 'history' && (
        <Body>
          <Row style={{ marginBottom: '0.25rem' }}>
            <span style={{ fontSize: '0.78rem', color: '#7e8b99', flex: 1 }}>{broadcasts.length} broadcast{broadcasts.length !== 1 ? 's' : ''}</span>
            <Btn $v="ghost" onClick={loadBroadcasts}>↺ Refresh</Btn>
          </Row>

          {broadcasts.length === 0 && (
            <div style={{ textAlign: 'center', color: '#4a5568', padding: '2rem', fontSize: '0.85rem' }}>
              No broadcasts yet. Use the Compose tab to send your first broadcast.
            </div>
          )}

          {broadcasts.map(b => {
            const isExpanded = expanded === b.id;
            const d = detail[b.id];
            const hasFailed = b.failed > 0;
            const sc = statusColor(b.status);
            const date = new Date(b.createdAt * 1000).toLocaleString();
            // Count unique clients from detail if loaded
            const clientCount = d
              ? new Set(d.sends.map(s => s.clientName)).size
              : null;
            const recipLabel = b.recipType === 'clients'
              ? `clients`
              : b.recipType === 'tag'
              ? `tag`
              : b.recipType === 'group'
              ? 'group'
              : b.recipType;
            return (
              <BroadcastCard key={b.id}>
                <BCardHeader onClick={() => toggleExpand(b.id)}>
                  <Badge $c={sc}>{b.status}</Badge>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ color: '#d9dde4', fontSize: '0.82rem', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {b.subject || '(no subject)'}
                    </div>
                    <div style={{ fontSize: '0.68rem', color: '#4a5568', marginTop: '0.1rem' }}>
                      → {clientCount !== null ? `${clientCount} client${clientCount !== 1 ? 's' : ''}` : recipLabel}
                      {' · '}{date}
                    </div>
                  </div>
                  <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-end', gap: 2, flexShrink: 0 }}>
                    <span style={{ fontSize: '0.72rem', color: '#7e8b99' }}>
                      <span style={{ color: '#33b48f' }}>✓{b.sent}</span>
                      {hasFailed && <span style={{ color: '#e05252', marginLeft: 4 }}>✗{b.failed}</span>}
                      <span style={{ marginLeft: 4, color: '#4a5568' }}>/{b.total} parts</span>
                    </span>
                  </div>
                  <span style={{ color: '#4a5568', marginLeft: '0.5rem', flexShrink: 0 }}>{isExpanded ? '▲' : '▼'}</span>
                </BCardHeader>

                {isExpanded && (
                  <BCardBody>
                    <Row style={{ marginBottom: '0.4rem' }}>
                      {hasFailed && (
                        <Btn $v="primary" onClick={() => handleRetry(b.id)} disabled={!!histBusy[b.id]} style={{ fontSize: '0.75rem' }}>
                          {histBusy[b.id] ? '⟳ Retrying…' : '↺ Retry Failed'}
                        </Btn>
                      )}
                      <Btn $v="danger" onClick={() => handleDeleteBroadcast(b.id)} style={{ fontSize: '0.75rem', marginLeft: 'auto' }}>🗑 Delete</Btn>
                    </Row>
                    {histStatus[b.id] && <Status $ok={histStatus[b.id].ok} style={{ marginBottom: '0.4rem', fontSize: '0.75rem' }}>{histStatus[b.id].msg}</Status>}

                    {d ? (
                      <>
                        {(() => {
                          // Group sends by clientName, preserving insertion order
                          const clientMap = new Map<string, { chatId: number; sends: BroadcastSend[] }>();
                          for (const s of d.sends) {
                            if (!clientMap.has(s.clientName)) {
                              clientMap.set(s.clientName, { chatId: s.chatId, sends: [] });
                            }
                            clientMap.get(s.clientName)!.sends.push(s);
                          }

                          return [...clientMap.entries()].map(([clientName, { chatId, sends }]) => {
                            const anyFailed  = sends.some(s => s.status === 'failed');
                            const anyPending = sends.some(s => s.status === 'pending');
                            const allSent    = sends.every(s => s.status === 'sent');
                            const clientStatus: 'sent' | 'failed' | 'pending' | 'mixed' =
                              allSent    ? 'sent'    :
                              anyFailed && anyPending ? 'mixed' :
                              anyFailed  ? 'failed'  :
                              anyPending ? 'pending' : 'sent';

                            const clientStatusLabel =
                              clientStatus === 'sent'    ? '✓ Delivered' :
                              clientStatus === 'failed'  ? '✗ Failed'    :
                              clientStatus === 'mixed'   ? '⚠ Partial'   :
                              '⏳ Pending';

                            return (
                              <ClientBlock key={clientName} $status={clientStatus}>
                                <ClientBlockHeader>
                                  <span style={{ fontSize: '0.7rem', color: '#4a5568' }}>👤</span>
                                  <ClientName title={clientName}>{clientName}</ClientName>
                                  <span style={{ fontSize: '0.65rem', color: '#4a5568', fontFamily: 'monospace' }}>{chatId}</span>
                                  <ClientStatusBadge $s={clientStatus}>{clientStatusLabel}</ClientStatusBadge>
                                </ClientBlockHeader>
                                {sends.map(s => {
                                  const part = d.parts.find(p => p.id === s.partId);
                                  const partLabel = part
                                    ? (part.partType === 'text'
                                        ? '✉ text body'
                                        : `${part.partType === 'photo' ? '🖼' : '📎'} ${part.filePath.split(/[\\/]/).pop() ?? part.partType}`)
                                    : `part ${s.partId.slice(-6)}`;
                                  return (
                                    <PartRow key={s.id} $s={s.status}>
                                      <PartStatusDot $s={s.status} />
                                      <PartLabel title={partLabel}>{partLabel}</PartLabel>
                                      {s.errorMsg
                                        ? <PartErr title={s.errorMsg}>{s.errorMsg}</PartErr>
                                        : s.status === 'sent' && s.messageId
                                          ? <span style={{ color: '#33b48f', fontSize: '0.65rem' }}>msg #{s.messageId}</span>
                                          : null
                                      }
                                      {s.attemptCount > 1 && (
                                        <PartAttempts title={`${s.attemptCount} attempts`}>×{s.attemptCount}</PartAttempts>
                                      )}
                                    </PartRow>
                                  );
                                })}
                              </ClientBlock>
                            );
                          });
                        })()}
                      </>
                    ) : (
                      <div style={{ color: '#4a5568', fontSize: '0.78rem' }}>Loading…</div>
                    )}
                  </BCardBody>
                )}
              </BroadcastCard>
            );
          })}
        </Body>
      )}
    </Panel>
  );
};

export default TelegramPanel;
