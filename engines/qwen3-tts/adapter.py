"""Qwen3-TTS adapter（qwen-tts 0.1.1）。

一個 worker 裡同時只放一個模型，依請求切換：
- 內建說話人 → 1.7B CustomVoice（0.6B 會忽略語氣指示，見 qwen_tts 原始碼）
- 參考音檔複製 → 0.6B Base
8GB 顯示卡放不下兩個 1.7B，所以切換時會先卸載另一個。
"""

from __future__ import annotations

import gc
import json
from pathlib import Path

from cantiva_worker.base import Adapter as BaseAdapter
from cantiva_worker.base import AdapterInfo, Segment, SynthesisError, pick_device, save_tensor_wav, set_seed

SPEAKERS = ["Vivian", "Serena", "Uncle_Fu", "Dylan", "Eric", "Ryan", "Aiden", "Ono_Anna", "Sohee"]
LANGUAGES = {
    "zh": "Chinese", "zh-tw": "Chinese", "zh-cn": "Chinese", "en": "English", "ja": "Japanese",
    "ko": "Korean", "de": "German", "fr": "French", "ru": "Russian", "pt": "Portuguese",
    "es": "Spanish", "it": "Italian",
}
DEFAULT_CONFIG = {
    "custom_voice_model": "Qwen3-TTS-12Hz-1.7B-CustomVoice",
    "base_model": "Qwen3-TTS-12Hz-0.6B-Base",
    # 每段最多生成的 token 數，避免模型偶爾停不下來。
    "max_new_tokens": 2048,
}


class Adapter(BaseAdapter):
    def load(self) -> None:
        import torch  # noqa: F401  確認 torch 可用
        from qwen_tts import Qwen3TTSModel

        self._cls = Qwen3TTSModel
        cfg_path = self.engine_home / "config.json"
        self.cfg = {**DEFAULT_CONFIG, **(json.loads(cfg_path.read_text("utf-8")) if cfg_path.is_file() else {})}
        self.device = pick_device()
        self.kind: str | None = None
        self.model = None
        self._prompts: dict[tuple[str, str], object] = {}
        self._ensure("custom")

    def _ensure(self, kind: str) -> None:
        if self.kind == kind and self.model is not None:
            return
        import torch

        if self.model is not None:
            del self.model
            self.model = None
            self._prompts.clear()
            gc.collect()
            if torch.cuda.is_available():
                torch.cuda.empty_cache()
        name = self.cfg["custom_voice_model"] if kind == "custom" else self.cfg["base_model"]
        path = self.engine_home / "models" / name
        if not path.is_dir():
            raise SynthesisError(f"找不到模型資料夾 {path}，請重新執行安裝")
        dtype = torch.bfloat16 if self.device.startswith("cuda") else torch.float32
        print(f"[qwen3] 載入 {name}（{self.device}）", flush=True)
        # Windows 沒有 flash-attn 的 wheel，用 PyTorch 內建的 sdpa。
        self.model = self._cls.from_pretrained(str(path), device_map=self.device, dtype=dtype, attn_implementation="sdpa")
        self.kind = kind

    def info(self) -> AdapterInfo:
        return AdapterInfo(sample_rate=24000, speakers=SPEAKERS)

    def synthesize(self, seg: Segment) -> None:
        set_seed(seg.seed)
        language = LANGUAGES.get((seg.language or "").lower())
        gen = {"max_new_tokens": int(self.cfg["max_new_tokens"])}
        if seg.reference_audio:
            if not seg.reference_text:
                raise SynthesisError("Qwen3-TTS 複製聲音需要參考音檔的逐字稿")
            self._ensure("base")
            key = (seg.reference_audio, seg.reference_text)
            prompt = self._prompts.get(key)
            if prompt is None:
                prompt = self.model.create_voice_clone_prompt(
                    ref_audio=seg.reference_audio, ref_text=seg.reference_text, x_vector_only_mode=False
                )
                self._prompts[key] = prompt
            wavs, sr = self.model.generate_voice_clone(
                text=seg.text, language=language, voice_clone_prompt=prompt, **gen
            )
        else:
            self._ensure("custom")
            speaker = seg.speaker or "Vivian"
            if speaker.lower() not in [s.lower() for s in SPEAKERS]:
                raise SynthesisError(f"Qwen3-TTS 沒有說話人「{speaker}」，可用：{', '.join(SPEAKERS)}")
            wavs, sr = self.model.generate_custom_voice(
                text=seg.text, speaker=speaker, language=language, instruct=seg.instruct or None, **gen
            )
        if not wavs:
            raise SynthesisError("模型沒有產生音訊")
        save_tensor_wav(seg.output_path, wavs[0], sr)
