# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog 2.0.0](https://keepachangelog.com/en/2.0.0/), and the project uses [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-11-07

### Added

- GitHub Releases: pushing a `v*` tag builds and publishes a Windows installer (`Cantiva-<version>-windows-x64-setup.exe`, Inno Setup, per-user, no administrator rights), a portable zip, the `cantiva-voice` skill as its own zip, and `SHA256SUMS.txt`.
- The installer adds a Start-menu shortcut, can copy the skill to `~/.claude/skills` and `~/.agents/skills`, and can install uv and Git with winget. Its wizard is in Traditional Chinese or English. Uninstalling keeps `%USERPROFILE%\Cantiva`.
- `scripts/package.ps1` builds the release files locally; `scripts/install-tools.ps1` installs the tools engines need (`-GptSovits` adds CMake and the C++ Build Tools, `-Tunnel` adds cloudflared); `scripts/release-notes.ps1` turns a changelog section into release notes.
- `CONTRIBUTING.md`, `SECURITY.md`, issue and pull request templates, and a Windows CI workflow that runs the Rust tests, the Python worker tests and the MCP end-to-end test.
- README banner (`docs/assets/banner.svg`).
- Cargo manifests now carry `description`, `repository` and `homepage`.

### Changed

- `README.md` is now in English, with the Traditional Chinese version in `README.zh-TW.md`. `docs/architecture.md` and `THIRD_PARTY_NOTICES.md` are in English, with zh-TW translations under `docs/zh-TW/`. `docs/README.en.md` is removed.
- `CHANGELOG.md` is now written in English.
- Release builds link the C runtime statically, so the Visual C++ Redistributable is not needed.
- `register-mcp.ps1` also works from an installed copy (`<install folder>\scripts`), not only from the repository.
- When uv, Git or CMake is missing, engine installs now point to `scripts\install-tools.ps1` instead of `setup.ps1`.
- On Windows the default data folder moved from `%APPDATA%\Naritaroad\Cantiva\data` to `%USERPROFILE%\Cantiva`. Processes started from MSIX-packaged apps such as Claude Desktop have their AppData writes redirected to that app's private folder, so takes made by an agent and installed engines were invisible to Cantiva. Move the 0.1.0 folder to the new location, or set `CANTIVA_HOME` to the old one.

### Fixed

- The About window shows the running version instead of a fixed "0.1.0".
- Re-running `setup.ps1` no longer fails when `cantiva-mcp.exe` or `cantiva.exe` is held open by an agent (for example Cursor): the old file is renamed out of the way and the new build is put in place.
- When `setup.ps1` runs inside another app (for example Claude Desktop), it detects that the Start-menu shortcut was redirected to a private folder and asks you to re-run it from a normal PowerShell window.
- When headless mode receives a shutdown request, the log now says so instead of always reporting that the main window took over.
- Windows: the `engines/_worker` self-test unit test reads child-process output as UTF-8 and no longer fails to decode under the system code page (cp950).
- Windows: the `studio_dev_engine` integration test is no longer skipped. It builds the fake engine's venv with `python -m venv`, so worker start-up and synthesis are covered on Windows too.
- Windows: `tests/e2e_mcp.py` runs on Windows. It shuts down headless mode through the control API's `/v1/shutdown` instead of relying on `pkill`.

## [0.1.0] - 2026-11-05

### Added

- Desktop app (Slint 1.18) with six pages (Voices, Script, Timeline, Export, Engines, Connect), a DaVinci Resolve-style page bar and a dark UI.
- Five TTS engines: Qwen3-TTS, GPT-SoVITS v2ProPlus, Fun-CosyVoice 3, IndexTTS 2.5 and BreezyVoice. Each has its own uv venv and a Windows install script that runs a self-test after installing.
- GPT-SoVITS character models: import trained GPT (`.ckpt`) and SoVITS (`.pth`) weights as a voice in the library. The weights are swapped in automatically at synthesis time.
- Script format: `@Voice：{tone}line`, `#` comments, blank lines between clips.
- Timeline: multiple tracks, waveforms, playhead and level meter. Clips can be reordered, regenerated, starred and removed.
- Export: joins the timeline into one WAV in order, unifies sample rates and normalizes loudness.
- Consent records: creating a cloned voice requires choosing a consent basis and writing a statement.
- `cantiva-mcp`: 10 MCP tools over stdio and Streamable HTTP. Starts Cantiva headless when it isn't running.
- Tunnel (off by default): a Cloudflare quick tunnel whose URL contains a secret, with a reduced tool set.
- Companion skill `cantiva-voice`, and `register-mcp.ps1`, which registers the server with Claude Code, Codex, Cursor and Grok Build CLI and installs the skill.
- `setup.ps1`: installs tools with winget, builds, assembles the dist folder and creates a shortcut.

### Security

- The tunnel gets a new one-time secret every time it opens, and cloudflared starts only after the MCP server is confirmed to be listening.
- Tunnel restrictions are enforced by the control API: no creating voices, playing audio, or loading or unloading models; responses contain no local paths or transcripts.
- A single-instance lock prevents a second Cantiva from loading another model at the same time.
- Engine workers watch the Cantiva process and exit if it crashes, so they don't keep holding VRAM.

[Unreleased]: https://github.com/naritaroad/cantiva/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/naritaroad/cantiva/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/naritaroad/cantiva/releases/tag/v0.1.0
