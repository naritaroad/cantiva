//! 播放：用 rodio 播放記憶體中的音訊。只在 UI 執行緒使用。
//!
//! 跳轉（seek）做法是從指定位置重新建立緩衝區，比依賴 rodio 的 try_seek 單純可靠。

use std::num::NonZero;
use std::sync::Arc;

use cantiva_audio::Clip;

pub struct AudioOut {
    sink: Option<rodio::MixerDeviceSink>,
    player: Option<rodio::Player>,
    clip: Option<Arc<Clip>>,
    /// 目前這段緩衝區是從 clip 的第幾秒開始播。
    offset: f64,
    paused: bool,
}

impl AudioOut {
    pub fn new() -> Self {
        Self { sink: None, player: None, clip: None, offset: 0.0, paused: false }
    }

    fn ensure_sink(&mut self) -> Result<&rodio::MixerDeviceSink, String> {
        if self.sink.is_none() {
            let mut sink = rodio::DeviceSinkBuilder::open_default_sink()
                .map_err(|e| format!("找不到可用的音訊輸出裝置：{e}"))?;
            sink.log_on_drop(false);
            self.sink = Some(sink);
        }
        Ok(self.sink.as_ref().expect("sink"))
    }

    /// 從 `from_sec` 開始播放 clip。
    pub fn play(&mut self, clip: Arc<Clip>, from_sec: f64) -> Result<(), String> {
        self.stop();
        let ch = clip.channels.max(1) as usize;
        let start = ((from_sec.max(0.0) * clip.sample_rate as f64) as usize).min(clip.frames()) * ch;
        let data: Vec<f32> = clip.samples[start..].to_vec();
        if data.is_empty() {
            return Ok(());
        }
        let channels = NonZero::new(clip.channels.max(1)).expect("nonzero");
        let rate = NonZero::new(clip.sample_rate.max(1)).expect("nonzero");
        let sink = self.ensure_sink()?;
        let player = rodio::Player::connect_new(sink.mixer());
        player.append(rodio::buffer::SamplesBuffer::new(channels, rate, data));
        player.play();
        self.player = Some(player);
        self.offset = from_sec.max(0.0);
        self.clip = Some(clip);
        self.paused = false;
        Ok(())
    }

    pub fn pause(&mut self) {
        if let Some(p) = &self.player {
            p.pause();
            self.paused = true;
        }
    }

    pub fn resume(&mut self) {
        if let Some(p) = &self.player {
            p.play();
            self.paused = false;
        }
    }

    pub fn stop(&mut self) {
        if let Some(p) = self.player.take() {
            p.stop();
        }
        self.paused = false;
    }

    pub fn is_playing(&self) -> bool {
        self.player.as_ref().is_some_and(|p| !p.empty() && !self.paused)
    }

    pub fn is_paused(&self) -> bool {
        self.paused && self.player.as_ref().is_some_and(|p| !p.empty())
    }

    /// 播完了（有播放過，而且緩衝區空了）。
    pub fn finished(&self) -> bool {
        self.player.as_ref().is_some_and(|p| p.empty())
    }

    /// 目前播到 clip 的第幾秒。
    pub fn position(&self) -> f64 {
        self.offset + self.player.as_ref().map_or(0.0, |p| p.get_pos().as_secs_f64())
    }
}
