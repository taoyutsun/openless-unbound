// History.tsx — 接 Tauri 后端 list_history / delete_history_entry / clear_history。
// 真实数据来自 ~/Library/Application Support/OpenLess Unbound/history.json。

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../components/Icon';
import { detectOS } from '../components/WindowChrome';
import { formatComboLabel } from '../lib/hotkey';
import { clearHistory, deleteHistoryEntry, listHistory, listStylePacks, readAudioRecording, repolish, retranscribeRecording } from '../lib/ipc';
import { defaultPackId, packDisplayName, retryPackId } from '../lib/historyRepolish';
import type { DictationSession, PolishMode, StylePack } from '../lib/types';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { Btn, Card, PageHeader, Pill } from './_atoms';

function useFilters(): Array<{ id: 'all' | PolishMode; label: string }> {
  const { t } = useTranslation();
  return [
    { id: 'all', label: t('history.filterAll') },
    { id: 'raw', label: t('style.modes.raw.name') },
    { id: 'light', label: t('style.modes.light.name') },
    { id: 'structured', label: t('style.modes.structured.name') },
    { id: 'formal', label: t('style.modes.formal.name') },
  ];
}

function useModeLabel(): Record<PolishMode, string> {
  const { t } = useTranslation();
  return {
    raw: t('style.modes.raw.name'),
    light: t('style.modes.light.name'),
    structured: t('style.modes.structured.name'),
    formal: t('style.modes.formal.name'),
  };
}

function styleLabelFor(
  session: DictationSession,
  allPacks: StylePack[] | null,
  modeLabel: Record<PolishMode, string>,
): string {
  const pack = session.stylePackId
    ? allPacks?.find(candidate => candidate.id === session.stylePackId)
    : undefined;
  return pack ? packDisplayName(pack, modeLabel) : modeLabel[session.mode];
}

export function History() {
  const { t } = useTranslation();
  const os = detectOS();
  const FILTERS = useFilters();
  const MODE_LABEL = useModeLabel();
  const [filter, setFilter] = useState<'all' | PolishMode>('all');
  // issue #612：历史页顶部原为静态 div，只显示统计、不可输入。改为真实搜索框。
  const [query, setQuery] = useState('');
  const searchRef = useRef<HTMLInputElement>(null);
  const [items, setItems] = useState<DictationSession[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [justCopied, setJustCopied] = useState(false);
  const [justCopiedRaw, setJustCopiedRaw] = useState(false);
  const [exportedRecordingId, setExportedRecordingId] = useState<string | null>(null);
  const [retranscribing, setRetranscribing] = useState(false);
  // 录音文件 lazily-detected missing 状态：retention / 条数 cap 清理后磁盘上 wav
  // 可能已被删，但 history 条目 hasAudioRecording 仍写 true。任一组件
  // （播放 / 导出）首次 IPC 拿到 'recording not found' 时把 id 加进来，
  // 之后渲染按钮的条件就转 false，避免反复点击得到同样的 error。
  // 修 pr_agent "Missing file check" 反馈。
  const [audioMissingIds, setAudioMissingIds] = useState<Set<string>>(() => new Set());
  const markAudioMissing = useCallback((id: string) => {
    setAudioMissingIds(prev => {
      if (prev.has(id)) return prev;
      const next = new Set(prev);
      next.add(id);
      return next;
    });
  }, []);
  const { prefs } = useHotkeySettings();
  const [allPacks, setAllPacks] = useState<StylePack[] | null>(null);
  const [packsError, setPacksError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const data = await listHistory();
      setItems(data);
      setActionError(null);
      setSelectedId(prev => (prev && data.some(s => s.id === prev) ? prev : data[0]?.id ?? null));
    } catch (error) {
      console.error('[history] failed to load history', error);
      setLoadError(errorMessage(error));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    let cancelled = false;
    listStylePacks()
      .then(packs => {
        if (!cancelled) setAllPacks(packs);
      })
      .catch(error => {
        if (!cancelled) setPacksError(errorMessage(error));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // ⌘K / Ctrl+K 聚焦搜索框（issue #612 验收可选项）。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && (e.key === 'k' || e.key === 'K')) {
        e.preventDefault();
        searchRef.current?.focus();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const filtered = useMemo(() => {
    const byMode = filter === 'all' ? items : items.filter(s => s.mode === filter);
    const q = query.trim().toLowerCase();
    if (!q) return byMode;
    return byMode.filter(
      s =>
        s.finalText.toLowerCase().includes(q) ||
        s.rawTranscript.toLowerCase().includes(q) ||
        (s.appName ?? '').toLowerCase().includes(q),
    );
  }, [items, filter, query]);
  const item = useMemo(
    () => filtered.find(s => s.id === selectedId) || filtered[0],
    [filtered, selectedId],
  );

  const onClear = async () => {
    if (items.length === 0) return;
    if (!confirm(t('history.confirmClear', { count: items.length }))) return;
    setActionError(null);
    try {
      await clearHistory();
      setItems([]);
      setSelectedId(null);
    } catch (error) {
      console.error('[history] failed to clear history', error);
      setActionError(t('history.clearFailed', { err: errorMessage(error) }));
    }
  };

  const onDelete = async () => {
    if (!item) return;
    const deletedId = item.id;
    setActionError(null);
    try {
      await deleteHistoryEntry(deletedId);
      setItems(prev => prev.filter(s => s.id !== deletedId));
      setSelectedId(current => (current === deletedId ? null : current));
    } catch (error) {
      console.error('[history] failed to delete history entry', error);
      setActionError(t('history.deleteFailed', { err: errorMessage(error) }));
    }
  };

  const onCopy = async () => {
    if (!item) return;
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error('clipboard unavailable');
      }
      await navigator.clipboard.writeText(item.finalText.trim() ? item.finalText : item.rawTranscript);
      setActionError(null);
      setJustCopied(true);
      window.setTimeout(() => setJustCopied(false), 1500);
    } catch (error) {
      console.error('[history] failed to copy entry', error);
      setActionError(t('history.copyFailed', { err: errorMessage(error) }));
    }
  };

  const onCopyRaw = async () => {
    if (!item) return;
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error('clipboard unavailable');
      }
      await navigator.clipboard.writeText(item.rawTranscript);
      setActionError(null);
      setJustCopiedRaw(true);
      window.setTimeout(() => setJustCopiedRaw(false), 1500);
    } catch (error) {
      console.error('[history] failed to copy raw transcript', error);
      setActionError(t('history.copyFailed', { err: errorMessage(error) }));
    }
  };

  const onExportAudio = async () => {
    if (!item || !item.hasAudioRecording) return;
    const exportedId = item.id;
    try {
      const bytes = await readAudioRecording(item.id);
      if (bytes.byteLength === 0) throw new Error('empty recording');
      const buffer = new ArrayBuffer(bytes.byteLength);
      new Uint8Array(buffer).set(bytes);
      const blob = new Blob([buffer], { type: 'audio/wav' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `openless-recording-${item.id}.wav`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      // 浏览器异步触发下载，立刻 revoke 偶尔被中断；延后 60s 兜底。
      window.setTimeout(() => URL.revokeObjectURL(url), 60_000);
      setActionError(null);
      setExportedRecordingId(exportedId);
      window.setTimeout(() => {
        setExportedRecordingId(current => (current === exportedId ? null : current));
      }, 1500);
    } catch (error) {
      console.error('[history] failed to export recording', error);
      const msg = errorMessage(error);
      // wav 已被 retention / 条数 cap 清理：把按钮隐藏，不显示错误（用户没干错事）。
      if (msg.includes('recording not found') || msg.includes('not found')) {
        markAudioMissing(item.id);
        return;
      }
      setActionError(t('history.exportFailed', { err: msg }));
    }
  };

  const onRetranscribe = async () => {
    if (!item || !item.hasAudioRecording) return;
    setRetranscribing(true);
    setActionError(null);
    try {
      const updated = await retranscribeRecording(item.id);
      setItems(prev => prev.map(s => (s.id === updated.id ? updated : s)));
    } catch (error) {
      console.error('[history] retranscribe failed', error);
      const msg = errorMessage(error);
      if (msg.includes('recording not found') || msg.includes('not found')) {
        markAudioMissing(item.id);
        return;
      }
      setActionError(t('history.retranscribeFailed', { err: msg }));
    } finally {
      setRetranscribing(false);
    }
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', minHeight: 0 }}>
      <PageHeader
        kicker={t('history.kicker')}
        title={t('history.title')}
        desc={t('history.desc')}
        right={
          <div style={{ display: 'flex', gap: 8 }}>
            <Btn icon="refresh" variant="ghost" size="sm" onClick={() => void refresh()}>{t('common.refresh')}</Btn>
            <Btn icon="trash" variant="ghost" size="sm" onClick={onClear}>{t('common.clear')}</Btn>
          </div>
        }
      />
      <div style={{ display: 'grid', gridTemplateColumns: '300px 1fr', gap: 14, flex: 1, minHeight: 0 }}>
        <Card padding={0} style={{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
          <div style={{ padding: '12px 14px', borderBottom: '0.5px solid var(--ol-line)' }}>
            <div style={{
              display: 'flex', alignItems: 'center', gap: 6,
              padding: '6px 10px',
              border: '0.5px solid var(--ol-line-strong)', borderRadius: 8,
              background: 'var(--ol-surface-2)', color: 'var(--ol-ink-3)',
            }}>
              <Icon name="search" size={12} />
              <input
                ref={searchRef}
                type="search"
                value={query}
                onChange={e => setQuery(e.target.value)}
                placeholder={t('history.searchPlaceholder', { shortcut: os === 'mac' ? '⌘K' : 'Ctrl+K' })}
                style={{
                  flex: 1, minWidth: 0, outline: 'none', border: 0,
                  background: 'transparent', fontSize: 12, color: 'var(--ol-ink-2)',
                  fontFamily: 'inherit',
                }}
              />
            </div>
            <div style={{ marginTop: 8, fontSize: 11, color: 'var(--ol-ink-4)' }}>
              {t('history.summary', { total: items.length, shown: filtered.length })}
            </div>
            <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap', marginTop: 10 }}>
              {FILTERS.map(f => (
                <button
                  key={f.id}
                  onClick={() => setFilter(f.id)}
                  style={{
                    padding: '3px 9px', fontSize: 11, borderRadius: 999,
                    border: '0.5px solid ' + (filter === f.id ? 'var(--ol-ink)' : 'var(--ol-line-strong)'),
                    background: filter === f.id ? 'var(--ol-ink)' : 'transparent',
                    color: filter === f.id ? '#fff' : 'var(--ol-ink-3)',
                    cursor: 'default', fontFamily: 'inherit', fontWeight: 500,
                    transition: 'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
                  }}
                >{f.label}</button>
              ))}
            </div>
          </div>
          <div className="ol-thinscroll" style={{ flex: 1, overflow: 'auto', padding: 6 }}>
            {actionError && (
              <div style={{ margin: 8, padding: '9px 10px', borderRadius: 8, background: 'rgba(239,68,68,0.08)', color: 'var(--ol-red, #ef4444)', fontSize: 12, lineHeight: 1.45 }}>
                {actionError}
              </div>
            )}
            {loading && <div style={{ padding: 16, fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>}
            {!loading && loadError && (
              <div style={{ padding: 16, fontSize: 12, color: 'var(--ol-ink-4)', display: 'flex', flexDirection: 'column', alignItems: 'flex-start', gap: 10 }}>
                <span>{t('history.loadFailed', { err: loadError })}</span>
                <Btn size="sm" variant="ghost" onClick={() => void refresh()}>{t('history.retry')}</Btn>
              </div>
            )}
            {!loading && !loadError && filtered.length === 0 && (
              <div style={{ padding: 16, fontSize: 12, color: 'var(--ol-ink-4)' }}>
                {query.trim()
                  ? t('history.searchNoMatch', { query: query.trim() })
                  : t('history.empty', { trigger: prefs ? formatComboLabel(prefs.dictationHotkey) : '' })}
              </div>
            )}
            {!loadError && filtered.map(s => (
              <button
                key={s.id}
                onClick={() => setSelectedId(s.id)}
                style={{
                  width: '100%', padding: '10px 12px', textAlign: 'left',
                  display: 'flex', flexDirection: 'column', gap: 4,
                  border: 0, borderRadius: 8,
                  background: selectedId === s.id ? 'rgba(37,99,235,0.06)' : 'transparent',
                  boxShadow: selectedId === s.id ? 'inset 2px 0 0 var(--ol-blue)' : 'none',
                  cursor: 'default', fontFamily: 'inherit', marginBottom: 1,
                  transition: 'background 0.16s var(--ol-motion-quick), box-shadow 0.18s var(--ol-motion-soft)',
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 }}>
                  <span style={{ fontSize: 11, fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink-3)' }}>
                    {formatTime(s.createdAt)}
                  </span>
                  <span style={{ fontSize: 10, color: 'var(--ol-ink-4)', fontFamily: 'var(--ol-font-mono)' }}>
                    {formatDuration(s.durationMs, t)}
                  </span>
                </div>
                <div style={{ fontSize: 12, color: 'var(--ol-ink-2)', lineHeight: 1.45, display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', overflow: 'hidden' }}>
                  {s.finalText.split('\n')[0]}
                </div>
                <div title={styleLabelFor(s, allPacks, MODE_LABEL)}>
                  <Pill size="sm" tone={s.mode === 'raw' ? 'outline' : 'default'}>{styleLabelFor(s, allPacks, MODE_LABEL)}</Pill>
                </div>
              </button>
            ))}
          </div>
        </Card>

        <Card padding={20} className="ol-thinscroll" style={{ overflow: 'auto' }}>
          {item ? (
            <>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  <span style={{ fontSize: 13, fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink-3)' }}>{formatTime(item.createdAt)}</span>
                  <Pill size="sm" tone="default">{styleLabelFor(item, allPacks, MODE_LABEL)}</Pill>
                  <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>{formatDuration(item.durationMs, t)}</span>
                </div>
                <div style={{ display: 'flex', gap: 6 }}>
                  <Btn icon={justCopied ? 'check' : 'copy'} variant="ghost" size="sm" onClick={() => void onCopy()}>{justCopied ? t('common.copied') : t('common.copy')}</Btn>
                  {item.hasAudioRecording && !audioMissingIds.has(item.id) && (
                    <Btn icon={exportedRecordingId === item.id ? 'check' : 'download'} variant="ghost" size="sm" onClick={() => void onExportAudio()}>
                      {exportedRecordingId === item.id ? t('history.exportedRecording') : t('history.exportRecording')}
                    </Btn>
                  )}
                  {item.hasAudioRecording
                    && !audioMissingIds.has(item.id)
                    && (item.errorCode === 'transcribeFailed' || item.errorCode === 'emptyTranscript') && (
                      <Btn icon="refresh" variant="ghost" size="sm" disabled={retranscribing} onClick={() => void onRetranscribe()}>
                        {retranscribing ? t('history.retranscribing') : t('history.retranscribe')}
                      </Btn>
                    )}
                  <Btn icon="trash" variant="ghost" size="sm" onClick={onDelete}>{t('common.delete')}</Btn>
                </div>
              </div>
              {item.hasAudioRecording && !audioMissingIds.has(item.id) && (
                <AudioRecordingPlayer
                  sessionId={item.id}
                  onMissing={() => markAudioMissing(item.id)}
                  key={`audio-${item.id}`}
                />
              )}
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
                <div style={{ padding: 14, border: '0.5px solid var(--ol-line)', borderRadius: 10, background: 'var(--ol-surface-2)' }}>
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, marginBottom: 10 }}>
                    <Pill size="sm" tone="outline">{t('history.rawLabel')}</Pill>
                    {item.rawTranscript && (
                      <Btn icon={justCopiedRaw ? 'check' : 'copy'} variant="ghost" size="sm" onClick={() => void onCopyRaw()}>
                        {justCopiedRaw ? t('common.copied') : t('common.copy')}
                      </Btn>
                    )}
                  </div>
                  <p style={{ margin: 0, fontSize: 13, lineHeight: 1.7, color: 'var(--ol-ink-2)', whiteSpace: 'pre-wrap' }}>
                    {item.rawTranscript || t('history.rawEmpty')}
                  </p>
                </div>
                <div style={{ padding: 14, border: '0.5px solid var(--ol-blue)', borderRadius: 10, background: 'var(--ol-blue-soft)' }}>
                  <Pill size="sm" tone="blue" style={{ marginBottom: 10 }}>{styleLabelFor(item, allPacks, MODE_LABEL)}</Pill>
                  <p style={{ margin: 0, fontSize: 13, lineHeight: 1.7, color: 'var(--ol-ink)', whiteSpace: 'pre-line' }}>
                    {item.finalText}
                  </p>
                </div>
              </div>
              <div style={{ marginTop: 18, paddingTop: 14, borderTop: '0.5px solid var(--ol-line-soft)', display: 'flex', gap: 18, fontSize: 11, color: 'var(--ol-ink-4)', flexWrap: 'wrap' }}>
                {item.appName && <span>{t('history.insertedTo')} <b style={{ color: 'var(--ol-ink-2)' }}>{item.appName}</b></span>}
                <span>{t('history.chars', { count: item.finalText.length })}</span>
                {item.dictionaryEntryCount != null && item.dictionaryEntryCount > 0 && (
                  <span>{t('history.vocabHits', { count: item.dictionaryEntryCount })}</span>
                )}
                <span>{
                  item.insertStatus === 'inserted'
                    ? t('history.inserted')
                    : item.insertStatus === 'pasteSent'
                      ? t('history.pasteSent')
                    : item.insertStatus === 'copiedFallback'
                      ? t('history.copiedFallback', { shortcut: os === 'mac' ? '⌘V' : 'Ctrl+V' })
                      : t('history.insertFailed')
                }</span>
              </div>
              {item.rawTranscript.trim() && item.errorCode !== 'qaSession' && (
                <RepolishPanel
                  session={item}
                  allPacks={allPacks}
                  packsError={packsError}
                  key={`repolish-${item.id}`}
                />
              )}
            </>
          ) : (
            <div style={{ padding: 40, textAlign: 'center', fontSize: 13, color: 'var(--ol-ink-4)' }}>
              {loading ? t('common.loading') : loadError ? t('history.loadFailed', { err: loadError }) : t('history.selectHint')}
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}

interface RepolishResult {
  key: string;
  title: string;
  text: string;
}

function RepolishPanel({ session, allPacks, packsError }: {
  session: DictationSession;
  allPacks: StylePack[] | null;
  packsError: string | null;
}) {
  const { t } = useTranslation();
  const modeLabel = useModeLabel();
  const packs = useMemo(
    () => (allPacks ? allPacks.filter(pack => pack.enabled) : null),
    [allPacks],
  );
  const [selectedPackId, setSelectedPackId] = useState('');
  const [running, setRunning] = useState<'retry' | 'apply' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [results, setResults] = useState<RepolishResult[]>([]);

  useEffect(() => {
    if (packs) setSelectedPackId(current => current || defaultPackId(packs));
  }, [packs]);

  const run = async (kind: 'retry' | 'apply') => {
    const packId = kind === 'retry'
      ? retryPackId(session, allPacks, packs ?? [])
      : selectedPackId || undefined;
    if (!packId) return;
    setRunning(kind);
    setError(null);
    try {
      const text = await repolish(session.rawTranscript, session.mode, packId);
      const pack = allPacks?.find(candidate => candidate.id === packId);
      const title = pack
        ? t('history.repolish.resultTitle', { name: packDisplayName(pack, modeLabel) })
        : t('history.repolish.retryResultTitle');
      const result = { key: packId, title, text };
      setResults(previous => [result, ...previous.filter(item => item.key !== result.key)]);
    } catch (caught) {
      const message = errorMessage(caught);
      const timeout = /^(timeout|timed out|request timed out)$/i.test(message.trim())
        || message.includes('超時')
        || message.includes('超时');
      setError(timeout
        ? t('history.repolish.timeout')
        : t('history.repolish.failed', { err: message }));
    } finally {
      setRunning(null);
    }
  };

  return (
    <div style={{ marginTop: 18, paddingTop: 14, borderTop: '0.5px solid var(--ol-line-soft)' }}>
      <div style={{ display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', gap: 10, flexWrap: 'wrap' }}>
        <b style={{ fontSize: 12, color: 'var(--ol-ink-2)' }}>{t('history.repolish.title')}</b>
        {results.length > 0 && (
          <Btn size="sm" variant="ghost" onClick={() => setResults([])}>{t('history.repolish.clear')}</Btn>
        )}
      </div>
      <p style={{ margin: '5px 0 12px', fontSize: 11, lineHeight: 1.55, color: 'var(--ol-ink-4)' }}>
        {t('history.repolish.hint')}
      </p>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
        <Btn icon="refresh" variant="ghost" size="sm" disabled={running !== null || !packs?.length} onClick={() => void run('retry')}>
          {running === 'retry' ? t('history.repolish.retrying') : t('history.repolish.retry')}
        </Btn>
        {packsError ? (
          <span style={{ fontSize: 11, color: 'var(--ol-red, #ef4444)' }}>{t('history.repolish.packsLoadFailed', { err: packsError })}</span>
        ) : (
          <>
            <select
              value={selectedPackId}
              onChange={event => setSelectedPackId(event.target.value)}
              aria-label={t('history.repolish.pickStyle')}
              disabled={!packs?.length || running !== null}
              style={{ padding: '5px 8px', fontSize: 11.5, fontFamily: 'inherit', color: 'var(--ol-ink-2)', background: 'var(--ol-surface-2)', border: '0.5px solid var(--ol-line-strong)', borderRadius: 8, maxWidth: 240 }}
            >
              {(packs ?? []).map(pack => (
                <option key={pack.id} value={pack.id}>{packDisplayName(pack, modeLabel)}</option>
              ))}
            </select>
            <Btn variant="ghost" size="sm" disabled={!selectedPackId || running !== null} onClick={() => void run('apply')}>
              {running === 'apply' ? t('history.repolish.applying') : t('history.repolish.apply')}
            </Btn>
          </>
        )}
      </div>
      {error && (
        <div style={{ marginTop: 10, padding: '8px 10px', borderRadius: 8, background: 'rgba(239,68,68,0.08)', color: 'var(--ol-red, #ef4444)', fontSize: 11.5 }}>
          {error}
        </div>
      )}
      {results.length > 0 && (
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(260px, 1fr))', gap: 12, marginTop: 14 }}>
          {results.map(result => <RepolishResultCard key={result.key} result={result} />)}
        </div>
      )}
    </div>
  );
}

function RepolishResultCard({ result }: { result: RepolishResult }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(result.text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch (error) {
      console.error('[history] failed to copy repolish result', error);
    }
  };
  return (
    <div style={{ minWidth: 0, padding: 14, border: '0.5px dashed var(--ol-line-strong)', borderRadius: 8, background: 'var(--ol-surface-2)' }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, marginBottom: 10 }}>
        <Pill size="sm" tone="default">{result.title}</Pill>
        <Btn icon={copied ? 'check' : 'copy'} variant="ghost" size="sm" onClick={() => void copy()}>
          {copied ? t('common.copied') : t('common.copy')}
        </Btn>
      </div>
      <p style={{ margin: 0, fontSize: 13, lineHeight: 1.7, color: 'var(--ol-ink-2)', whiteSpace: 'pre-wrap' }}>
        {result.text || t('history.repolish.empty')}
      </p>
    </div>
  );
}

function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

const MIN_RELIABLE_AUDIO_SECONDS = 1;

function parseWavDurationSeconds(bytes: Uint8Array): number | null {
  if (bytes.byteLength < 44) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const ascii = (offset: number, length: number) => {
    let out = '';
    for (let i = 0; i < length; i += 1) out += String.fromCharCode(view.getUint8(offset + i));
    return out;
  };
  if (ascii(0, 4) !== 'RIFF' || ascii(8, 4) !== 'WAVE') return null;

  let byteRate = 0;
  let dataSize = 0;
  for (let offset = 12; offset + 8 <= bytes.byteLength;) {
    const chunkId = ascii(offset, 4);
    const chunkSize = view.getUint32(offset + 4, true);
    const chunkDataOffset = offset + 8;
    if (chunkDataOffset + chunkSize > bytes.byteLength) break;
    if (chunkId === 'fmt ' && chunkSize >= 16) {
      byteRate = view.getUint32(chunkDataOffset + 8, true);
    } else if (chunkId === 'data') {
      dataSize = chunkSize;
      break;
    }
    offset = chunkDataOffset + chunkSize + (chunkSize % 2);
  }

  if (byteRate <= 0) return null;
  return dataSize / byteRate;
}

/** 当 session.hasAudioRecording 为 true 时渲染：一个加载按钮 + 拿到字节后切换为
 *  原生 audio controls。Blob URL 在组件 unmount 时 revoke，避免泄漏。
 *  `onMissing` 在后端返回 'recording not found'（wav 已被 prune）时触发，让父组件
 *  把按钮永久隐藏，避免用户继续点击得到同样错误。 */
function AudioRecordingPlayer({
  sessionId,
  onMissing,
}: {
  sessionId: string;
  onMissing?: () => void;
}) {
  const { t } = useTranslation();
  const [url, setUrl] = useState<string | null>(null);
  const [status, setStatus] = useState<'idle' | 'loading' | 'ready' | 'error'>('idle');
  const [errorText, setErrorText] = useState<string | null>(null);

  useEffect(() => {
    return () => {
      if (url) URL.revokeObjectURL(url);
    };
  }, [url]);

  const load = async () => {
    setStatus('loading');
    setErrorText(null);
    try {
      const bytes = await readAudioRecording(sessionId);
      if (bytes.byteLength === 0) throw new Error('empty recording');
      const durationSeconds = parseWavDurationSeconds(bytes);
      if (durationSeconds == null) throw new Error('invalid wav recording');
      if (durationSeconds < MIN_RELIABLE_AUDIO_SECONDS) {
        setStatus('error');
        setErrorText(t('history.audioTooShort', { seconds: durationSeconds.toFixed(2) }));
        return;
      }
      // typed array 在严格 TS lib 下不直接是 BlobPart；构造独立 ArrayBuffer 后 cast。
      const buffer = new ArrayBuffer(bytes.byteLength);
      new Uint8Array(buffer).set(bytes);
      const blob = new Blob([buffer], { type: 'audio/wav' });
      const objectUrl = URL.createObjectURL(blob);
      setUrl(objectUrl);
      setStatus('ready');
    } catch (error) {
      console.error('[history] load recording failed', error);
      const msg = errorMessage(error);
      // 文件被清理：通知父组件隐藏按钮组，自身不显示 error UI（用户没干错事）。
      if (msg.includes('recording not found') || msg.includes('not found')) {
        onMissing?.();
        return;
      }
      setStatus('error');
      setErrorText(msg);
    }
  };

  if (status === 'ready' && url) {
    return (
      <div style={{ marginBottom: 14 }}>
        <audio src={url} controls preload="auto" autoPlay style={{ width: '100%' }} />
      </div>
    );
  }
  return (
    <div style={{ marginBottom: 14, display: 'flex', alignItems: 'center', gap: 10 }}>
      <Btn
        icon="play"
        variant="ghost"
        size="sm"
        onClick={() => void load()}
        disabled={status === 'loading'}
      >
        {status === 'loading' ? t('history.audioLoading') : t('history.playRecording')}
      </Btn>
      {status === 'error' && (
        <span style={{ fontSize: 11, color: 'var(--ol-err)' }}>{errorText}</span>
      )}
    </div>
  );
}

function formatTime(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  const pad = (n: number) => String(n).padStart(2, '0');
  if (sameDay) return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  return `${d.getMonth() + 1}/${d.getDate()} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function formatDuration(ms: number | null, t: ReturnType<typeof useTranslation>['t']): string {
  if (ms == null || ms <= 0) return '—';
  const sec = ms / 1000;
  if (sec < 60) return t('common.durationSeconds', { value: sec.toFixed(1) });
  return t('common.durationMinutes', { value: (sec / 60).toFixed(1) });
}
