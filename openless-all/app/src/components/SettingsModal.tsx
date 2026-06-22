// SettingsModal.tsx — 居中弹窗，左侧单层侧栏。
//
// 重构（2026-05）：原本是「外层弹窗侧栏 + 设置页内层侧栏」双层嵌套，用户点
// 「设置」还要再面对第二个侧栏。现在拍平成单层 —— 通用 / 服务 / 隐私 / 高级 /
// 个性化 / 关于 六个 tab + 帮助外链组。每个 tab 的内容见 pages/settings/。
//
// 设计原则：每个可见控件都必须可用。没有后端支撑的占位（账号 / 主题切换 等）
// 不在此弹窗出现。

import { useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { Icon } from './Icon';
import { SavedToast } from './SavedToast';
import { useSavedToastListener } from '../lib/savedEvent';
import { openExternal } from '../lib/ipc';
import { OPENLESS_UNBOUND_LINKS } from '../lib/aboutLinks';
import type { OS } from './WindowChrome';
import { GeneralTab, ServicesTab, PrivacyTab, AdvancedTab } from '../pages/settings/tabs';
import { AboutSection } from '../pages/settings/AboutSection';

// 稳定 tab ID（与 i18n key `modal.sections.*` 一致）。
export type SettingsSectionId =
  | 'general'
  | 'services'
  | 'privacy'
  | 'advanced'
  | 'about';

interface SettingsModalProps {
  os: OS;
  onClose: () => void;
  initialSettingsSection?: SettingsSectionId;
}

interface ModalNavItem {
  id: string;
  icon: string;
  external?: boolean;
  href?: string;
}

const HELP_URL = OPENLESS_UNBOUND_LINKS.docs;
const RELEASE_NOTES_URL = OPENLESS_UNBOUND_LINKS.releases;

// 第一组：可选中的 tab；第二组：外部链接（永远不 active）。
const TAB_ITEMS: ModalNavItem[] = [
  { id: 'general', icon: 'settings' },
  { id: 'services', icon: 'cloud' },
  { id: 'privacy', icon: 'shield' },
  { id: 'advanced', icon: 'bolt' },
  { id: 'about', icon: 'info' },
];
const LINK_ITEMS: ModalNavItem[] = [
  { id: 'helpCenter', icon: 'help', external: true, href: HELP_URL },
  { id: 'releaseNotes', icon: 'doc', external: true, href: RELEASE_NOTES_URL },
];

export function SettingsModal({ os: _os, onClose, initialSettingsSection }: SettingsModalProps) {
  const { t } = useTranslation();
  const [section, setSection] = useState<SettingsSectionId>(initialSettingsSection ?? 'general');
  const savedToast = useSavedToastListener();

  // 与 sidebar nav 一致的滑动指示器：仅 tab 组有 pill；外链组永远不画 pill。
  const tabRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [pillRect, setPillRect] = useState<{ top: number; height: number } | null>(null);
  useLayoutEffect(() => {
    const idx = TAB_ITEMS.findIndex(it => it.id === section);
    const el = tabRefs.current[idx];
    if (!el) return;
    setPillRect({ top: el.offsetTop, height: el.offsetHeight });
  }, [section]);

  // issue #580：用 Portal 渲染到 document.body，脱离页面 overflow:hidden 容器的
  // stacking context。否则 WebKitGTK（Debian/KDE Wayland）下页面自绘滚动条
  // (.ol-thinscroll) 不创建独立合成层，z-index 无法隔离，滚动时会盖在弹窗之上。
  // 配合 position:fixed 覆盖整窗。
  return createPortal(
    <div
      onClick={onClose}
      style={{
        position: 'fixed', inset: 0,
        background: 'rgba(15,17,22,0.32)',
        backdropFilter: 'blur(8px) saturate(140%)',
        WebkitBackdropFilter: 'blur(8px) saturate(140%)',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        padding: 28,
        zIndex: 50,
        animation: 'ol-modal-backdrop-in 0.18s var(--ol-motion-soft)',
      }}>

      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          width: '100%', maxWidth: 880, height: '100%', maxHeight: 600,
          background: 'var(--ol-surface)',
          borderRadius: 14,
          border: '0.5px solid rgba(0,0,0,.08)',
          boxShadow: '0 30px 80px -20px rgba(15,17,22,.35), 0 0 0 0.5px rgba(0,0,0,.06)',
          display: 'flex', overflow: 'hidden',
          animation: 'ol-modal-card-in 0.24s var(--ol-motion-spring)',
          position: 'relative',
        }}>

        {/* ─── 单层侧栏 ────────────────────────────────────────────── */}
        <aside
          style={{
            width: 200, flexShrink: 0,
            background: 'rgba(247,247,250,0.7)',
            borderRight: '0.5px solid var(--ol-line-soft)',
            padding: '18px 12px',
            display: 'flex', flexDirection: 'column', gap: 14,
          }}>

          {/* tab 组 */}
          <div style={{ position: 'relative', display: 'flex', flexDirection: 'column', gap: 1 }}>
            {pillRect && (
              <div
                aria-hidden
                style={{
                  position: 'absolute',
                  left: 0,
                  right: 0,
                  top: pillRect.top,
                  height: pillRect.height,
                  background: '#fff',
                  borderRadius: 8,
                  boxShadow: '0 1px 2px rgba(0,0,0,.05), 0 0 0 0.5px rgba(0,0,0,.06)',
                  transition: 'top 0.36s var(--ol-motion-spring), height 0.36s var(--ol-motion-spring)',
                  pointerEvents: 'none',
                  zIndex: 0,
                }}
              />
            )}
            {TAB_ITEMS.map((it, idx) => {
              const active = section === it.id;
              return (
                <button
                  key={it.id}
                  ref={el => { tabRefs.current[idx] = el; }}
                  onClick={() => setSection(it.id as SettingsSectionId)}
                  className={active ? 'ol-nav-btn ol-nav-btn-active' : 'ol-nav-btn'}
                  style={navBtnStyle}>
                  <Icon name={it.icon} size={14} />
                  <span style={{ flex: 1 }}>{t(`modal.sections.${it.id}`)}</span>
                </button>
              );
            })}
          </div>

          {/* 外链组 */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: 1, paddingTop: 8, borderTop: '0.5px solid var(--ol-line-soft)' }}>
            {LINK_ITEMS.map(it => (
              <button
                key={it.id}
                onClick={() => { if (it.href) void openExternal(it.href); }}
                className="ol-nav-btn"
                style={navBtnStyle}>
                <Icon name={it.icon} size={14} />
                <span style={{ flex: 1 }}>{t(`modal.sections.${it.id}`)}</span>
                <Icon name="external" size={11} />
              </button>
            ))}
          </div>
        </aside>

        {/* ─── 内容区 ──────────────────────────────────────────────
            父容器 overflow:hidden + 列向 flex；关闭按钮、section 标题固定在头部，
            只有最里层的 scroll wrapper 真正滚动。 */}
        <div style={{ flex: 1, minWidth: 0, overflow: 'hidden', position: 'relative', display: 'flex', flexDirection: 'column' }}>
          {/* "已保存" toast：right:54 避开 28×28 关闭按钮 + 12px gap。 */}
          <SavedToast
            saveState={savedToast.state}
            message={savedToast.message}
            slideFrom="top"
            offsetStyle={{ position: 'absolute', top: 16, right: 54 }}
          />
          <button
            onClick={onClose}
            style={{
              position: 'absolute', top: 14, right: 14, zIndex: 2,
              width: 28, height: 28, border: 0, borderRadius: 999,
              background: 'transparent', color: 'var(--ol-ink-3)',
              display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
              cursor: 'default',
              transition: 'background 0.16s var(--ol-motion-quick)',
            }}
            onMouseEnter={e => (e.currentTarget.style.background = 'rgba(0,0,0,0.05)')}
            onMouseLeave={e => (e.currentTarget.style.background = 'transparent')}
            title={t('common.close')}>
            <Icon name="close" size={14} />
          </button>

          <h2 style={{ margin: 0, padding: '22px 28px 8px', fontSize: 22, fontWeight: 600, letterSpacing: '-0.02em', flexShrink: 0 }}>
            {t(`modal.sections.${section}`)}
          </h2>

          <div
            className="ol-thinscroll"
            style={{ flex: 1, minHeight: 0, overflow: 'auto', padding: '10px 28px 28px' }}>
            {/* key=section 让切 tab 时整块重挂载，ol-tab-fade 轻微淡入。 */}
            <div
              key={section}
              style={{ display: 'flex', flexDirection: 'column', gap: 12, animation: 'ol-tab-fade 0.2s var(--ol-motion-soft)' }}>
              {section === 'general' && <GeneralTab />}
              {section === 'services' && <ServicesTab />}
              {section === 'privacy' && <PrivacyTab />}
              {section === 'advanced' && <AdvancedTab />}
              {section === 'about' && <AboutSection />}
            </div>
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}

const navBtnStyle = {
  display: 'flex', alignItems: 'center', gap: 10,
  padding: '7px 10px',
  borderRadius: 8, border: 0,
  background: 'transparent',
  fontFamily: 'inherit', fontSize: 13,
  cursor: 'default', textAlign: 'left' as const,
  position: 'relative' as const,
  zIndex: 1,
  transition: 'color 0.16s var(--ol-motion-quick), background 0.16s var(--ol-motion-quick)',
};
