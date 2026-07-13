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
- 「劃詞追問」回答氣泡可手動選取局部文字，也可一鍵複製整則回答。
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

## 近期同步內容

`v1.3.14-1` 選擇性整合上游 OpenLess 1.3.14 中適合 Windows 與 Unbound 主線的修正，並保留本 fork 的既有功能：

- Windows Foundry Local Whisper 更新至相容的 1.2.1 runtime，長錄音會以 30 秒片段辨識並合併，等待時間也會依音訊長度調整。
- Windows 輸入法 IPC 改用可取消的 overlapped I/O，降低關閉或重新啟動程式時背景執行緒卡住的機率；繁體中文台灣 TSF profile 維持不變。
- 自訂 OpenAI-compatible LLM provider 可設定額外 HTTP Headers，套用於一般潤色、模型清單與劃詞追問的獨立自訂 provider。
- 強化錄音提示音恢復、設定寫入競態保護，以及歷史記錄原文／潤色結果的複製行為。

本次仍不納入 Android APK runtime、手機遠端輸入、Less Computer / Cloud Agent 與大規模 mobile UI，避免增加目前 Windows 版本不需要的依賴與風險。

`v1.3.11-2` 修正繁體中文強制字形轉換使用一般 `s2t` 導致「吃」被轉成「喫」的問題，改用台灣繁體字形轉換，讓「吃飯」維持台灣日常用字；同時修正一般語音輸入在介面語言為繁體中文時，LLM 潤色階段可能把英文口語內容翻成中文的問題。介面語言現在只同步中文字形偏好；未啟用翻譯時，語音輸入會保留原本說出的語言與中英夾雜內容。

`v1.3.11-1` 是一個低風險 ASR 穩定性同步版本，先整合已完成測試的 provider 與 timeout 改善，同時保留本 fork 的劃詞追問 provider 可配置性、Codex OAuth、繁體中文 TSF profile 與 portable DLL 打包修正：

- 新增 OpenRouter ASR provider，可使用 OpenRouter 的 Whisper-compatible 轉寫服務。
- Whisper-compatible ASR 的等待時間改為依錄音長度動態調整，並將全域兜底 timeout 從 15 秒提高到 30 秒，降低較長錄音或網路較慢時過早失敗的機率。
- 劃詞追問使用 Whisper-compatible ASR 時，也套用同一組較穩定的 timeout。
- ASR provider 下拉選單中的 Groq 顯示為 `Groq`，模型名稱交由「模型」欄位設定。

`v1.3.11-1` 並未整包同步官方 1.3.11 的 Android、手機遠端輸入、Less Computer、大型 UI/theme 重構，這些功能會在後續版本分批評估。

`v1.3.6-1` 起，OpenLess Unbound 已整合部分上游 OpenLess 1.3.6 beta 改善：

- 本地 ASR 模型儲存管理。
- 多螢幕環境下，膠囊提示跟隨目前輸入所在螢幕。
- Toggle 錄音結束後加入短暫冷卻，降低誤觸後立即重新錄音的機率。
- 支援 MediaPlayPause 熱鍵觸發。
- Groq / OpenAI Whisper verbose JSON 過濾，降低極短音訊或靜音時產生幻覺文字的機率。

## 安裝

請到本 repo 的 [Releases](../../releases) 頁面下載最新版本。

Windows 建議優先使用：

- `OpenLess_Unbound_<version>_x64.msi`：安裝版，需要系統安裝流程，適合日常使用。
- `OpenLess_Unbound_<version>_x64_portable.zip`：免安裝版，適合測試或臨時使用。

安裝後第一次啟動時，Windows 可能顯示 SmartScreen 或未識別發行者提醒，原因是目前尚未配置 Windows code signing 憑證。

## 基本設定

1. 開啟 OpenLess Unbound。
2. 到「設定 → 服務」設定 ASR provider，例如 Groq、OpenRouter Whisper、OpenAI Whisper-compatible endpoint、Foundry Local Whisper、Sherpa-ONNX local；也可依需求使用火山、百煉等其他支援項目。
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
5. 可手動選取回答中的局部文字，或按回答旁邊的複製按鈕複製整則回答。
6. 按 Esc 關閉浮窗並清空暫存。

注意：目前劃詞追問本身不內建瀏覽器、即時網路搜尋或外部 tool 調用流程。它適合針對已選取文字做解釋、整理、改寫與延伸提問，但不會即時上網查匯率、天氣或新聞。

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
