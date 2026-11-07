# 第三方授權說明

[English](../../THIRD_PARTY_NOTICES.md) | **繁體中文**

Cantiva 本身以 MIT 授權發佈。模型、預訓練權重與部分元件有各自的授權，安裝腳本會從官方來源下載；Cantiva 不重新散布這些權重。

## 模型（2026-11-05 查證）

| 模型 | 來源 | 授權 | 商用 |
|---|---|---|---|
| Qwen3-TTS（qwen-tts 0.1.1） | https://github.com/QwenLM/Qwen3-TTS | Apache-2.0 | 可 |
| GPT-SoVITS | https://github.com/RVC-Boss/GPT-SoVITS ；權重 https://huggingface.co/lj1995/GPT-SoVITS | MIT。原作者聲明：使用者與散布產生聲音者自負全責 | 可 |
| Fun-CosyVoice3-0.5B-2512 | https://github.com/FunAudioLLM/CosyVoice | Apache-2.0 | 可 |
| IndexTTS 2.5 | https://github.com/index-tts/index-tts | bilibili 模型使用協議：月活躍使用者超過 1 億或年營收超過人民幣 10 億須另取授權；不得用於改進其他 AI 模型 | 有條件 |
| BreezyVoice-300M | https://github.com/mtkresearch/BreezyVoice | Apache-2.0 | 可 |

GPT-SoVITS 的部分預訓練元件（chinese-roberta-wwm-ext-large、chinese-hubert-base、ERes2NetV2 說話人驗證模型）有各自的授權，使用前請查看各自的模型頁。

## 聲音

- 角色模型與參考音檔由使用者自行提供，Cantiva 不提供任何人或角色的聲音。
- 動畫、遊戲角色的聲音屬於配音員與版權方。未取得授權就公開發布仿製的聲音，可能侵害著作權、肖像或人格權。

## 主要 Rust 套件

| 套件 | 授權 |
|---|---|
| Slint 1.18 | GPL-3.0-only、Slint Royalty-free License 2.0，或 Slint Software License 3.0。Cantiva 採用 Royalty-free 2.0：「關於」視窗顯示 AboutSlint，公開網頁需附「Made with Slint」標示 |
| rmcp 3.8 | Apache-2.0 |
| rodio 0.23 | MIT OR Apache-2.0 |
| cpal 0.18 | Apache-2.0 |
| rusqlite 0.41、libsqlite3-sys 0.39（內含 SQLite，SQLite 為公眾領域） | MIT |
| axum 0.8、tokio 1.54 | MIT |
| reqwest 0.13、arboard 3.7 | MIT OR Apache-2.0 |
| rfd 0.17、ebur128 0.1 | MIT |
| hound 3.5 | Apache-2.0 |

## 安裝程式

Windows 安裝程式用 [Inno Setup](https://jrsoftware.org/isinfo.php) 建置（Inno Setup License）。`packaging/ChineseTraditional.isl` 是 [Inno Setup repo](https://github.com/jrsoftware/issrc/blob/main/Files/Languages/ChineseTraditional.isl) 裡的繁體中文翻譯，依相同授權散布。

完整清單請在專案根目錄執行 `cargo tree`，或用 cargo-about 產生。
