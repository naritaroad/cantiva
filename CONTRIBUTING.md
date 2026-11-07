# Contributing to Cantiva

Thanks for helping. This file covers how to build, test and send changes. [AGENTS.md](AGENTS.md) has the same rules in the form AI coding agents read; follow it too if you use one.

## Before you start

- For anything larger than a small fix, open an issue first so the approach can be agreed on before you write code.
- Security problems go through [SECURITY.md](SECURITY.md), not public issues.

## Build

You need Windows 10/11, Rust 1.92 or newer, Python 3 and the Visual Studio Build Tools. `scripts\setup.ps1` installs all of them with winget and builds a release into `dist\Cantiva`. For day-to-day work a debug build is enough:

```powershell
cargo build
```

You don't need a GPU or any real engine to develop. Set `CANTIVA_DEV=1` and the `dev-sine` engine appears: it fakes speech with synthetic syllables, so the whole pipeline (worker start-up, synthesis, timeline, export, MCP) runs on any machine. Point `CANTIVA_HOME` at a scratch folder to keep test data away from your real library.

## Tests

```powershell
cargo test --workspace                              # Rust unit and integration tests, including a dev-sine pipeline run
python -m unittest discover engines/_worker/tests   # Python worker
cargo build; python tests/e2e_mcp.py                # MCP end to end: an agent talking to cantiva-mcp over stdio
```

`tests/screenshots.py` renders every page for the README screenshots; it runs on Linux under Xvfb.

CI runs the first three on every push and pull request.

## Project rules

These keep Cantiva safe to run on someone's own machine. Pull requests that break them won't be merged.

1. **One GPU model at a time.** Only `Studio` starts workers, and only while holding the worker lock.
2. **Consent is mandatory for cloned voices.** No code path may create a reference voice without a `Consent` record, and tunnel-origin requests may never create voices.
3. **Never write outside the data folder**, except outputs the user chose. Validate file names with `paths::sanitize_file_stem` and containment with `paths::is_within`.
4. **Local services bind 127.0.0.1 only** and require a token. The worker token travels in an environment variable, never in argv.
5. **stdout of `cantiva-mcp` is the JSON-RPC channel.** Log to stderr only.
6. **The shared worker (`engines/_worker`) is stdlib-only**, so it never conflicts with a model's dependencies.
7. **Pin model dependencies** in `install.ps1` and record the upstream commit in `installed.json`.

## Language

- UI strings are Traditional Chinese as used in Taiwan (zh-TW). Avoid Mainland Chinese terms (for example 視頻, 軟件, 網絡; use 影片, 軟體, 網路).
- Identifiers and the English docs are in English. Most code comments are in Traditional Chinese; either language is fine, but match the file you're editing.
- `README.md` and `README.zh-TW.md` carry the same sections. If you change one, change the other, or say in the pull request that the translation still needs doing.

## Adding an engine

1. Create `engines/<id>/engine.toml`. Copy an existing one and set `order`, the capabilities and `builtin_voices`.
2. Write `adapter.py` with an `Adapter(cantiva_worker.base.Adapter)` that implements `load`, `info` and `synthesize`. It must write a WAV to `segment.output_path`.
3. Write `install.ps1` on top of `engines/_common.ps1`, ending with `Invoke-SelfTest` and `Complete-Install`.
4. Add the id to the catalog test and to `skill/cantiva-voice/reference/engines.md`, and the model's license to `THIRD_PARTY_NOTICES.md`.

## Releasing

1. Move the `[Unreleased]` entries in `CHANGELOG.md` under a new version heading, and set the same version in `[workspace.package]` in `Cargo.toml` and in the internal `version = "..."` requirements of the crates.
2. Commit, wait for CI to pass, then tag and push: `git tag v<version>` and `git push origin v<version>`.
3. `.github/workflows/release.yml` builds the installer, the portable zip and the skill zip with `scripts\package.ps1` and publishes them as a GitHub Release, with that version's changelog section as the notes. The tag must match the `Cargo.toml` version or the workflow stops.

To build the same files locally, install Inno Setup 6 (`winget install --id JRSoftware.InnoSetup`) and run `powershell -ExecutionPolicy Bypass -File scripts\package.ps1`. They land in `target\release-assets`.

## Pull requests

- Keep each pull request to one change, and include tests for behavior you add or fix.
- Add an entry under `[Unreleased]` in [CHANGELOG.md](CHANGELOG.md) for every user-visible change ([Keep a Changelog](https://keepachangelog.com/en/2.0.0/) format).
- Make sure the tests above pass.
- By submitting a pull request you agree that your contribution is licensed under the [MIT License](LICENSE).
