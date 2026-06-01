// SelectionAsk.tsx — 独立的"划词追问"页（issue #118 / PR #119 配置 UI 拆分版）。
// 功能：用户在任意 app 选中一段文字 → 按 hotkey → 浮窗弹出 + 进入语音录音 →
// 用户口述提问 → ASR + 选区 + 提问 一起送 LLM → 答案以 markdown 显示在浮窗。
//
// 这一页把原本散在 Settings → 录音 里的两条配置（hotkey 预设 / 保存 Q&A 历史）
// 集中起来 + 加完整使用指南，跟"翻译"页平级。

import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Card, PageHeader } from './_atoms';
import { Toggle } from './settings/shared';
import { SavedToast } from '../components/SavedToast';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { defaultQaShortcut, formatComboLabel } from '../lib/hotkey';
import type { UserPreferences } from '../lib/types';

type SaveState = 'idle' | 'saving' | 'saved' | 'failed';

const QA_LLM_PROVIDER_OPTIONS = [
  { id: '', labelKey: 'selectionAsk.answerModel.inherit', modelPlaceholder: '' },
  { id: 'ark', labelKey: 'settings.providers.presets.ark', modelPlaceholder: 'deepseek-v3-2' },
  { id: 'deepseek', labelKey: 'settings.providers.presets.deepseek', modelPlaceholder: 'deepseek-v4-flash' },
  { id: 'siliconflow', labelKey: 'settings.providers.presets.siliconflow', modelPlaceholder: 'Qwen/Qwen2.5-7B-Instruct' },
  { id: 'openai', labelKey: 'settings.providers.presets.openai', modelPlaceholder: 'gpt-4o' },
  { id: 'gemini', labelKey: 'settings.providers.presets.gemini', modelPlaceholder: 'gemini-2.5-flash' },
  { id: 'codex_oauth', labelKey: 'settings.providers.presets.codexOAuth', modelPlaceholder: 'gpt-5.3-codex-spark' },
  { id: 'mimo', labelKey: 'settings.providers.presets.mimo', modelPlaceholder: 'xiaomi/mimo-v2-flash' },
  { id: 'cometapi', labelKey: 'settings.providers.presets.cometapi', modelPlaceholder: 'gpt-4o' },
  { id: 'openrouterFree', labelKey: 'settings.providers.presets.openrouterFree', modelPlaceholder: 'qwen/qwen3-coder:free' },
  { id: 'alibabaCoding', labelKey: 'settings.providers.presets.alibabaCoding', modelPlaceholder: 'qwen3-coder-plus' },
  { id: 'codingPlanX', labelKey: 'settings.providers.presets.codingPlanX', modelPlaceholder: 'gpt-5-mini' },
  { id: 'custom', labelKey: 'settings.providers.presets.custom', modelPlaceholder: 'openai/gpt-oss-120b' },
] as const;

export function SelectionAsk() {
  const { t } = useTranslation();
  const { prefs, refresh, updatePrefs: savePrefs } = useHotkeySettings();
  const [saveState, setSaveState] = useState<SaveState>('idle');
  const [saveMessage, setSaveMessage] = useState('');
  const [qaModelDraft, setQaModelDraft] = useState('');
  const statusTimer = useRef<number | null>(null);
  const defaultHotkeyLabel = formatComboLabel(defaultQaShortcut());
  const recordHotkeyLabel = prefs ? formatComboLabel(prefs.dictationHotkey) : '快捷键';

  useEffect(() => () => {
    if (statusTimer.current !== null) window.clearTimeout(statusTimer.current);
  }, []);

  useEffect(() => {
    setQaModelDraft(prefs?.qaLlmModel ?? '');
  }, [prefs?.qaLlmModel]);

  const showSaveStatus = (state: SaveState, message: string, temporary = false) => {
    if (statusTimer.current !== null) {
      window.clearTimeout(statusTimer.current);
      statusTimer.current = null;
    }
    setSaveState(state);
    setSaveMessage(message);
    if (temporary) {
      statusTimer.current = window.setTimeout(() => {
        setSaveState('idle');
        setSaveMessage('');
        statusTimer.current = null;
      }, 1600);
    }
  };

  const persistPrefs = async (
    resolveNext: (current: UserPreferences) => UserPreferences,
    failureMessage: string,
  ) => {
    try {
      await savePrefs(resolveNext);
      showSaveStatus('saved', t('common.saved'), true);
      return true;
    } catch (error) {
      console.error('[selection-ask] failed to save preferences', error);
      showSaveStatus('failed', failureMessage);
      await refresh().catch(refreshError => {
        console.warn('[selection-ask] failed to refresh preferences after save error', refreshError);
      });
      return false;
    }
  };

  if (!prefs) {
    return (
      <>
        <PageHeader title={t('selectionAsk.title')} />
        <Card>
          <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
        </Card>
      </>
    );
  }

  const onSaveHistoryChange = (qaSaveHistory: boolean) => {
    showSaveStatus('saving', t('common.saving'));
    void persistPrefs(
      current => ({ ...current, qaSaveHistory }),
      t('selectionAsk.save.historySaveFailed'),
    );
  };

  const onAnswerProviderChange = (providerId: string) => {
    showSaveStatus('saving', t('common.saving'));
    setQaModelDraft('');
    void persistPrefs(
      current => ({
        ...current,
        qaLlmProvider: providerId || null,
        qaLlmModel: null,
      }),
      t('selectionAsk.save.answerModelSaveFailed'),
    );
  };

  const commitAnswerModel = () => {
    const nextModel = qaModelDraft.trim();
    const currentModel = prefs.qaLlmModel ?? '';
    if (nextModel === currentModel) return;
    showSaveStatus('saving', t('common.saving'));
    void persistPrefs(
      current => ({ ...current, qaLlmModel: nextModel || null }),
      t('selectionAsk.save.answerModelSaveFailed'),
    );
  };

  const onAnswerThinkingChange = (enabled: boolean) => {
    showSaveStatus('saving', t('common.saving'));
    void persistPrefs(
      current => ({ ...current, qaLlmThinkingEnabled: enabled }),
      t('selectionAsk.save.answerModelSaveFailed'),
    );
  };

  const enabled = prefs.qaHotkey !== null;
  const currentLabel = prefs.qaHotkey ? formatComboLabel(prefs.qaHotkey) : defaultHotkeyLabel;
  const answerProvider = prefs.qaLlmProvider ?? '';
  const answerProviderOption =
    QA_LLM_PROVIDER_OPTIONS.find(option => option.id === answerProvider) ?? QA_LLM_PROVIDER_OPTIONS[0];
  const answerThinkingEnabled = prefs.qaLlmThinkingEnabled ?? prefs.llmThinkingEnabled;
  const saving = saveState === 'saving';

  return (
    <>
      <PageHeader title={t('selectionAsk.title')} />

      <SavedToast saveState={saveState} message={saveMessage} />

      <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        {/* 1. 历史保存 — 小型自适宽度模块，避免在宽页面里拉成一条长横条 */}
        <div
          style={{
            display: 'inline-flex',
            alignSelf: 'flex-start',
            alignItems: 'center',
            gap: 12,
            padding: '8px 14px',
            borderRadius: 10,
            background: 'rgba(0,0,0,0.04)',
            border: '0.5px solid var(--ol-line)',
          }}
        >
          <span style={{ fontSize: 12.5, fontWeight: 500, color: 'var(--ol-ink-2)' }}>
            {t('selectionAsk.history.title')}
          </span>
          <button
            onClick={() => onSaveHistoryChange(!prefs.qaSaveHistory)}
            aria-pressed={prefs.qaSaveHistory}
            disabled={saving}
            style={{
              position: 'relative',
              width: 36,
              height: 20,
              borderRadius: 999,
              border: 0,
              background: prefs.qaSaveHistory ? 'var(--ol-blue)' : 'rgba(0,0,0,0.18)',
              cursor: saving ? 'not-allowed' : 'default',
              opacity: saving ? 0.68 : 1,
              transition: 'background 0.16s var(--ol-motion-quick)',
              padding: 0,
            }}
          >
            <span
              style={{
                position: 'absolute',
                top: 2,
                left: prefs.qaSaveHistory ? 18 : 2,
                width: 16,
                height: 16,
                borderRadius: 999,
                background: '#fff',
                boxShadow: '0 1px 2px rgba(0,0,0,.2)',
                transition: 'left .16s var(--ol-motion-spring)',
              }}
            />
          </button>
        </div>

        {/* 2. 使用方法 */}
        <Card>
          <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 12 }}>{t('selectionAsk.answerModel.title')}</div>
          <div style={{ display: 'grid', gridTemplateColumns: '120px minmax(0, 1fr)', gap: '10px 14px', alignItems: 'center' }}>
            <label style={{ fontSize: 12.5, color: 'var(--ol-ink-2)' }}>{t('selectionAsk.answerModel.provider')}</label>
            <select
              value={answerProvider}
              disabled={saving}
              onChange={event => onAnswerProviderChange(event.currentTarget.value)}
              style={{
                width: '100%',
                height: 34,
                borderRadius: 8,
                border: '1px solid var(--ol-line)',
                background: 'var(--ol-bg)',
                color: 'var(--ol-ink-1)',
                padding: '0 10px',
                fontSize: 13,
              }}
            >
              {QA_LLM_PROVIDER_OPTIONS.map(option => (
                <option key={option.id || 'inherit'} value={option.id}>
                  {t(option.labelKey)}
                </option>
              ))}
            </select>

            <label style={{ fontSize: 12.5, color: 'var(--ol-ink-2)' }}>{t('selectionAsk.answerModel.model')}</label>
            <input
              value={qaModelDraft}
              disabled={!answerProvider || saving}
              placeholder={answerProviderOption.modelPlaceholder}
              onChange={event => setQaModelDraft(event.currentTarget.value)}
              onBlur={commitAnswerModel}
              onKeyDown={event => {
                if (event.key === 'Enter') {
                  event.currentTarget.blur();
                }
              }}
              style={{
                width: '100%',
                height: 34,
                borderRadius: 8,
                border: '1px solid var(--ol-line)',
                background: answerProvider ? 'var(--ol-bg)' : 'rgba(0,0,0,0.035)',
                color: 'var(--ol-ink-1)',
                padding: '0 10px',
                fontSize: 13,
                boxSizing: 'border-box',
              }}
            />

            <label style={{ fontSize: 12.5, color: 'var(--ol-ink-2)' }}>{t('settings.providers.thinkingModeLabel')}</label>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <Toggle on={answerThinkingEnabled} onToggle={next => onAnswerThinkingChange(next)} />
              <span style={{ fontSize: 11.5, color: answerThinkingEnabled ? 'var(--ol-blue)' : 'var(--ol-ink-4)' }}>
                {answerThinkingEnabled ? t('settings.providers.thinkingModeOn') : t('settings.providers.thinkingModeOff')}
              </span>
            </div>
          </div>
        </Card>

        <Card>
          <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 10 }}>{t('selectionAsk.howto.title')}</div>
          <ol style={{ margin: 0, paddingLeft: 18, fontSize: 12.5, color: 'var(--ol-ink-2)', lineHeight: 1.7 }}>
            <li>{t('selectionAsk.howto.step1', { hotkey: enabled ? currentLabel : defaultHotkeyLabel })}</li>
            <li>{t('selectionAsk.howto.step2')}</li>
            <li>{t('selectionAsk.howto.step3', { recordHotkey: recordHotkeyLabel })}</li>
            <li>{t('selectionAsk.howto.step4', { recordHotkey: recordHotkeyLabel })}</li>
            <li>{t('selectionAsk.howto.step5', { hotkey: enabled ? currentLabel : defaultHotkeyLabel })}</li>
          </ol>
        </Card>
      </div>
    </>
  );
}
