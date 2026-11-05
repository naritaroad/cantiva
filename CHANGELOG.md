# 變更紀錄

格式依 [Keep a Changelog 2.0.0](https://keepachangelog.com/en/2.0.0/)，版本號依 [SemVer 2.0.0](https://semver.org/spec/v2.0.0.html)。

## [Unreleased]

### Changed

- Windows 的預設資料夾從 `%APPDATA%\Naritaroad\Cantiva\data` 改成 `%USERPROFILE%\Cantiva`。從 Claude 桌面版這類 MSIX 套件程式啟動的程序寫入 AppData 時，會被轉到該程式的私有資料夾，造成 agent 合成的音檔與裝好的引擎在 Cantiva 裡看不到。0.1.0 的資料請整個資料夾搬到新位置；或設定 `CANTIVA_HOME` 指向舊位置。

### Fixed

- `setup.ps1` 重跑時，如果 `cantiva-mcp.exe` 或 `cantiva.exe` 正被 agent（例如 Cursor）開著，不再因為無法覆蓋而失敗：舊檔改名移開後放上新版。
- `setup.ps1` 從其他應用程式（例如 Claude 桌面版）裡執行時，會偵測開始功能表捷徑是否被轉到私有資料夾，並提示改在一般 PowerShell 重跑。
- 無視窗模式收到結束要求時，記錄改寫成「收到結束要求」，不再一律寫成主視窗接手。

- Windows：`engines/_worker` 的自我測試單元測試改用 UTF-8 讀取子程序輸出，不再因系統字碼頁（cp950）解碼失敗。
- Windows：`studio_dev_engine` 整合測試不再跳過，改用 `python -m venv` 建立假引擎的 venv，完整流程在 Windows 上也會測到 worker 啟動與合成。
- Windows：`tests/e2e_mcp.py` 可以在 Windows 執行，結束時改用控制 API 的 `/v1/shutdown` 關閉無視窗模式，不再依賴 `pkill`。

## [0.1.0] - 2026-11-05

### Added

- 桌面程式（Slint 1.18）：聲音庫、腳本、時間軸、輸出、引擎、連線六頁，DaVinci Resolve 式的頁面列與深色介面。
- 五個 TTS 引擎：Qwen3-TTS、GPT-SoVITS v2ProPlus、Fun-CosyVoice 3、IndexTTS 2.5、BreezyVoice。每個引擎有獨立的 uv venv 與 Windows 安裝腳本，安裝後會自我測試。
- GPT-SoVITS 角色模型：可以匯入訓練好的 GPT（`.ckpt`）與 SoVITS（`.pth`）權重，存成聲音庫的一個聲音；合成時自動切換權重。
- 腳本格式：`@聲音：{語氣}台詞`、`#` 註解、空行分段。
- 時間軸：多軌、波形、播放頭與音量表；可以調整片段順序、重新合成、標星號、移除。
- 輸出：依時間軸順序合併成一個 WAV，自動統一取樣率並做響度正規化。
- 同意紀錄：建立複製聲音時必須選擇同意依據並寫下說明。
- `cantiva-mcp`：10 個 MCP 工具，支援 stdio 與 Streamable HTTP；Cantiva 沒開時自動以無視窗模式啟動。
- 對外通道（預設關閉）：Cloudflare 臨時通道，網址含密鑰，工具權限縮小。
- 配套 skill `cantiva-voice`，以及 `register-mcp.ps1`：可以註冊到 Claude Code、Codex、Cursor、Grok Build CLI，並安裝 skill。
- `setup.ps1`：用 winget 補齊工具、編譯、組成 dist 資料夾並建立捷徑。

### Security

- 對外通道每次開啟都用新的一次性密鑰；確認 MCP 真的在監聽後才啟動 cloudflared。
- 通道模式的限制改由控制 API 把關：不能建立聲音、播放、載入或卸載模型；回傳內容不含本機路徑與逐字稿。
- 單一實例鎖，避免第二個 Cantiva 同時載入另一個模型。
- 引擎 worker 監看 Cantiva 程序，Cantiva 當掉時 worker 自己結束，不會殘留佔用顯示記憶體。

[Unreleased]: https://github.com/naritaroad/cantiva/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/naritaroad/cantiva/releases/tag/v0.1.0
