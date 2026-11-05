# Engines (as shipped with Cantiva 0.1.0, verified 2026-11-05)

Only one engine is loaded at a time; switching engines unloads the previous one.

| id | Built-in voices | Cloning | Tone instruction | Emotion | Speed | Languages | Output | License |
|---|---|---|---|---|---|---|---|---|
| `qwen3-tts` | 9 (Vivian, Serena, Uncle_Fu, Dylan, Eric, Ryan, Aiden, Ono_Anna, Sohee) | yes (0.6B Base) | yes (1.7B CustomVoice, built-in voices only) | — | — | zh en ja ko de fr ru pt es it | 24 kHz | Apache-2.0 |
| `gpt-sovits` | — | yes (3–10 s clip); imports trained GPT `.ckpt` + SoVITS `.pth` | — | — | yes | ja zh en ko yue | 32 kHz | MIT |
| `cosyvoice3` | — | yes | yes | — | yes | zh en ja ko de es fr it ru yue + dialects | 24 kHz | Apache-2.0 |
| `indextts2` | — | yes | — | yes (text or 8-vector) | yes | zh en ja es ar | 22.05 kHz | bilibili model license (large companies need a separate license) |
| `breezyvoice` | — | yes | — | — | — | zh-TW | 22.05 kHz | Apache-2.0 |

## Notes per engine
- **qwen3-tts**: built-in voices load the 1.7B model (about 6–7 GB VRAM). Cloned voices load the 0.6B Base model. Switching between the two takes a few seconds.
- **gpt-sovits**: Japanese text is detected from kana. Set `language` explicitly for kanji-only Japanese. A library voice may carry a trained character model; it is swapped in automatically when that voice is used.
- **indextts2**: the `emotion_vector` order is happy, angry, sad, afraid, disgusted, melancholic, surprised, calm, each 0–1. `emotion_strength` defaults to 0.6. Text emotions are mapped by keyword (開心, 生氣, 難過, 害怕, 平靜 …).
- **breezyvoice**: override a polyphone reading by writing 注音 after the character, e.g. `銀行[:ㄏㄤ2]`. Use sparingly.
- **cosyvoice3**: an instruction switches it to instruct mode, e.g. 「請用廣東話表達」 or 「用很慢的語速」.

## Output
- Takes are 16-bit PCM WAV, loudness-normalized (default −16 LUFS).
- Exporting a whole project to one WAV happens in Cantiva's 輸出 page; there is no MCP tool for it yet.
