<div align="center">

<img src="docs/assets/banner.svg" alt="Cantiva：本機語音工作室，開源 TTS 模型、多軌時間軸，以及給 AI agent 用的 MCP server" width="100%">

<br>

[架構](docs/zh-TW/architecture.md) · [變更紀錄](CHANGELOG.md) · [問題回報](https://github.com/naritaroad/cantiva/issues) · [安全性](SECURITY.md)

[![CI](https://img.shields.io/github/actions/workflow/status/naritaroad/cantiva/ci.yml?branch=main&style=flat-square&label=ci&labelColor=1c1c1f)](https://github.com/naritaroad/cantiva/actions/workflows/ci.yml)
![version](https://img.shields.io/badge/version-0.1.0-e3a23b?style=flat-square&labelColor=1c1c1f)
![platform](https://img.shields.io/badge/platform-Windows%2010%2F11-1c1c1f?style=flat-square)
![rust](https://img.shields.io/badge/rust-%E2%89%A51.92-1c1c1f?style=flat-square&logo=rust)
![MCP](https://img.shields.io/badge/MCP-rmcp%203.8-1c1c1f?style=flat-square)
[![license](https://img.shields.io/badge/license-MIT-6b6b6b?style=flat-square&labelColor=1c1c1f)](LICENSE)

[English](README.md) | **繁體中文**

</div>

---

Cantiva 是 Windows 桌面程式，用你自己的顯示卡跑開源 TTS 模型。寫好腳本後，Cantiva 用你挑的聲音逐行合成，把片段排在多軌時間軸上，再匯出成一個 WAV。同一套功能也透過 MCP server 提供給 Claude、Codex、Cursor 與 Grok，讓 agent 直接在你的電腦上做旁白或廣播劇。

模型、聲音與音檔都留在這台電腦。介面配置參考 DaVinci Resolve 的頁面列與深色面板，介面文字是繁體中文。

## 功能

- **五個模型，一次只佔一個的顯示記憶體**：Qwen3-TTS、GPT-SoVITS（v2ProPlus）、Fun-CosyVoice 3、IndexTTS 2.5、BreezyVoice。每個模型有獨立的 Python 虛擬環境，套件版本互不衝突；換模型時自動卸載前一個。
- **腳本式多角色合成**：用 `@聲音：{語氣}台詞` 的格式寫腳本，一段一個片段，依序排上時間軸。
- **時間軸與檢查器**：每個聲音一條軌道，片段帶波形；可以調整順序、重新合成單一片段、標星號，並用音量表監聽。
- **聲音庫與同意紀錄**：用 3–15 秒參考音檔複製聲音（GPT-SoVITS 為 3–10 秒），或匯入自己訓練的 GPT-SoVITS 角色模型（`.ckpt` 加 `.pth`）。每個複製聲音都必須選擇同意依據並寫下說明。
- **輸出**：依時間軸順序接成一個 WAV，自動統一不同模型的取樣率，並依 EBU R128 做響度正規化（預設 −16 LUFS）。
- **MCP server**：`cantiva-mcp` 提供 10 個工具，例如 `speak`、`render_script`、`list_voices`、`create_voice`，支援 stdio 與 Streamable HTTP。Cantiva 沒開時會自動以無視窗模式啟動。
- **對外通道（預設關閉）**：透過 Cloudflare 臨時通道讓 ChatGPT 等雲端 agent 連線。網址帶有一次性密鑰，通道來源的工具權限會縮小。
- **配套 skill**：[`skill/cantiva-voice`](skill/cantiva-voice/SKILL.md) 告訴 agent 怎麼挑模型、寫腳本、處理同意紀錄。

## 畫面

<img src="docs/assets/timeline.png" alt="時間軸頁：兩條軌道、帶波形的片段與檢查器" width="100%">

<details>
<summary>其他頁面</summary>

<img src="docs/assets/script.png" alt="腳本頁" width="100%">
<img src="docs/assets/voices.png" alt="聲音庫頁" width="100%">
<img src="docs/assets/engines.png" alt="引擎頁" width="100%">
<img src="docs/assets/connect.png" alt="連線頁" width="100%">

</details>

截圖是用內建的開發測試引擎產生的（波形是模擬音節，不是真的語音）。

## 架構

```mermaid
flowchart LR
  subgraph PC[你的電腦]
    GUI[cantiva.exe<br>Slint 介面] --- Studio
    Headless[cantiva --headless] --- Studio
    Studio[Studio<br>工作佇列、聲音庫、SQLite] -->|本機 HTTP| W[Python worker<br>一次一個模型]
    API[控制 API<br>127.0.0.1 + 權杖] --- Studio
    MCP[cantiva-mcp] -->|HTTP| API
    Agents[Claude Code／Desktop<br>Codex、Cursor、Grok CLI] -->|stdio| MCP
  end
  Cloud[ChatGPT 等雲端 agent] -.->|Cloudflare 通道<br>預設關閉| MCP
```

Rust 負責介面、音訊處理、工作排程與 MCP；Python 只負責呼叫模型。每個模型跑在自己的 worker 程序裡，透過只綁 127.0.0.1 的 HTTP 和 Cantiva 溝通。執行流程、worker 協定、通道的安全設計與技術選擇的評估見 [docs/zh-TW/architecture.md](docs/zh-TW/architecture.md)。

<details>
<summary>引擎一覽</summary>

| 引擎 | 擅長 | 顯示記憶體 | 授權 |
|---|---|---|---|
| Qwen3-TTS | 9 個內建聲音、用自然語言指定語氣 | 約 7 GB（1.7B） | Apache-2.0 |
| GPT-SoVITS v2ProPlus | 日文、角色模型、3–10 秒複製 | 約 4 GB | MIT |
| Fun-CosyVoice 3 | 跨語言複製、中文方言 | 約 5 GB | Apache-2.0 |
| IndexTTS 2.5 | 情緒控制（文字或八維向量） | 約 6 GB | bilibili 模型協議 |
| BreezyVoice | 台灣華語、注音標破音字 | 約 4 GB | Apache-2.0 |

顯示記憶體是依官方資料與參數量估計，實際用量以「引擎」頁顯示為準。各引擎支援的功能見 [skill/cantiva-voice/reference/engines.md](skill/cantiva-voice/reference/engines.md)。

</details>

<details>
<summary>資料放在哪裡</summary>

| 內容 | 位置 |
|---|---|
| 設定、資料庫、記錄檔 | `%USERPROFILE%\Cantiva` |
| 各模型的虛擬環境與權重 | `…\Cantiva\engines\<引擎>` |
| 合成的音檔 | `…\Cantiva\takes\<專案>`（可在設定中修改） |
| 參考音檔與角色模型 | `…\Cantiva\references` |

設定環境變數 `CANTIVA_HOME` 可以把整個資料夾搬到別處。不放在 `%APPDATA%` 是因為從 Claude 桌面版這類 MSIX 套件程式啟動的程序寫入 AppData 時，會被轉到該程式的私有資料夾，Cantiva 看不到。

</details>

<details>
<summary>腳本格式</summary>

```text
# 第一幕
@旁白：{低沉、緩慢}夜深了，港口只剩下最後一盞燈。

@qwen3-tts/Serena：你還在等那艘船嗎？
```

- 空行結束一個片段。
- `@名稱：`（或 `@名稱:`）指定聲音庫的聲音，或內建的 `引擎/說話人`。
- 緊接在標籤後的 `{…}` 是語氣指示，只在支援的引擎上有作用。
- `#` 開頭的行是註解。

完整規則見 [skill/cantiva-voice/reference/script-format.md](skill/cantiva-voice/reference/script-format.md)。

</details>

## 安裝

需要 Windows 10/11 與 NVIDIA 顯示卡（建議 8 GB 以上顯示記憶體）。在專案根目錄開 PowerShell：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\setup.ps1
```

`setup.ps1` 會用 winget 補齊缺少的工具（Git、Rust、Visual Studio Build Tools、uv、CMake、cloudflared），編譯到 `dist\Cantiva` 並建立開始功能表捷徑。重跑一次就是更新。

之後：

1. 開啟 Cantiva，到「引擎」頁安裝模型。每個模型會開一個 PowerShell 視窗下載，需要幾 GB 空間，最後會跑自我測試。
2. 註冊給 AI agent 使用。不加參數就只印出設定、不改任何檔案。

   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts\register-mcp.ps1 -Claude -Codex -Cursor -Grok -Skill
   ```

   Claude 桌面版請照 `cantiva-mcp --print-config` 的輸出手動加入，改完重新開啟。

## 專案結構

```
cantiva/
├── app/                 桌面程式（Slint 介面、播放、通道）
│   ├── ui/              .slint 介面檔
│   └── src/gui/         介面控制器
├── mcp/                 cantiva-mcp（rmcp）
├── crates/
│   ├── cantiva-core/    資料模型、SQLite、腳本解析、設定、路徑安全
│   ├── cantiva-audio/   WAV、合併、重新取樣、響度、波形
│   ├── cantiva-engine/  模型目錄、worker 管理、Studio
│   └── cantiva-api/     本機控制 API 與客戶端
├── engines/             各模型的 adapter.py、engine.toml 與 install.ps1
│   └── _worker/         共用的 Python worker（只用標準函式庫）
├── skill/cantiva-voice/ 給 agent 的配套 skill
├── scripts/             setup、register-mcp、install-engine
├── tests/               端到端測試與截圖腳本
└── docs/                架構說明、繁中翻譯、圖檔
```

## 聲音與授權

複製聲音只用在你本人、已取得同意的人，或有授權的素材。Cantiva 不附帶任何真人或角色的聲音。各模型與預訓練權重有自己的授權，其中 IndexTTS 2.5 對大型商業使用者有限制，見 [第三方授權說明](docs/zh-TW/THIRD_PARTY_NOTICES.md)。

## 參與貢獻

歡迎回報問題與送 pull request。編譯方式、測試指令與專案規則（一次只載入一個模型、複製聲音必須有同意紀錄、本機服務只綁 127.0.0.1）見 [CONTRIBUTING.md](CONTRIBUTING.md)。沒有顯示卡也能用 `dev-sine` 假引擎跑完整流程。

安全性問題請依 [SECURITY.md](SECURITY.md) 私下回報。

## 授權

MIT，見 [LICENSE](LICENSE)。介面使用 Slint 的免版稅授權 2.0，「關於」視窗依授權條款顯示 Slint 標示。
