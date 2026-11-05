//! 用開發假引擎（dev-sine）跑完整流程：啟動 Python worker → 分段合成 → 合併 → 正規化 → 存檔。
//! 需要系統上有 python3（CI 與開發機都有）。Windows 上跳過，因為假的 venv 是用 symlink 做的。

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use cantiva_core::model::{Consent, ConsentBasis, SpeakRequest, now};
use cantiva_core::paths::AppPaths;
use cantiva_engine::studio::{NewVoice, Origin, VoiceSource};
use cantiva_engine::{Studio, StudioConfig};

fn setup(name: &str) -> (std::sync::Arc<Studio>, PathBuf) {
    unsafe { std::env::set_var("CANTIVA_DEV", "1") };
    let root = std::env::temp_dir().join(format!("cantiva-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = AppPaths::at(&root);
    // 假裝 dev-sine 已安裝：.venv/bin/python 指到系統 python3。
    let home = paths.engine_home("dev-sine");
    std::fs::create_dir_all(home.join(".venv/bin")).unwrap();
    let python = which("python3").expect("python3");
    std::os::unix::fs::symlink(python, home.join(".venv/bin/python")).unwrap();
    std::fs::write(home.join("installed.json"), "{}").unwrap();
    let engines_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../engines");
    let studio = Studio::open(StudioConfig { paths, engines_dir, load_timeout: Duration::from_secs(30) }).unwrap();
    (studio, root)
}

fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?.to_str()?.split(':').map(|d| Path::new(d).join(bin)).find(|p| p.is_file())
}

#[tokio::test]
async fn speaks_long_text_with_builtin_voice() {
    let (studio, root) = setup("speak");
    let text = "第一句話在這裡。第二句話也在這裡！第三句話問一下呢？最後一句是比較長的內容，用來測試分段與合併的功能。";
    let req = SpeakRequest { text: text.into(), voice: Some("dev-sine/high".into()), ..Default::default() };
    let take = studio.speak(req, Origin::LocalAgent).await.unwrap();
    assert!(Path::new(&take.path).is_file(), "{}", take.path);
    assert!(take.segments >= 2, "segments = {}", take.segments);
    assert_eq!(take.engine_id, "dev-sine");
    assert_eq!(take.voice_name, "dev-sine/high");
    let lufs = take.loudness_lufs.unwrap();
    assert!((lufs + 16.0).abs() < 1.0, "lufs = {lufs}");
    assert!(take.duration_sec > 3.0);
    assert_eq!(studio.with_store(|s| s.recent_takes(5)).unwrap().len(), 1);
    assert_eq!(studio.loaded_engine().await.as_deref(), Some("dev-sine"));
    studio.unload().await;
    assert_eq!(studio.loaded_engine().await, None);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn worker_errors_surface_as_messages() {
    let (studio, root) = setup("fail");
    let req = SpeakRequest { text: "這段會 FAIL 掉".into(), voice: Some("dev-sine/low".into()), ..Default::default() };
    let err = studio.speak(req, Origin::Gui).await.unwrap_err().to_string();
    assert!(err.contains("測試用的合成失敗"), "{err}");
    let err = studio
        .speak(SpeakRequest { text: "嗨".into(), voice: Some("不存在".into()), engine: Some("dev-sine".into()), ..Default::default() }, Origin::Gui)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("找不到聲音"), "{err}");
    studio.unload().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn reference_voices_respect_origin_rules_and_scripts_render() {
    let (studio, root) = setup("clone");
    // 先用內建聲音做一段音檔，當成參考音檔。
    let seed = studio
        .speak(SpeakRequest { text: "這是參考音檔的內容，長度要超過三秒鐘才可以拿來複製聲音。".into(), voice: Some("dev-sine/low".into()), ..Default::default() }, Origin::Gui)
        .await
        .unwrap();
    let consent = Consent { basis: ConsentBasis::OwnVoice, statement: "我本人的聲音".into(), recorded_at: now() };
    let v = studio.create_reference_voice("我的聲音", "dev-sine", Path::new(&seed.path), "這是參考音檔的內容，長度要超過三秒鐘才可以拿來複製聲音。", consent).unwrap();
    assert_eq!(v.engine_id, "dev-sine");

    let clone_req = SpeakRequest { text: "用複製的聲音說話。".into(), voice: Some("我的聲音".into()), ..Default::default() };
    let err = studio.speak(clone_req.clone(), Origin::Tunnel).await.unwrap_err().to_string();
    assert!(err.contains("對外通道"), "{err}");
    assert!(studio.speak(clone_req, Origin::LocalAgent).await.is_ok());

    let script = "@我的聲音：{溫柔}第一行。\n\n@dev-sine/high：第二行。\n\n# 註解\n第三行用預設聲音。";
    let takes = studio.render_script("廣播劇", script, Some("dev-sine/low"), Origin::Gui).await.unwrap();
    assert_eq!(takes.len(), 3);
    assert_eq!(takes[0].voice_name, "我的聲音");
    assert_eq!(takes[2].voice_name, "dev-sine/low");
    assert!(Path::new(&takes[1].path).file_name().unwrap().to_str().unwrap().starts_with("002-"));
    studio.unload().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn trained_character_models_are_copied_and_passed_to_worker() {
    let (studio, root) = setup("trained");
    let seed = studio
        .speak(
            SpeakRequest { text: "角色模型測試用的參考音檔，長度要足夠才行。".into(), voice: Some("dev-sine/low".into()), ..Default::default() },
            Origin::Gui,
        )
        .await
        .unwrap();
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("char-e15.ckpt"), b"gpt").unwrap();
    std::fs::write(src.join("char_e8_s200.pth"), b"sovits").unwrap();
    let consent = Consent { basis: ConsentBasis::LicensedMaterial, statement: "測試素材".into(), recorded_at: now() };

    let missing_pair = NewVoice {
        name: "角色".into(),
        engine_id: "dev-sine".into(),
        audio: seed.path.clone().into(),
        transcript: "測試".into(),
        language: Some("ja".into()),
        gpt_weights: Some(src.join("char-e15.ckpt")),
        sovits_weights: None,
        consent: consent.clone(),
    };
    assert!(studio.create_voice(missing_pair.clone()).unwrap_err().to_string().contains("兩個檔案"));
    let wrong_ext = NewVoice { sovits_weights: Some(src.join("char-e15.ckpt")), ..missing_pair.clone() };
    assert!(studio.create_voice(wrong_ext).unwrap_err().to_string().contains(".pth"));

    let ok = NewVoice { sovits_weights: Some(src.join("char_e8_s200.pth")), ..missing_pair };
    studio.create_voice(ok).unwrap();
    std::fs::remove_dir_all(&src).unwrap(); // 原始檔刪掉也要能用，代表有複製一份
    let choice = studio.voice_choices().unwrap().into_iter().find(|v| v.name == "角色").unwrap();
    assert_eq!(choice.source, VoiceSource::Library);
    assert_eq!(choice.language, "ja");
    assert!(choice.description.contains("角色模型"));

    let take = studio
        .speak(SpeakRequest { text: "ボクはテイオー！".into(), voice: Some("角色".into()), ..Default::default() }, Origin::LocalAgent)
        .await
        .unwrap();
    assert!(Path::new(&take.path).is_file());
    studio.unload().await;
    let _ = std::fs::remove_dir_all(root);
}
