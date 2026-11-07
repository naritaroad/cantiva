# Cantiva architecture

**English** | [繁體中文](zh-TW/architecture.md)

Version 0.1.0, 2026-11-05.

## Components

| Component | Language | Responsibility |
|---|---|---|
| `crates/cantiva-core` | Rust | Data models, SQLite store, settings, script parsing, long-text splitting, path safety. No UI, network or processes. |
| `crates/cantiva-audio` | Rust | WAV I/O, concatenation, silence trimming, linear resampling, EBU R128 loudness, waveform peaks. |
| `crates/cantiva-engine` | Rust | Reads `engines/*/engine.toml`, starts and supervises Python workers, the `Studio` service. |
| `crates/cantiva-api` | Rust | Local control API (axum, bound to 127.0.0.1, Bearer token) and its client. |
| `app` | Rust + Slint | Desktop UI, playback (rodio), Cloudflare tunnel. `--headless` runs only Studio and the API. |
| `mcp` | Rust | `cantiva-mcp`: an rmcp 3.8 MCP server over stdio or Streamable HTTP. |
| `engines/_worker` | Python (stdlib) | Shared worker: local HTTP protocol, self-test. |
| `engines/<id>/adapter.py` | Python | Connects one model's Python API to the worker protocol. |

## Request flow

1. The user clicks Synthesize in the UI, or an agent calls the MCP tool `speak`.
2. `cantiva-mcp` reads `api-port` and `api-token` from the data folder and calls the control API. If Cantiva isn't running, it first starts it with `--headless`.
3. `Studio::speak` resolves the voice name to an engine and a speaker (or a reference clip plus character-model weights), then splits long text by the engine's `chunk_chars`.
4. It takes the worker lock. If the needed model isn't loaded, it stops the current worker (freeing VRAM), starts a new worker from that engine's venv and waits for it to report ready.
5. It calls `POST /synthesize` once per chunk; the worker writes WAVs to a temporary folder.
6. The Rust side trims leading and trailing silence from each chunk, joins them with 0.22 s gaps, normalizes loudness, saves the result to the project folder, writes it to SQLite and broadcasts `TakeCreated`.
7. The UI updates the timeline on that event, so clips made by an agent show up immediately.

### Worker protocol

- Bound to 127.0.0.1 on a random port.
- A new token is generated on every start and passed in the `CANTIVA_WORKER_TOKEN` environment variable. It is never put on the command line, because other programs can read command lines.
- Endpoints:
  - `GET /health`: reports loading, ready or error.
  - `GET /info`
  - `POST /synthesize`: the body is a SegmentRequest.
  - `POST /shutdown`
- `python -m cantiva_worker --selftest`: the last step of every install script. It loads the model and, for models with built-in speakers, synthesizes one sentence to confirm the environment works.

### Tunnel

- Startup order:
  1. The UI starts `cantiva-mcp --tunnel --port 0`. It binds a port picked by the OS and prints `CANTIVA_PORT <n>` on stdout.
  2. Only after confirming the MCP server is listening does the UI start `cloudflared tunnel --url` pointing at it, then reads the `*.trycloudflare.com` URL from its output.
  3. When either child process exits, the other is stopped and the UI shows the tunnel as closed.
- Authentication:
  - Each time the tunnel opens, a new 40-character secret is generated and handed to the MCP server in `CANTIVA_TUNNEL_SECRET`. It is never written to disk and is not shared with `mcp-secret`, the secret used by local HTTP mode.
  - ChatGPT's remote MCP supports only OAuth or no authentication, not a fixed Bearer token, so the secret goes in the URL path (`/mcp/<secret>`). Other clients can use `/mcp` with `Authorization: Bearer <secret>`.
- Cloudflare quick tunnels don't support SSE, so MCP responses are always JSON (`with_json_response(true)`).
- Tunnel mode has fewer permissions, enforced separately by the MCP server and the control API:
  - The MCP server drops four tools: `create_voice`, `play_take`, `load_engine` and `unload_engine`.
  - The control API also refuses tunnel-origin requests to create voices, play audio, or load or unload models.
  - Using a cloned voice requires both "allow agents to use cloned voices" and "allow over the tunnel" to be enabled.
  - Responses contain file names only, never local paths or reference-clip transcripts.
  - A script is limited to 6000 characters and 200 lines per request.

### Single instance

- `instance.lock` uses an OS file lock (`File::try_lock`), which is released automatically if the program crashes.
- A second `cantiva --headless` that can't get the lock exits immediately, so it can't take over the control API or load another model.
- When the main window opens while a headless instance is running, it asks that instance to exit and takes over once it holds the lock.
- The health check (`/v1/health`) doesn't wait for the synthesis worker lock, so Cantiva isn't reported as "not running" while a large model is loading.

### Worker lifetime

- The Rust side stops workers with `kill_on_drop`.
- Workers also watch Cantiva through `CANTIVA_PARENT_PID` (`OpenProcess` plus `WaitForSingleObject` on Windows). If Cantiva is killed, the worker exits on its own and frees VRAM.

## Technology choices

### Rust

| Component | Decision | Reason | Integration |
|---|---|---|---|
| Desktop UI | Rust + Slint | Native, small, fast to start; the same stack as the author's other desktop tool, which keeps maintenance cheap | Rust main program |
| Audio, scheduling, API, MCP | Rust | Long-running service where memory safety and a small footprint matter; rmcp is the official SDK | Rust main program |
| Model inference | **Python, not Rust** | All five models exist only as PyTorch implementations; Rust has no mature equivalent | Python sidecar process |

What would change this: if a model gets an ONNX or candle port of equal quality, its worker can move to Rust.

### Database

- **SQLite** (rusqlite 0.41, bundled). A single-user desktop app doesn't need a separate server. In WAL mode the GUI and headless mode can both have the database open for short overlaps.
- Audio and weights live on the file system; the database stores paths and metadata only.
- Schema changes are migrations keyed on `PRAGMA user_version`, currently at v2.
- What would change this: a voice library shared by several people, or a server deployment, would justify PostgreSQL.

### Docker

- **Not used.** Cantiva needs direct access to the GPU, audio devices and desktop windows.
- Docker Desktop on Windows runs a WSL 2 VM that can claim up to half the system memory, which an 8 GB-GPU machine can't spare.
- Dependency isolation comes from one `uv` venv per model with pinned PyTorch versions.
- What would change this: serving many users from a Linux server.

### Existing open-source options

| Capability | Candidates | Decision |
|---|---|---|
| MCP server | rmcp 3.8 (official Rust SDK) | Adopted |
| Local TTS server | Kokoro-FastAPI, vLLM-Omni, Xinference | Protocol design borrowed, not adopted: vLLM-Omni runs only on Linux; Xinference has neither BreezyVoice nor GPT-SoVITS v2ProPlus character models |
| Existing TTS MCP servers | giannisanni/kokoro-tts-mcp and similar | Not adopted: Kokoro only, no multi-model support or cloning |
| Playback | rodio 0.23 | Adopted (playback feature only) |
| Loudness | ebur128 | Adopted |
| Resampling | rubato | Not adopted: speech bandwidth is below 12 kHz, so linear interpolation is enough and avoids a dependency |

## Known limitations

- Install scripts exist for Windows only. The Rust code builds and tests on Linux, but there are no Linux model install scripts.
- The CosyVoice 3, IndexTTS, BreezyVoice and GPT-SoVITS install scripts fetch the upstream `main` branch and record the actual commit in `installed.json`. Upstream changes can break an adapter, so each adapter keeps its imports and parameters at the top of the file where they're easy to fix.
- Export is WAV only. Convert to MP3 or AAC with ffmpeg.
