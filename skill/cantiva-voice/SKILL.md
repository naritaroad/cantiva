---
name: cantiva-voice
description: Use when the user wants speech audio made on their own computer with Cantiva — text-to-speech, narration, voice-overs, radio dramas, podcasts, audiobook chapters, multi-character dialogue, character voices, or cloning a voice they have rights to — via the cantiva MCP tools (speak, render_script, list_voices).
---

# Cantiva voice production

Cantiva is a local TTS studio. Its MCP server (`cantiva`) runs models on the user's GPU and saves WAV files on their disk. Tools return file paths, not audio.

## Workflow
1. `list_voices` (optionally `engine`) before picking a voice. Never invent voice names.
   - Built-in voices are `engine/speaker`, e.g. `qwen3-tts/Serena`.
   - Library voices (cloned, or GPT-SoVITS character models) use their plain name.
2. One short passage → `speak`. Several lines or characters → `render_script` (one WAV per line, in timeline order).
3. Report each result's `path` and `duration_sec`. Offer `play_take` only when `status` shows `can_play_on_this_computer: true`.
4. When the first call is slow, the model is loading (up to a few minutes). Don't retry; the call is still running.

## Choosing an engine
- Need instruction-controlled tone (「用溫柔、放慢的語氣」) with a built-in voice → `qwen3-tts`.
- Japanese, anime-style, or a user-supplied character model → `gpt-sovits`.
- Precise emotion (`emotion` text or 8-value `emotion_vector`) → `indextts2`.
- Taiwanese Mandarin pronunciation or 注音 fixes such as `好[:ㄏㄠ3]` → `breezyvoice`.
- Cross-lingual cloning or Chinese dialects → `cosyvoice3`.
- Parameters the engine doesn't support are ignored silently. Check `list_engines` capabilities instead of assuming.
- For engine details and limits, read `reference/engines.md`.

## Script format (render_script)
- One paragraph per line; separate lines with a blank line.
- `@Voice name：` at the start of a line picks the voice.
- `{tone}` right after it is an instruction.
- Lines starting with `#` are comments.
- For full rules and examples, read `reference/script-format.md`.

## Voice cloning and consent
- `create_voice` needs a basis: `own_voice`, `written_permission` or `licensed_material`.
- Ask the user which basis applies and record their own words in `consent_statement`. Never choose the basis for them or infer it from context.
- Don't clone a real person's voice (including voice actors behind anime or game characters) unless the user confirms they have permission or it is licensed material.
- Don't download or extract voice data or trained models from games, streams or other people. Use only files the user provides.
- Over a tunnel (cloud clients), `create_voice`, `play_take`, `load_engine` and `unload_engine` are unavailable, and cloned voices may be blocked. Explain this instead of retrying.

## Limits and errors
- Requests have a per-call character limit (default 6000). Split long text into chapters and call per chapter.
- GPT-SoVITS reference clips must be 3–10 s.
- Tool errors are written for the user. Relay them, then fix the cause — wrong voice name, engine not installed (installed from Cantiva's 引擎 page), clip too long — rather than retrying the same call.
- `unload_engine` frees VRAM when the user wants their GPU back.
