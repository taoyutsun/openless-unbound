<p align="center">
  <img src="openless-all/app/src-tauri/icons/128x128@2x.png" alt="OpenLess Unbound" width="144" />
</p>

<h1 align="center">OpenLess Unbound</h1>

<p align="center">
  <strong>以 OpenLess 為基礎的自由化 fork。</strong><br/>
  讓語音輸入、AI 潤色、即時翻譯與劃詞追問更容易使用自己的 provider，並改善繁體中文 Windows 輸入體驗。
</p>

<p align="center">
  <a href="README.md">繁體中文</a> · <a href="README.en.md">English</a>
</p>

---

OpenLess Unbound 是基於 [OpenLess](https://github.com/Open-Less/openless) 的 GitHub fork。原開發者已經提供完整的跨平台語音輸入體驗：按下全域快捷鍵錄音，透過 ASR 轉文字，再交給 LLM 依照風格潤色，最後插入到目前游標所在的位置。

這個 fork 的方向不是重寫 OpenLess，而是針對實際使用時遇到的限制做整理：

- 「劃詞追問」不再強制依賴火山 ASR，可跟隨目前已設定的 ASR provider。
- 「劃詞追問」的回答模型可獨立設定，不必與一般潤色模型綁在一起。
- 支援 Codex OAuth、自訂 OpenAI-compatible endpoint 等 LLM 路由，讓使用者可依速度、品質、成本分工。
- Windows 輸入法後端改以繁體中文台灣 profile 註冊，降低語音輸入後被切到簡體中文輸入法的機率。
- 使用獨立 app identity 與資料夾，避免和官方 OpenLess 設定互相覆蓋。
- 保留原開發者資訊與上游來源，方便追蹤官方更新與回饋。

## 功能概覽

- **語音輸入**：全域快捷鍵啟動錄音，ASR 轉寫後插入目前輸入框。
- **AI 潤色**：可依「原文、輕度潤色、清晰結構、正式表述」等風格整理口語內容。
- **即時翻譯**：錄音中按翻譯快捷鍵，將轉寫內容翻譯成指定語言後插入。
- **劃詞追問**：選取文字後開啟浮窗，用語音或文字對選取內容提問。
- **詞彙表**：加入專有名詞、英文產品名、人名、地名，提高 ASR 與潤色時的命中率。
- **糾正規則**：用規則把常見誤識別修正成指定文字。
- **歷史記錄**：可檢視與重用過去的語音輸入結果。
- **本地優先**：設定與歷史主要保留在本機；API 請求只在使用雲端 ASR / LLM provider 時發生。

## 與上游 OpenLess 的主要差異

| 項目 | 上游 OpenLess | OpenLess Unbound |
| --- | --- | --- |
| 劃詞追問 ASR | 舊版流程容易要求火山 ASR 設定 | 跟隨目前啟用的 ASR provider |
| 劃詞追問 LLM | 與既有流程綁定較緊 | 可獨立選 provider、model、thinking |
| Windows IME | 曾在繁中使用者環境切到簡中 profile | TSF profile 改為繁體中文台灣 |
| App 身分 | `OpenLess` | `OpenLess Unbound`，獨立資料目錄 |
| 更新路線 | 官方上游 | fork 獨立維護，保留上游歸屬 |

## 安裝

請到本 repo 的 [Releases](../../releases) 頁面下載最新版本。

Windows 建議優先使用：

- `OpenLess_Unbound_<version>_x64.msi`：安裝版，需要系統安裝流程，適合日常使用。
- `OpenLess_Unbound_<version>_x64_portable.zip`：免安裝版，適合測試或臨時使用。

安裝後第一次啟動時，Windows 可能顯示 SmartScreen 或未識別發行者提醒，原因是目前尚未配置 Windows code signing 憑證。

## 基本設定

1. 開啟 OpenLess Unbound。
2. 到「設定 → 服務」設定 ASR provider，例如 Groq Whisper、OpenAI Whisper-compatible endpoint、Foundry Local Whisper、Sherpa-ONNX local；也可依需求使用火山、百煉等其他支援項目。
3. 到「設定 → 服務」設定 LLM 潤色模型，例如 Codex OAuth、Groq、OpenAI、Gemini、OpenRouter 或自訂 OpenAI-compatible endpoint。
4. 到「設定 → 通用」確認開始 / 停止錄音快捷鍵，預設為 Right Ctrl。
5. 到「詞彙表」加入常用專有名詞，例如產品名稱、英文工具、人名或品牌。
6. 到「劃詞追問」設定回答模型；可以沿用潤色模型，也可以獨立使用另一組 provider、model 與 thinking 設定。

## 一般語音輸入

1. 將游標放在想輸入文字的 app 或文字框。
2. 按 Right Ctrl 開始錄音，再按一次停止。
3. OpenLess Unbound 會先用 ASR 轉寫，再依目前「風格」設定交給 LLM 潤色。
4. 結果會自動插入游標位置；若目標 app 不接受直接插入，內容仍會保留在剪貼簿或 fallback 流程中。

常用專有名詞建議先加入「詞彙表」；固定誤識別可放在「糾正規則」。

## 即時翻譯

1. 到「翻譯」頁面選擇工作語言與目標語言。
2. 用 Right Ctrl 開始錄音。
3. 錄音中按一下 Shift 觸發翻譯模式。
4. 再按 Right Ctrl 停止錄音後，翻譯結果會插入游標位置。

如果目標語言設為「不啟用」，Shift 不會觸發翻譯。若翻譯失敗，程式會回退插入原始轉寫，避免內容遺失。

## 劃詞追問

1. 按 `Ctrl+Shift+;` 開啟劃詞追問浮窗。
2. 在任意 app 選取一段文字。
3. 按 Right Ctrl 開始錄音，再按一次送出。
4. 可繼續按 Right Ctrl 多輪追問。
5. 按 Esc 關閉浮窗並清空暫存。

注意：目前劃詞追問本身不內建瀏覽器或即時網路搜尋工具。若模型回覆「無法查詢即時資訊」，那是 provider / model 本身能力限制，不是熱鍵或浮窗故障。

## Windows 輸入法說明

OpenLess Unbound 內含 Windows TSF 輸入法後端，用來把語音轉出的文字插入目前 app。這個 fork 將 TSF profile 註冊語系改為繁體中文台灣，目標是避免繁中使用者在語音輸入後被切到「簡體中文（中國）」。

## 從原始碼執行

本 repo 的 Git root 是專案根目錄，實際 Tauri app 在 `openless-all/app/`。

```powershell
git submodule update --init --recursive
cd openless-all/app
npm ci
npm run tauri -- dev
```

Windows 打包建議使用 MSVC 路線：

```powershell
cd openless-all/app
powershell -ExecutionPolicy Bypass -File .\scripts\windows-preflight.ps1 -Toolchain msvc
powershell -ExecutionPolicy Bypass -File .\scripts\windows-package-msvc.ps1
```

打包產物會輸出到：

```text
openless-all/app/.artifacts/windows-msvc/
```

## 專案結構

```text
openless-src/
  README.md                 # 繁中首頁 README
  README.en.md              # 英文輔助 README
  USAGE.md                  # 使用與設定補充
  openless-all/
    app/                    # Tauri 2 + React + Rust 主程式
      src/                  # 前端
      src-tauri/            # Rust 後端與打包設定
      windows-ime/          # Windows TSF IME 後端
      scripts/              # 建置、測試、匯入工具
```

## 上游與授權

OpenLess Unbound fork 自 [OpenLess](https://github.com/Open-Less/openless)。感謝原開發者與社群提供基礎架構、介面與跨平台語音輸入能力。

本專案沿用 MIT License。Fork 維護與新增內容同樣以 MIT License 發布。
