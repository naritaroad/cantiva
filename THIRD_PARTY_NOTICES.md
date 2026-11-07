# Third-party notices

**English** | [繁體中文](docs/zh-TW/THIRD_PARTY_NOTICES.md)

Cantiva itself is released under the MIT License. Models, pretrained weights and some components carry their own licenses. The install scripts download them from their official sources; Cantiva does not redistribute any weights.

## Models (checked 2026-11-05)

| Model | Source | License | Commercial use |
|---|---|---|---|
| Qwen3-TTS (qwen-tts 0.1.1) | https://github.com/QwenLM/Qwen3-TTS | Apache-2.0 | Yes |
| GPT-SoVITS | https://github.com/RVC-Boss/GPT-SoVITS ; weights https://huggingface.co/lj1995/GPT-SoVITS | MIT. The authors state that users, and anyone who distributes generated audio, bear full responsibility for it | Yes |
| Fun-CosyVoice3-0.5B-2512 | https://github.com/FunAudioLLM/CosyVoice | Apache-2.0 | Yes |
| IndexTTS 2.5 | https://github.com/index-tts/index-tts | bilibili model license: services with more than 100 million monthly active users or more than CNY 1 billion annual revenue need a separate license; the model may not be used to improve other AI models | Conditional |
| BreezyVoice-300M | https://github.com/mtkresearch/BreezyVoice | Apache-2.0 | Yes |

Some GPT-SoVITS pretrained components (chinese-roberta-wwm-ext-large, chinese-hubert-base, the ERes2NetV2 speaker-verification model) have their own licenses. Check each model page before use.

## Voices

- Character models and reference clips are supplied by the user. Cantiva ships no voice of any person or character.
- Voices of anime and game characters belong to their voice actors and rights holders. Publishing an imitation without permission may infringe copyright, publicity or personality rights.

## Main Rust dependencies

| Crate | License |
|---|---|
| Slint 1.18 | GPL-3.0-only, Slint Royalty-free License 2.0, or Slint Software License 3.0. Cantiva uses Royalty-free 2.0: the About window shows AboutSlint, and public web pages must carry a "Made with Slint" attribution |
| rmcp 3.8 | Apache-2.0 |
| rodio 0.23 | MIT OR Apache-2.0 |
| cpal 0.18 | Apache-2.0 |
| rusqlite 0.41, libsqlite3-sys 0.39 (bundles SQLite, which is public domain) | MIT |
| axum 0.8, tokio 1.54 | MIT |
| reqwest 0.13, arboard 3.7 | MIT OR Apache-2.0 |
| rfd 0.17, ebur128 0.1 | MIT |
| hound 3.5 | Apache-2.0 |

For the full list, run `cargo tree` in the repository root or generate a report with cargo-about.
