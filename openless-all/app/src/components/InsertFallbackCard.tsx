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
      <div
        role="alert"
        aria-live="polite"
        style={{ width: '100%', padding: 14, borderRadius: 8, background: 'var(--ol-capsule-pill-bg, rgba(255, 255, 255, 0.98))', border: '1px solid var(--ol-capsule-pill-border, rgba(15, 23, 42, 0.18))', boxShadow: 'var(--ol-capsule-pill-shadow, 0 18px 48px -14px rgba(15, 23, 42, 0.34))', color: 'var(--ol-capsule-btn-ink, #111827)', fontFamily: 'var(--ol-font-sans)', boxSizing: 'border-box' }}
      >
        <div style={{ fontSize: 14, lineHeight: '20px', fontWeight: 700, marginBottom: 6 }}>
          {t('insertFallbackCard.title')}
        </div>
        <div style={{ maxHeight: 92, overflowY: 'auto', fontSize: 15, lineHeight: '22px', fontWeight: 500, whiteSpace: 'pre-wrap', wordBreak: 'break-word', userSelect: 'text', cursor: 'text', marginBottom: 12 }}>
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
      onMouseDown={event => {
        event.preventDefault();
        event.stopPropagation();
      }}
      onClick={onClick}
      style={{ flex: primary ? 1 : undefined, height: 34, padding: '0 16px', borderRadius: 6, border: primary ? '1px solid var(--ol-blue, #2563eb)' : '1px solid var(--ol-capsule-btn-border, rgba(15, 23, 42, 0.18))', background: primary ? 'var(--ol-capsule-btn-bg-confirm, #2563eb)' : 'var(--ol-capsule-btn-bg, #ffffff)', color: primary ? '#ffffff' : 'var(--ol-capsule-btn-ink, #111827)', fontSize: 13, fontWeight: 600, fontFamily: 'inherit', cursor: 'pointer' }}
    >
      {label}
    </button>
  );
}
