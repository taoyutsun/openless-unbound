<p align="center">
  <img src="openless-all/app/src-tauri/icons/128x128@2x.png" alt="OpenLess Unbound" width="144" />
</p>

<h1 align="center">OpenLess Unbound</h1>

<p align="center">
  <strong>A liberated GitHub fork of OpenLess.</strong><br/>
  Voice input, AI polishing, live translation, and selection-based Q&A with more flexible provider routing and a better Traditional Chinese Windows input experience.
</p>

<p align="center">
  <a href="README.md">繁體中文</a> · <a href="README.en.md">English</a>
</p>

---

OpenLess Unbound is a GitHub fork of [OpenLess](https://github.com/Open-Less/openless). The original developers already provide a full cross-platform voice input workflow: press a global hotkey, record audio, transcribe it with ASR, polish the text with an LLM, and insert the result at the current cursor.

This fork keeps that foundation and focuses on practical constraints we ran into during real use:

- Selection Q&A no longer requires Volcengine ASR credentials; it follows the active ASR provider.
- Selection Q&A can use its own answer model instead of being tightly coupled to the general polishing model.
- Selection Q&A answers can be partially selected manually, or copied as a whole with the copy button.
- Codex OAuth, custom OpenAI-compatible endpoints, and other LLM routes can be used according to speed, quality, and cost.
- The Windows TSF profile is registered as Traditional Chinese Taiwan to reduce accidental switching to Simplified Chinese input.
- The app uses its own identity and data directory, so it can coexist with upstream OpenLess during testing.
- Original developer attribution and upstream links are preserved for tracking official updates and feedback.

## Features

- **Voice input**: start recording with a global hotkey, transcribe, and insert text into the active app.
- **AI polishing**: clean up spoken text into raw, light, structured, or formal styles.
- **Live translation**: press the translation shortcut while recording to translate into a target language before insertion.
- **Selection Q&A**: select text in any app and ask follow-up questions in a floating panel.
- **Vocabulary**: add names, product terms, English tool names, and domain words to improve recognition.
- **Correction rules**: rewrite common ASR mistakes into preferred text.
- **History**: review and reuse previous dictation results.
- **Local-first storage**: settings and history stay on the device; cloud ASR / LLM calls happen only when enabled by the user.

## Differences From Upstream

| Area | Upstream OpenLess | OpenLess Unbound |
| --- | --- | --- |
| Selection Q&A ASR | Older flow may require Volcengine ASR | Uses the active ASR provider |
| Selection Q&A LLM | More tightly coupled to the existing flow | Separate provider, model, and thinking settings |
| Windows IME | Could switch Traditional Chinese users to a Simplified Chinese profile | TSF profile registered as Traditional Chinese Taiwan |
| App identity | `OpenLess` | `OpenLess Unbound`, with a separate data directory |
| Update path | Official upstream | Independent fork, with upstream attribution preserved |

## Recent Sync

`v1.3.14-2` fixes an issue where uninstalling the Windows MSI could leave `OpenLess Unbound Voice Input` registered in Windows. The installer now unregisters the COM and TSF profiles before deleting the x64 and x86 IME DLLs. The portable build does not register the IME by itself and is not affected by this uninstall issue.

`v1.3.14-1` selectively integrates upstream OpenLess 1.3.14 fixes that fit the Windows and Unbound mainline while retaining this fork's existing behavior:

- Updates Windows Foundry Local Whisper to the compatible 1.2.1 runtime, transcribes long recordings in 30-second chunks, and scales timeouts with audio duration.
- Moves Windows IME IPC to cancellable overlapped I/O to reduce shutdown or restart hangs while preserving the Traditional Chinese Taiwan TSF profile.
- Lets custom OpenAI-compatible LLM providers define extra HTTP headers for regular polishing, model listing, and the independent custom Selection Q&A provider.
- Loads the Codex OAuth model list from the local Codex `models_cache.json` when available, with a built-in fallback that includes GPT-5.6 Sol, Terra, and Luna.
- Improves audio cue recovery, protects style settings from stale concurrent writes, and makes copying raw or polished history text more reliable.

Android APK runtime, mobile remote input, Less Computer / Cloud Agent, and large mobile UI changes remain excluded to avoid adding dependencies and risk that the Windows build does not need.

`v1.3.11-2` fixes a Traditional Chinese conversion regression where the general `s2t` table changed `吃` into the less common `喫`. The app now uses the Taiwan Traditional conversion table so everyday Taiwan wording such as `吃飯` stays intact. It also fixes a dictation polish regression where using the Traditional Chinese UI could cause English speech to be rewritten into Chinese. The UI locale now only syncs the Chinese script preference; normal dictation keeps the spoken language and mixed Chinese-English text unless translation is explicitly enabled.

`v1.3.11-1` is a low-risk ASR stability sync. It brings over the provider and timeout changes that have already been tested, while keeping this fork's configurable Selection Q&A provider routing, Codex OAuth support, Traditional Chinese TSF profile, and portable DLL packaging fix:

- Adds OpenRouter as an ASR provider for Whisper-compatible transcription through OpenRouter.
- Scales Whisper-compatible ASR timeouts by recording length and raises the global fallback timeout from 15 seconds to 30 seconds, reducing premature failures on longer recordings or slower networks.
- Applies the same more stable timeout path to Selection Q&A when it uses a Whisper-compatible ASR provider.
- Keeps the Groq ASR provider label as `Groq`; the Whisper model is configured in the model field.

`v1.3.11-1` is not a full upstream 1.3.11 merge. Android, mobile remote input, Less Computer, and large UI/theme refactors remain excluded and will be evaluated separately.

Starting with `v1.3.6-1`, OpenLess Unbound includes selected upstream OpenLess 1.3.6 beta improvements:

- Local ASR model storage management.
- Multi-monitor capsule positioning so the capsule follows the active input screen.
- A short cooldown after Toggle recording sessions to reduce accidental immediate re-triggering.
- MediaPlayPause hotkey trigger support.
- Groq / OpenAI Whisper verbose JSON filtering to reduce hallucinated text from very short or silent audio.

## Installation

Download the latest build from this repository's [Releases](../../releases) page.

Recommended Windows artifacts:

- `OpenLess_Unbound_<version>_x64.msi`: installer build for daily use.
- `OpenLess_Unbound_<version>_x64_portable.zip`: portable build for testing.

Windows may show SmartScreen or "unknown publisher" warnings until code signing is configured.

## Basic Setup

1. Open OpenLess Unbound.
2. Configure an ASR provider in Settings -> Services, such as Groq, OpenRouter Whisper, an OpenAI Whisper-compatible endpoint, Foundry Local Whisper, Sherpa-ONNX local, or another supported provider.
3. Configure an LLM polishing model in Settings -> Services, such as Codex OAuth, Groq, OpenAI, Gemini, OpenRouter, or a custom OpenAI-compatible endpoint.
4. Confirm the start / stop recording hotkey in Settings -> General. The default is Right Ctrl.
5. Add frequently used names, product names, and domain terms in Vocabulary.
6. Configure the Selection Q&A answer model separately if you want it to use a higher quality or faster provider than normal polishing.

## General Dictation

1. Place the cursor in the target app or text field.
2. Press Right Ctrl to start recording, then press it again to stop.
3. OpenLess Unbound transcribes with ASR, then polishes the text according to the current style.
4. The result is inserted at the cursor. If the target app rejects direct insertion, the text is still available through the clipboard or fallback insertion path.

Add recurring names and product terms to Vocabulary. Put stable recognition mistakes in Correction Rules.

## Live Translation

1. Open the Translation page and choose your working languages and target language.
2. Press Right Ctrl to start recording.
3. Press Shift once during recording to activate translation mode.
4. Press Right Ctrl again to stop recording and insert the translated result.

If the target language is disabled, Shift does nothing. If translation fails, the app falls back to inserting the original transcript so text is not lost.

## Selection Q&A

1. Press `Ctrl+Shift+;` to open the floating Q&A panel.
2. Select text in any app.
3. Press Right Ctrl to record, then press it again to submit.
4. Press Right Ctrl again for follow-up questions.
5. Select part of an answer manually, or use the copy button to copy the whole assistant answer.
6. Press Esc to close the panel and clear the temporary context.

Selection Q&A does not provide a built-in browser, live search, or external tool-calling flow. It is meant for explaining, rewriting, summarizing, and asking follow-up questions about the selected text; it does not look up current exchange rates, weather, or news in real time.

## Windows IME Notes

OpenLess Unbound includes a Windows TSF input backend for inserting dictated text into the current app. This fork registers that profile as Traditional Chinese Taiwan, reducing the chance that Traditional Chinese users get switched to Simplified Chinese after dictation.

## Run From Source

The Git root is the repository root. The runnable Tauri app lives in `openless-all/app/`.

```powershell
git submodule update --init --recursive
cd openless-all/app
npm ci
npm run tauri -- dev
```

Recommended Windows packaging route:

```powershell
cd openless-all/app
powershell -ExecutionPolicy Bypass -File .\scripts\windows-preflight.ps1 -Toolchain msvc
powershell -ExecutionPolicy Bypass -File .\scripts\windows-package-msvc.ps1
```

Artifacts are written to:

```text
openless-all/app/.artifacts/windows-msvc/
```

## Repository Layout

```text
openless-src/
  README.md                 # Main Traditional Chinese README
  README.en.md              # English README
  USAGE.md                  # Additional usage notes
  openless-all/
    app/                    # Tauri 2 + React + Rust app
      src/                  # Frontend
      src-tauri/            # Rust backend and bundle config
      windows-ime/          # Windows TSF IME backend
      scripts/              # Build, test, and import utilities
```

## Upstream And License

OpenLess Unbound is forked from [OpenLess](https://github.com/Open-Less/openless). Thanks to the original developers and community for the original cross-platform voice input foundation.

This project uses the MIT License. Fork-specific changes are also released under the MIT License.
