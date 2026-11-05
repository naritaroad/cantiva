//! 音訊處理：讀寫 WAV、合併分段、響度量測與正規化、波形峰值。
//!
//! 內部一律用 f32 單聲道或多聲道交錯樣本處理，輸出 16-bit PCM WAV（相容性最好）。

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("讀寫 WAV 失敗：{0}")]
    Wav(#[from] hound::Error),
    #[error("響度量測失敗：{0}")]
    Loudness(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, AudioError>;

/// 記憶體中的一段音訊。`samples` 是交錯排列的 f32（-1.0 到 1.0）。
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl Clip {
    pub fn silence(sample_rate: u32, channels: u16, seconds: f64) -> Self {
        let frames = (seconds.max(0.0) * sample_rate as f64).round() as usize;
        Self { sample_rate, channels, samples: vec![0.0; frames * channels as usize] }
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    pub fn duration_sec(&self) -> f64 {
        self.frames() as f64 / self.sample_rate.max(1) as f64
    }

    pub fn peak(&self) -> f32 {
        self.samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()))
    }
}

pub fn read_wav(path: &Path) -> Result<Clip> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<std::result::Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1_i64 << (spec.bits_per_sample.saturating_sub(1))) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<std::result::Result<_, _>>()?
        }
    };
    Ok(Clip { sample_rate: spec.sample_rate, channels: spec.channels, samples })
}

/// 寫成 16-bit PCM WAV。先寫暫存檔再改名，避免播放器讀到寫一半的檔案。
pub fn write_wav(path: &Path, clip: &Clip) -> Result<()> {
    let spec = hound::WavSpec {
        channels: clip.channels,
        sample_rate: clip.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let tmp = path.with_extension("wav.part");
    {
        let mut writer = hound::WavWriter::create(&tmp, spec)?;
        for s in &clip.samples {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
            writer.write_sample(v)?;
        }
        writer.finalize()?;
    }
    std::fs::rename(&tmp, path).map_err(hound::Error::IoError)?;
    Ok(())
}

/// 依序合併多段，段與段之間插入 `gap_sec` 秒靜音。所有段必須同取樣率、同聲道數。
pub fn concat(clips: &[Clip], gap_sec: f64) -> Result<Clip> {
    let first = clips.first().ok_or_else(|| AudioError::Invalid("沒有可以合併的音訊".into()))?;
    let (rate, ch) = (first.sample_rate, first.channels);
    if let Some(bad) = clips.iter().find(|c| c.sample_rate != rate || c.channels != ch) {
        return Err(AudioError::Invalid(format!(
            "分段格式不一致：{rate} Hz／{ch} 聲道 與 {} Hz／{} 聲道",
            bad.sample_rate, bad.channels
        )));
    }
    let gap = Clip::silence(rate, ch, gap_sec);
    let total: usize = clips.iter().map(|c| c.samples.len()).sum::<usize>() + gap.samples.len() * (clips.len() - 1);
    let mut samples = Vec::with_capacity(total);
    for (i, c) in clips.iter().enumerate() {
        if i > 0 {
            samples.extend_from_slice(&gap.samples);
        }
        samples.extend_from_slice(&c.samples);
    }
    Ok(Clip { sample_rate: rate, channels: ch, samples })
}

/// 重新取樣到 `target_rate`（線性內插）。語音合成的輸出頻寬本來就在 12 kHz 以下，
/// 在 22.05/24/48 kHz 之間轉換時線性內插的失真聽不出來；不需要另外引入重新取樣函式庫。
pub fn resample(clip: &Clip, target_rate: u32) -> Clip {
    if clip.sample_rate == target_rate || clip.frames() == 0 {
        return Clip { sample_rate: target_rate, ..clip.clone() };
    }
    let ch = clip.channels.max(1) as usize;
    let src_frames = clip.frames();
    let ratio = clip.sample_rate as f64 / target_rate as f64;
    let dst_frames = ((src_frames as f64) / ratio).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(dst_frames * ch);
    for i in 0..dst_frames {
        let pos = i as f64 * ratio;
        let i0 = (pos.floor() as usize).min(src_frames - 1);
        let i1 = (i0 + 1).min(src_frames - 1);
        let frac = (pos - i0 as f64) as f32;
        for c in 0..ch {
            let a = clip.samples[i0 * ch + c];
            let b = clip.samples[i1 * ch + c];
            out.push(a + (b - a) * frac);
        }
    }
    Clip { sample_rate: target_rate, channels: clip.channels, samples: out }
}

/// 把頭尾低於 `threshold`（線性振幅）的靜音修掉，各保留 `pad_sec` 秒。
pub fn trim_silence(clip: &Clip, threshold: f32, pad_sec: f64) -> Clip {
    let ch = clip.channels.max(1) as usize;
    let frames = clip.frames();
    let loud = |f: usize| clip.samples[f * ch..f * ch + ch].iter().any(|s| s.abs() > threshold);
    let Some(start) = (0..frames).find(|&f| loud(f)) else {
        return clip.clone();
    };
    let end = (0..frames).rev().find(|&f| loud(f)).unwrap_or(start) + 1;
    let pad = (pad_sec * clip.sample_rate as f64) as usize;
    let s = start.saturating_sub(pad);
    let e = (end + pad).min(frames);
    Clip { sample_rate: clip.sample_rate, channels: clip.channels, samples: clip.samples[s * ch..e * ch].to_vec() }
}

/// EBU R128 整合響度（LUFS）。太短或全靜音時回傳 None。
pub fn integrated_loudness(clip: &Clip) -> Result<Option<f64>> {
    let mut meter = ebur128::EbuR128::new(clip.channels as u32, clip.sample_rate, ebur128::Mode::I)
        .map_err(|e| AudioError::Loudness(e.to_string()))?;
    meter.add_frames_f32(&clip.samples).map_err(|e| AudioError::Loudness(e.to_string()))?;
    match meter.loudness_global() {
        Ok(l) if l.is_finite() => Ok(Some(l)),
        Ok(_) => Ok(None),
        Err(e) => Err(AudioError::Loudness(e.to_string())),
    }
}

/// 把整合響度調到 `target_lufs`，同時確保峰值不超過 -1 dBFS（避免爆音）。
/// 回傳調整後的響度。量不到響度（例如全靜音）時原樣返回。
pub fn normalize_loudness(clip: &mut Clip, target_lufs: f64) -> Result<Option<f64>> {
    let Some(current) = integrated_loudness(clip)? else {
        return Ok(None);
    };
    let mut gain = 10f64.powf((target_lufs - current) / 20.0) as f32;
    let ceiling = 10f32.powf(-1.0 / 20.0);
    let peak = clip.peak();
    if peak * gain > ceiling && peak > 0.0 {
        gain = ceiling / peak;
    }
    for s in &mut clip.samples {
        *s *= gain;
    }
    integrated_loudness(clip)
}

/// 給波形顯示用：把音訊切成 `buckets` 格，每格回傳 (最小值, 最大值)。
pub fn peaks(clip: &Clip, buckets: usize) -> Vec<(f32, f32)> {
    let buckets = buckets.max(1);
    let ch = clip.channels.max(1) as usize;
    let frames = clip.frames();
    if frames == 0 {
        return vec![(0.0, 0.0); buckets];
    }
    (0..buckets)
        .map(|b| {
            let start = b * frames / buckets;
            let end = ((b + 1) * frames / buckets).max(start + 1).min(frames);
            let mut lo = 0.0_f32;
            let mut hi = 0.0_f32;
            for f in start..end {
                // 多聲道取平均，畫成單一波形。
                let v = clip.samples[f * ch..f * ch + ch].iter().sum::<f32>() / ch as f32;
                lo = lo.min(v);
                hi = hi.max(v);
            }
            (lo, hi)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, seconds: f64, amp: f32) -> Clip {
        let n = (rate as f64 * seconds) as usize;
        let samples =
            (0..n).map(|i| amp * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin()).collect();
        Clip { sample_rate: rate, channels: 1, samples }
    }

    #[test]
    fn wav_roundtrip_keeps_format_and_length() {
        let dir = std::env::temp_dir().join(format!("cantiva-audio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.wav");
        let clip = tone(24_000, 0.5, 0.5);
        write_wav(&path, &clip).unwrap();
        let back = read_wav(&path).unwrap();
        assert_eq!(back.sample_rate, 24_000);
        assert_eq!(back.frames(), clip.frames());
        assert!((back.peak() - 0.5).abs() < 0.01);
        assert!(!path.with_extension("wav.part").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn concat_inserts_gaps_and_rejects_mixed_formats() {
        let a = tone(24_000, 1.0, 0.3);
        let b = tone(24_000, 0.5, 0.3);
        let joined = concat(&[a.clone(), b], 0.25).unwrap();
        assert!((joined.duration_sec() - 1.75).abs() < 1e-3);
        let c = tone(22_050, 0.5, 0.3);
        assert!(concat(&[a, c], 0.0).is_err());
        assert!(concat(&[], 0.0).is_err());
    }

    #[test]
    fn resample_keeps_duration_and_level() {
        let clip = tone(22_050, 1.0, 0.5);
        let up = resample(&clip, 24_000);
        assert_eq!(up.sample_rate, 24_000);
        assert!((up.duration_sec() - 1.0).abs() < 1e-3);
        assert!((up.peak() - 0.5).abs() < 0.02);
        assert_eq!(resample(&clip, 22_050), clip);
    }

    #[test]
    fn trims_leading_and_trailing_silence() {
        let clip = concat(&[Clip::silence(24_000, 1, 1.0), tone(24_000, 0.5, 0.5), Clip::silence(24_000, 1, 1.0)], 0.0)
            .unwrap();
        let trimmed = trim_silence(&clip, 0.01, 0.05);
        assert!((trimmed.duration_sec() - 0.6).abs() < 0.01, "{}", trimmed.duration_sec());
    }

    #[test]
    fn normalizes_to_target_with_peak_ceiling() {
        let mut quiet = tone(24_000, 3.0, 0.02);
        let before = integrated_loudness(&quiet).unwrap().unwrap();
        let after = normalize_loudness(&mut quiet, -16.0).unwrap().unwrap();
        assert!(before < -30.0);
        assert!((after - -16.0).abs() < 0.5, "{after}");
        assert!(quiet.peak() <= 10f32.powf(-1.0 / 20.0) + 1e-4);

        let mut silent = Clip::silence(24_000, 1, 2.0);
        assert_eq!(normalize_loudness(&mut silent, -16.0).unwrap(), None);
    }

    #[test]
    fn peaks_cover_requested_buckets() {
        let clip = tone(24_000, 1.0, 0.8);
        let p = peaks(&clip, 200);
        assert_eq!(p.len(), 200);
        assert!(p.iter().all(|(lo, hi)| *lo >= -0.81 && *hi <= 0.81));
        assert!(p.iter().any(|(_, hi)| *hi > 0.7));
        assert_eq!(peaks(&Clip::silence(24_000, 1, 0.0), 4), vec![(0.0, 0.0); 4]);
    }
}
