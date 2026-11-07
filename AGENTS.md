# AGENTS.md

Guidance for AI coding agents (and humans) working on this repository. Read before changing code.

## What this is

Cantiva: a Windows desktop TTS studio (Rust + Slint) that runs open-source TTS models in per-engine Python worker processes, plus an MCP server (`cantiva-mcp`, rmcp) so AI agents can use it.

## Layout

| Path | Responsibility |
|---|---|
| `crates/cantiva-core` | Models, SQLite store (migrations via `PRAGMA user_version`), settings, script parsing, text splitting, path safety. Pure, unit-tested. |
| `crates/cantiva-audio` | WAV I/O, concat, trim, resample, loudness, peaks. Pure. |
| `crates/cantiva-engine` | Engine catalog, worker process supervisor, `Studio` (the shared service). |
| `crates/cantiva-api` | Local control API (axum) and its client. |
| `app` | Slint UI (`ui/*.slint`), controller (`src/gui/`), headless mode. |
| `mcp` | MCP tools (`src/tools.rs`) and transports (`src/main.rs`). |
| `engines/_worker` | Shared Python worker — **stdlib only**, so it never conflicts with model dependencies. |
| `engines/<id>/` | `engine.toml` (manifest), `adapter.py`, `install.ps1`. |

## Rules

1. **One GPU model at a time.** Only `Studio` starts workers, and only while holding the worker lock.
2. **Consent is mandatory for cloned voices.** Never add a code path that creates a reference voice without a `Consent` record, and never let tunnel-origin requests create voices.
3. **Never write outside the data folder** except outputs the user chose. Validate file names with `paths::sanitize_file_stem` and containment with `paths::is_within`.
4. **Local services bind 127.0.0.1 only** and require a token. The worker token travels in an env var, never in argv.
5. **stdout of `cantiva-mcp` is the JSON-RPC channel** — log to stderr only.
6. **No agent attribution in commits.** Commits are authored by the repo owner only: no `Co-authored-by` agent trailers, session links or "Generated with" footers.
7. UI strings are Taiwan Traditional Chinese (zh-TW); no Mainland terms.
8. Pin model dependencies in `install.ps1`; record the upstream commit in `installed.json`.

## Adding an engine

1. Create `engines/<id>/engine.toml` (see existing ones; set `order`, capabilities, `builtin_voices`).
2. Write `adapter.py` with an `Adapter(cantiva_worker.base.Adapter)` implementing `load`, `info`, `synthesize`. It must write a WAV to `segment.output_path`.
3. Write `install.ps1` using `engines/_common.ps1`; end with `Invoke-SelfTest` and `Complete-Install`.
4. Add the id to the catalog test and to `skill/cantiva-voice/reference/engines.md`.

## Tests

```
cargo test --workspace
python -m unittest discover engines/_worker/tests
python tests/e2e_mcp.py          # Linux/macOS/Windows; uses the dev-sine engine
python tests/screenshots.py out  # Linux with Xvfb; renders every page
```

The `dev-sine` engine (shown only with `CANTIVA_DEV=1`) fakes speech so the whole pipeline is testable without a GPU.

## Changelog

Every user-visible change gets an entry under `[Unreleased]` in `CHANGELOG.md` in the same commit.

## Docs

- `README.md` (English) and `README.zh-TW.md` carry the same sections; change both together.
- English docs live in `docs/`, their zh-TW translations in `docs/zh-TW/`. Keep each pair in sync.
- Human contributors read `CONTRIBUTING.md`; keep its rules consistent with this file.
