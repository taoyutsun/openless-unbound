# OpenLess Unbound Workspace

This directory contains the active cross-platform Tauri app for OpenLess Unbound.

The runnable app lives in `app/`.

## First Clone

```bash
git submodule update --init --recursive
cd openless-all/app
npm ci
```

The macOS local ASR path uses the vendored `Open-Less/qwen-asr` submodule under `app/src-tauri/vendor/qwen-asr/`.

## Development

```bash
cd openless-all/app
npm run tauri -- dev
```

The Tauri dev server can take longer than the installed app because it starts Vite, TypeScript, Rust compilation, and the Tauri shell.

## Windows Build

Use the MSVC route for the current OpenLess Unbound installer and portable package:

```powershell
cd openless-all/app
powershell -ExecutionPolicy Bypass -File .\scripts\windows-preflight.ps1 -Toolchain msvc
powershell -ExecutionPolicy Bypass -File .\scripts\windows-package-msvc.ps1
```

Generated artifacts:

```text
openless-all/app/.artifacts/windows-msvc/
```

## Related Docs

- Main README: `../README.md`
- Usage notes: `../USAGE.md`
- English README: `../README.en.md`
