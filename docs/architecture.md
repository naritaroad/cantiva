# Cantiva 架構

版本 0.1.0，2026-11-05。

## 元件

| 元件 | 語言 | 職責 |
|---|---|---|
| `crates/cantiva-core` | Rust | 資料模型、SQLite 儲存、設定、腳本解析、長文分段、路徑安全。不碰 UI、網路或程序 |
| `crates/cantiva-audio` | Rust | WAV 讀寫、合併、修剪靜音、線性重新取樣、EBU R128 響度、波形峰值 |
| `crates/cantiva-engine` | Rust | 讀取 `engines/*/engine.toml`、啟動與監看 Python worker、`Studio` 服務 |
| `crates/cantiva-api` | Rust | 本機控制 API（axum，只綁 127.0.0.1，Bearer 權杖）與客戶端 |
| `app` | Rust + Slint | 桌面介面、播放（rodio）、Cloudflare 通道；`--headless` 只跑 Studio 與 API |
| `mcp` | Rust | `cantiva-mcp`：rmcp 3.8 的 MCP server，stdio 或 Streamable HTTP |
| `engines/_worker` | Python（標準函式庫） | 共用 worker：本機 HTTP 協定、自我測試 |
| `engines/<id>/adapter.py` | Python | 把各模型的 Python API 接到 worker 協定 |

## 執行流程

1. 使用者在介面按「合成」，或 agent 呼叫 MCP 的 `speak`。
2. `cantiva-mcp` 讀取資料夾裡的 `api-port` 與 `api-token`，呼叫控制 API。Cantiva 沒開時，會先以 `--headless` 啟動它。
3. `Studio::speak` 解析聲音名稱，取得引擎與說話人（或參考音檔與角色模型權重），再依引擎的 `chunk_chars` 把長文分段。
4. 拿到 worker 鎖：需要的模型沒載入時，先關掉目前的 worker（釋放顯示記憶體），再用該引擎的 venv 啟動新 worker，等它回報 ready。
5. 逐段呼叫 `POST /synthesize`，worker 把 WAV 寫到暫存資料夾。
6. Rust 端修剪每段頭尾靜音，以 0.22 秒間隔合併，做響度正規化，存到專案資料夾，寫入 SQLite，並廣播 `TakeCreated`。
7. 介面收到事件後更新時間軸，所以 agent 產生的片段也會即時出現。

### Worker 協定

- 只綁 127.0.0.1，連接埠隨機。
- 每次啟動產生新權杖，透過環境變數 `CANTIVA_WORKER_TOKEN` 傳入；不放在命令列，因為其他程式看得到命令列。
- 端點：
  - `GET /health`：回報 loading、ready 或 error。
  - `GET /info`
  - `POST /synthesize`：請求內容是 SegmentRequest。
  - `POST /shutdown`
- `python -m cantiva_worker --selftest`：安裝腳本最後一步會載入模型；有內建說話人的模型還會試合成一句，用來確認環境可用。

### 對外通道

- 啟動順序：
  1. 介面先啟動 `cantiva-mcp --tunnel --port 0`。它綁定系統挑選的連接埠，並在 stdout 印出 `CANTIVA_PORT <n>`。
  2. 介面確認 MCP 真的在監聽後，才啟動 `cloudflared tunnel --url` 指向它，再從輸出抓出 `*.trycloudflare.com` 網址。
  3. 任一子程序結束，另一個也會關掉，介面會顯示通道已關閉。
- 驗證方式：
  - 每次開啟通道都產生新的 40 字元密鑰，透過環境變數 `CANTIVA_TUNNEL_SECRET` 交給 MCP。它不寫進檔案，也不和本機 HTTP 模式的 `mcp-secret` 共用。
  - ChatGPT 的遠端 MCP 只支援 OAuth 或不驗證，不支援固定 Bearer 權杖，所以密鑰放在網址路徑（`/mcp/<secret>`）。其他客戶端可以改用 `/mcp` 加上 `Authorization: Bearer <secret>`。
- Cloudflare 臨時通道不支援 SSE，所以 MCP 回應一律用 JSON（`with_json_response(true)`）。
- 通道模式的權限比較小，由 MCP 與控制 API 兩邊各自把關：
  - MCP 端移除 `create_voice`、`play_take`、`load_engine`、`unload_engine` 四個工具。
  - 控制 API 也會拒絕通道來源建立聲音、播放、載入或卸載模型。
  - 使用複製的聲音要同時開啟「允許 agent 使用複製聲音」與「允許透過通道使用」。
  - 回傳內容只有檔名，不含本機路徑與參考音檔的逐字稿。
  - 腳本一次最多 6000 字、200 行。

### 單一實例

- `instance.lock` 用作業系統的檔案鎖（`File::try_lock`）。程式當掉時鎖會自動釋放。
- 第二個 `cantiva --headless` 拿不到鎖就直接結束，不會搶走控制 API 或另外載入模型。
- 開主視窗時，如果無視窗模式正在跑，主視窗會先請它結束，等拿到鎖再接手。
- 健康檢查（`/v1/health`）不等待合成用的 worker 鎖。所以就算正在載入大模型，也不會被誤判成「沒有在執行」。

### Worker 生命週期

- Rust 端用 `kill_on_drop` 結束 worker。
- worker 也會透過 `CANTIVA_PARENT_PID` 監看 Cantiva（Windows 用 `OpenProcess` 加 `WaitForSingleObject`）。Cantiva 被強制結束時，worker 會自己退出並釋放顯示記憶體。

## 技術選擇評估

### Rust 評估

| 元件 | 決定 | 理由 | 整合方式 |
|---|---|---|---|
| 桌面介面 | Rust + Slint | 原生、小、啟動快；和作者既有的 qrcode-tool 同一套，降低維護成本 | Rust 主程式 |
| 音訊處理、排程、API、MCP | Rust | 常駐服務，重視記憶體安全與低佔用；rmcp 是官方 SDK | Rust 主程式 |
| 模型推論 | **不用 Rust**，改用 Python | 五個模型都只有 PyTorch 實作，Rust 沒有成熟的替代 | Python sidecar 程序（層級 2） |

改變決定的條件：模型有了 ONNX 或 candle 版本且品質相同，就把對應的 worker 換成 Rust。

### 資料庫評估（PostgreSQL 優先原則）

- **選 SQLite**（rusqlite 0.41，bundled）。單機、單一使用者的桌面程式，不需要另裝伺服器。WAL 模式下，GUI 與無視窗模式可以短暫同時開著。
- 音檔與權重放在檔案系統，資料庫只存路徑與中繼資料。
- 結構變更透過 `PRAGMA user_version` 做遷移，目前到 v2。
- 改用 PostgreSQL 的條件：多人共用一個聲音庫，或部署成伺服器。

### Docker 評估

- **不用 Docker。** 需要直接存取 GPU、音訊裝置與桌面視窗。
- Windows 上的 Docker Desktop 會多開一個 WSL 2 VM，佔用一半記憶體上限，8 GB 顯示卡的機器負擔不起。
- 依賴隔離改用每個模型各自的 `uv` venv，加上固定版本的 PyTorch。
- 改用 Docker 的條件：要在 Linux 伺服器上提供多人服務。

### 既有開源方案評估

| 能力 | 候選 | 決定 |
|---|---|---|
| MCP server | rmcp 3.8（官方 Rust SDK） | 採用 |
| 本機 TTS 伺服器 | Kokoro-FastAPI、vLLM-Omni、Xinference | 參考協定設計，不直接採用：vLLM-Omni 只能在 Linux 上執行；Xinference 沒有 BreezyVoice 與 GPT-SoVITS 的 v2ProPlus 角色模型 |
| 現成 TTS MCP | giannisanni/kokoro-tts-mcp 等 | 不採用：只支援 Kokoro，沒有多模型與複製聲音 |
| 播放 | rodio 0.23 | 採用（只開 playback 功能） |
| 響度 | ebur128 | 採用 |
| 重新取樣 | rubato | 不採用：語音頻寬在 12 kHz 以下，線性內插就夠，不必多一個相依套件 |

## 已知限制

- 第一版只有 Windows 安裝腳本。Rust 程式在 Linux 上可以編譯與測試，但沒有 Linux 的模型安裝腳本。
- CosyVoice 3、IndexTTS、BreezyVoice、GPT-SoVITS 的安裝腳本會抓官方 `main` 分支，並把實際的 commit 記在 `installed.json`。官方日後改動可能讓 adapter 失效，所以 adapter 都把 import 與參數集中在檔案開頭，方便修改。
- 只匯出 WAV。要 MP3 或 AAC，請另外用 ffmpeg 轉檔。
