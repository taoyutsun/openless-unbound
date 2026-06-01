# OpenLess Unbound 使用補充

這份文件補充 `README.md` 未展開的日常使用細節。首次使用請先閱讀 `README.md`，再依照本文件調整日常工作流。

## 安裝版與免安裝版

- `OpenLess_Unbound_<version>_x64.msi`：安裝到系統，會註冊必要的 Windows 輸入法後端，適合長期使用。
- `OpenLess_Unbound_<version>_x64_portable.zip`：免安裝版，適合短期測試或驗證設定。

Windows 的 TSF 輸入法後端需要註冊到系統。若你要長期使用語音輸入，建議使用 MSI 安裝版。

## 服務設定

OpenLess Unbound 的服務設定主要分成兩類：

- ASR：把語音轉成文字，例如 Groq Whisper、OpenAI Whisper-compatible endpoint、Foundry Local Whisper、Sherpa-ONNX local，或其他支援 provider。
- LLM：把文字潤色、整理、翻譯，或回答劃詞追問，例如 Codex OAuth、Groq、OpenAI、Gemini、OpenRouter 或自訂 OpenAI-compatible endpoint。

一般語音輸入可以使用速度較快、成本較低的模型；劃詞追問可以獨立改用品質較高或更適合推理的模型。

## 一般語音輸入

預設流程：

1. 把游標放到目標 app 的文字輸入位置。
2. 按 Right Ctrl 開始錄音。
3. 再按 Right Ctrl 停止錄音。
4. ASR 轉寫完成後，LLM 會依目前「風格」設定潤色。
5. 結果會自動插入游標位置。

如果插入目標是終端機或特殊 app，可到「設定 → 通用 → 插入與剪貼板」調整模擬貼上快捷鍵，例如 `Ctrl+V`、`Ctrl+Shift+V` 或 `Shift+Insert`。

錄音模式會影響 Right Ctrl 的使用方式：

- 按住說話 / Hold：按住 Right Ctrl 時錄音，放開後停止。若只是快速點一下，送出的音訊太短，可能顯示「沒有識別到語音」。
- 切換 / Toggle：按一次開始錄音，再按一次停止，適合長段口述。

`v1.3.6-1` 起，Groq / OpenAI Whisper 會使用 verbose JSON 過濾極短音訊或靜音造成的幻覺片段。這能減少無聲音時出現 `Thank you.` 之類誤判，但太短的錄音仍會被視為沒有有效語音。

## 即時翻譯

預設流程：

1. 到「翻譯」頁面選擇工作語言。
2. 選擇翻譯目標語言；若選「不啟用」，Shift 不會觸發翻譯。
3. 按 Right Ctrl 開始錄音。
4. 錄音中按一下 Shift，畫面底部會顯示藍色「正在翻譯」標識。
5. 再按 Right Ctrl 停止錄音，翻譯結果會插入游標位置。

翻譯失敗時會回退為插入原始轉寫，不會直接丟失內容。

## 劃詞追問

預設流程：

1. 按 `Ctrl+Shift+;` 開啟浮窗。
2. 選取 app 內文字。
3. 按 Right Ctrl 開始錄音。
4. 再按 Right Ctrl 停止錄音並送出。
5. 可繼續按 Right Ctrl 追問。
6. 按 Esc 關閉浮窗。

目前劃詞追問不內建即時上網搜尋。如果你問匯率、天氣、新聞等即時資訊，模型能否回答取決於你選用的 provider 與模型能力。

## 詞彙表

詞彙表適合放：

- 人名、地名、公司名
- 英文工具與產品名，例如 Codex、Claude Code、OpenClaw
- 專案名、部落格名、品牌名
- 容易被 ASR 誤判的中文或中英混合詞

OpenLess Unbound 會把啟用的詞彙送入 ASR prompt 或 LLM prompt，並在輸出命中時累加 hits。

## Windows 輸入法

OpenLess Unbound 的 Windows TSF 後端註冊為繁體中文台灣 profile，目標是避免語音輸入後切到簡體中文輸入法。

如果遇到輸入法後端顯示不可用，通常表示 TSF 註冊沒有完成或被 Windows 清掉。請重新執行 MSI 安裝版，或安裝新版 release。

## 本機資料位置

OpenLess Unbound 的使用者資料預設在：

```text
%APPDATA%\OpenLess Unbound
```

常見檔案：

- `preferences.json`：偏好設定。
- `history.json`：歷史紀錄。
- `dictionary.json`：詞彙表，可自行備份；分享前請確認沒有私人資訊。

## 批次匯入詞彙

本 repo 提供匯入輔助腳本，可把文字、CSV 或剪貼簿詞彙匯入 OpenLess Unbound：

```powershell
cd openless-all/app
powershell -ExecutionPolicy Bypass -File .\scripts\import-openless-unbound-vocab.ps1 -InputFile .\terms.txt
```

匯入前腳本會去重；若既有詞條被停用，會重新啟用。已有 `dictionary.json` 時會先備份。
