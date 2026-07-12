// 录音提示音：用 Web Audio API 即时「合成」一段短促上升双音，不打包任何音频文件。
// 提供两个操作：
//   - playRecordStartCue()  播放（按下录音热键、进入 recording 状态时调用）
//   - stopAudioCue()        关闭/停止（离开 recording、或连按热键避免叠音时调用）
//
// 触发点在 capsule 窗口（始终存活、收到 capsule:state 事件）；设置页「试听」也复用同一份。
// 设计原则：任何环境（无 Web Audio、AudioContext 被自动播放策略挂起、单音创建失败）都
// 静默降级，绝不抛错影响录音主流程。

/** 单个正弦音符的合成参数（相对提示音起点）。 */
export interface CueTone {
  /** 频率 (Hz)。 */
  freq: number;
  /** 相对提示音起点的开始时间 (ms)。 */
  startMs: number;
  /** 持续时长 (ms)。 */
  durationMs: number;
  /** 指数包络峰值增益 (0..1)，控制响度。 */
  peakGain: number;
}

// 上升小三度双音 (A5 880Hz → C#6 1108.73Hz)：给「开始录音」一个明确、轻快、不刺耳的听感。
// 两个音轻微交叠，听感连贯成一个「叮咚」而非两声独立 beep。纯数据 → 便于单测。
export function recordStartCueTones(): CueTone[] {
  return [
    { freq: 880, startMs: 0, durationMs: 130, peakGain: 0.16 },
    { freq: 1108.73, startMs: 95, durationMs: 170, peakGain: 0.18 },
  ];
}

/** 提示音总时长 (ms) = 最后一个音的结束时刻。供调用方排期 stop / 试听反馈用。 */
export function cueTotalDurationMs(tones: CueTone[]): number {
  return tones.reduce((max, t) => Math.max(max, t.startMs + t.durationMs), 0);
}

// Safari/WKWebView 旧前缀；用结构化类型而非 any 拿到 webkit 兜底构造器。
type AudioContextCtor = typeof AudioContext;
interface WebkitWindow {
  webkitAudioContext?: AudioContextCtor;
}

// 模块级单例。Tauri 每个窗口是独立 webview = 独立 JS 模块实例，所以 capsule 窗口与
// 设置窗口各自持有一份 ctx / activeVoices，不会互相打架。
let sharedCtx: AudioContext | null = null;
interface ActiveVoice {
  osc: OscillatorNode;
  gain: GainNode;
}
let activeVoices: ActiveVoice[] = [];
let playSeq = 0;
let stopSeq = 0;

const DEFERRED_CUE_LATE_THRESHOLD_MS = 400;

function nowMs(): number {
  return typeof performance !== 'undefined' ? performance.now() : 0;
}

export function shouldPlayDeferredCue(params: {
  superseded: boolean;
  stoppedWhileWaiting: boolean;
  elapsedMs: number;
  lateThresholdMs: number;
}): boolean {
  if (params.superseded) return false;
  if (params.stoppedWhileWaiting && params.elapsedMs > params.lateThresholdMs) return false;
  return true;
}

export type AudioContextAction = 'ready' | 'resume' | 'recreate';

export function audioContextActionForState(state: string): AudioContextAction {
  if (state === 'closed') return 'recreate';
  if (state === 'running') return 'ready';
  return 'resume';
}

export type CueAction = 'schedule' | 'recreate-retry' | 'drop';

export function cueActionAfterResume(params: {
  runningAfterResume: boolean;
  shouldPlay: boolean;
  allowRecreate: boolean;
}): CueAction {
  if (!params.shouldPlay) return 'drop';
  if (params.runningAfterResume) return 'schedule';
  return params.allowRecreate ? 'recreate-retry' : 'drop';
}

function resolveAudioContextCtor(): AudioContextCtor | null {
  if (typeof window === 'undefined') return null;
  // window.AudioContext 来自全局声明；webkit 前缀单独用结构化类型拿，避免 any。
  const webkit = window as WebkitWindow;
  return window.AudioContext ?? webkit.webkitAudioContext ?? null;
}

function getContext(): AudioContext | null {
  const Ctor = resolveAudioContextCtor();
  if (!Ctor) return null;
  if (sharedCtx && audioContextActionForState(sharedCtx.state) === 'recreate') {
    sharedCtx = null;
    activeVoices = [];
  }
  if (!sharedCtx) {
    try {
      sharedCtx = new Ctor();
    } catch {
      sharedCtx = null;
      return null;
    }
  }
  return sharedCtx;
}

function discardContext(ctx: AudioContext): void {
  if (sharedCtx === ctx) sharedCtx = null;
  activeVoices = [];
  try {
    void ctx.close().catch(() => undefined);
  } catch {
    // The context may already be closed or expose a non-Promise implementation.
  }
}

// 停掉当前正在发声的节点（不影响 playGeneration —— 仅做去叠音 / 收尾）。
function stopVoices(): void {
  const ctx = sharedCtx;
  const now = ctx?.currentTime ?? 0;
  for (const { osc, gain } of activeVoices) {
    try {
      gain.gain.cancelScheduledValues(now);
      // 指数 ramp 不能到 0，用极小值做近似静音后立即停振。
      gain.gain.setValueAtTime(0.0001, now);
      osc.stop(now + 0.02);
    } catch {
      // 已停止 / 已断开，忽略。
    }
  }
  activeVoices = [];
}

/** 关闭/停止提示音，并标记等待 resume 的播放请求可能已经过期。 */
export function stopAudioCue(): void {
  stopSeq++;
  stopVoices();
}

// 实际排期合成节点。必须在 AudioContext 处于 running（非 suspended）时调用：
// suspended 时 currentTime 冻结在暂停时刻，节点会排到过期时间点 → 不发声还堆积。
function scheduleCueVoices(ctx: AudioContext): void {
  // 连按热键时先停掉上一轮，避免叠音越来越响。用 stopVoices 而非 stopAudioCue：
  // 这里不该作废自己这一轮的 generation。
  stopVoices();

  const base = ctx.currentTime + 0.01;
  for (const tone of recordStartCueTones()) {
    try {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = 'sine';
      const t0 = base + tone.startMs / 1000;
      const tEnd = t0 + tone.durationMs / 1000;
      osc.frequency.setValueAtTime(tone.freq, t0);
      // 5ms attack + 指数 release：避免起停的 click 爆音。
      gain.gain.setValueAtTime(0.0001, t0);
      gain.gain.exponentialRampToValueAtTime(tone.peakGain, t0 + 0.005);
      gain.gain.exponentialRampToValueAtTime(0.0001, tEnd);
      osc.connect(gain).connect(ctx.destination);
      osc.start(t0);
      osc.stop(tEnd + 0.02);

      const voice: ActiveVoice = { osc, gain };
      activeVoices.push(voice);
      osc.onended = () => {
        activeVoices = activeVoices.filter(v => v !== voice);
        try {
          osc.disconnect();
          gain.disconnect();
        } catch {
          // noop
        }
      };
    } catch {
      // 单个音创建/排期失败不影响其余音。
    }
  }
}

/** 播放「开始录音」提示音。无 Web Audio 或被挂起且无法恢复时静默降级。 */
export function playRecordStartCue(): void {
  playRecordStartCueOnce(true);
}

function playRecordStartCueOnce(allowRecreate: boolean): void {
  const ctx = getContext();
  if (!ctx) return;

  if (audioContextActionForState(ctx.state) !== 'resume') {
    scheduleCueVoices(ctx);
    return;
  }

  const myPlay = ++playSeq;
  const stopAtRequest = stopSeq;
  const requestedAt = nowMs();

  const settle = (runningAfterResume: boolean): void => {
    const action = cueActionAfterResume({
      runningAfterResume,
      shouldPlay: shouldPlayDeferredCue({
        superseded: myPlay !== playSeq,
        stoppedWhileWaiting: stopSeq !== stopAtRequest,
        elapsedMs: nowMs() - requestedAt,
        lateThresholdMs: DEFERRED_CUE_LATE_THRESHOLD_MS,
      }),
      allowRecreate,
    });
    if (action === 'schedule') {
      scheduleCueVoices(ctx);
    } else if (action === 'recreate-retry') {
      discardContext(ctx);
      playRecordStartCueOnce(false);
    }
  };

  ctx
    .resume()
    .then(() => settle(ctx.state === 'running'))
    .catch(() => settle(false));
}
