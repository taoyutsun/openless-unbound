import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { copyTextToClipboard, dismissInsertFallbackCard } from '../lib/ipc';
import type { InsertFallbackCardPayload } from '../lib/types';

const TTL_MS = 20_000;

export function InsertFallbackCard({ payload }: { payload: InsertFallbackCardPayload }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);
  const [paused, setPaused] = useState(false);
  const timerRef = useRef<number | null>(null);

  useEffect(() => {
    if (paused) return undefined;
    timerRef.current = window.setTimeout(() => {
      void dismissInsertFallbackCard();
    }, TTL_MS);
    return () => {
      if (timerRef.current != null) window.clearTimeout(timerRef.current);
    };
  }, [paused, payload.presentationId]);

  const copy = async () => {
    try {
      await copyTextToClipboard(payload.text);
      setCopyFailed(false);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopyFailed(true);
    }
  };

  return (
    <div
      style={{ width: '100%', height: '100%', display: 'flex', alignItems: 'flex-end', padding: 12, boxSizing: 'border-box', pointerEvents: 'auto' }}
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
    >
      <div style={{ width: '100%', padding: 12, borderRadius: 8, background: 'var(--ol-capsule-pill-bg)', border: '1px solid var(--ol-capsule-pill-border)', boxShadow: 'var(--ol-capsule-pill-shadow)', color: 'var(--ol-capsule-btn-ink)', fontFamily: 'var(--ol-font-sans)', backdropFilter: 'blur(20px)', WebkitBackdropFilter: 'blur(20px)' }}>
        <div style={{ maxHeight: 126, overflowY: 'auto', fontSize: 13, lineHeight: '18px', whiteSpace: 'pre-wrap', wordBreak: 'break-word', userSelect: 'text', cursor: 'text', marginBottom: 10 }}>
          {payload.text}
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          <CardButton
            primary
            label={copyFailed ? t('insertFallbackCard.copyFailed') : copied ? t('insertFallbackCard.copied') : t('insertFallbackCard.copy')}
            onClick={() => void copy()}
          />
          <CardButton label={t('insertFallbackCard.dismiss')} onClick={() => void dismissInsertFallbackCard()} />
        </div>
      </div>
    </div>
  );
}

function CardButton({ label, primary, onClick }: { label: string; primary?: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      onMouseDown={event => event.preventDefault()}
      onClick={onClick}
      style={{ flex: primary ? 1 : undefined, height: 30, padding: '0 14px', borderRadius: 6, border: '0.8px solid var(--ol-capsule-btn-border)', background: primary ? 'var(--ol-capsule-btn-bg-confirm)' : 'var(--ol-capsule-btn-bg)', color: 'var(--ol-capsule-btn-ink)', fontSize: 12, fontFamily: 'inherit', cursor: 'default' }}
    >
      {label}
    </button>
  );
}
