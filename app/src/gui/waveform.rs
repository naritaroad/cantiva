//! 波形圖：把音訊峰值畫成白色像素圖，Slint 端再用 colorize 染成片段顏色。

use cantiva_audio::{Clip, peaks};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

pub const WIDTH: u32 = 640;
pub const HEIGHT: u32 = 56;

pub fn render(clip: &Clip) -> Image {
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(WIDTH, HEIGHT);
    let pixels = buf.make_mut_slice();
    let p = peaks(clip, WIDTH as usize);
    // 語音通常很少接近滿格，用 0.9 分位數放大，讓小聲的片段也看得出起伏。
    let mut mags: Vec<f32> = p.iter().map(|(lo, hi)| lo.abs().max(hi.abs())).collect();
    mags.sort_by(|a, b| a.total_cmp(b));
    let reference = mags.get(mags.len() * 9 / 10).copied().unwrap_or(1.0).max(0.05);
    let scale = (0.9 / reference).min(4.0);
    let mid = HEIGHT as f32 / 2.0;
    for (x, (lo, hi)) in p.iter().enumerate() {
        let top = (mid - (hi * scale).min(1.0) * mid).floor().max(0.0) as u32;
        let bottom = (mid - (lo * scale).max(-1.0) * mid).ceil().min(HEIGHT as f32 - 1.0) as u32;
        let (top, bottom) = (top.min(bottom), bottom.max(top + 1));
        for y in top..bottom.min(HEIGHT) {
            let px = &mut pixels[(y * WIDTH) as usize + x];
            *px = Rgba8Pixel { r: 255, g: 255, b: 255, a: 230 };
        }
    }
    // 中線
    for x in 0..WIDTH {
        let px = &mut pixels[(mid as u32 * WIDTH + x) as usize];
        if px.a == 0 {
            *px = Rgba8Pixel { r: 255, g: 255, b: 255, a: 60 };
        }
    }
    Image::from_rgba8(buf)
}

/// 每 20 毫秒一格的 RMS，給音量表用（0–1，已換算成接近人耳的分貝刻度）。
pub fn envelope(clip: &Clip) -> Vec<f32> {
    let ch = clip.channels.max(1) as usize;
    let win = (clip.sample_rate as usize / 50).max(1);
    clip.samples
        .chunks(win * ch)
        .map(|chunk| {
            let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len().max(1) as f32).sqrt();
            // -60 dBFS → 0，0 dBFS → 1
            let db = 20.0 * rms.max(1e-6).log10();
            ((db + 60.0) / 60.0).clamp(0.0, 1.0)
        })
        .collect()
}
