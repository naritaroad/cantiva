"""IndexTTS-2.5 adapter（indextts.infer_v2_5）。

- 必須提供參考音檔（零樣本複製）。
- 情緒：優先用八維向量；只有文字描述時，用關鍵字對應成向量，
  不載入額外的 Qwen 情緒模型（8GB 顯示記憶體放不下）。
- 語速：轉成 duration_factor（>1 變慢）。
"""

from __future__ import annotations

import os
import re
import sys

from cantiva_worker.base import Adapter as BaseAdapter
from cantiva_worker.base import AdapterInfo, Segment, SynthesisError, pick_device, set_seed

# 向量順序：喜、怒、哀、懼、厭惡、低落、驚訝、平靜（與 IndexTTS 相同）
EMOTION_KEYWORDS: list[tuple[int, tuple[str, ...]]] = [
    (0, ("開心", "高興", "快樂", "興奮", "愉快", "happy", "joy", "cheer")),
    (1, ("生氣", "憤怒", "惱火", "火大", "angry", "rage", "mad")),
    (2, ("難過", "悲傷", "傷心", "哭", "sad", "grief", "cry")),
    (3, ("害怕", "恐懼", "緊張", "驚恐", "afraid", "fear", "scared", "nervous")),
    (4, ("厭惡", "噁心", "嫌棄", "disgust")),
    (5, ("低落", "沮喪", "憂鬱", "疲憊", "無力", "melancholy", "depressed", "tired")),
    (6, ("驚訝", "意外", "吃驚", "surprise", "shock")),
    (7, ("平靜", "冷靜", "沉穩", "溫柔", "calm", "gentle", "soft")),
]
LANG = {"zh": "ZH", "zh-tw": "ZH", "zh-cn": "ZH", "en": "EN", "ja": "JA", "es": "ES", "ar": "AR"}
CJK = re.compile(r"[㐀-鿿]")


def emotion_from_text(text: str) -> list[float] | None:
    t = text.lower()
    vec = [0.0] * 8
    for idx, words in EMOTION_KEYWORDS:
        if any(w in t for w in words):
            vec[idx] = 1.0
    return vec if any(vec) else None


class Adapter(BaseAdapter):
    def load(self) -> None:
        import torch

        repo = self.engine_home / "repo"
        model_dir = self.engine_home / "models" / "IndexTTS-2.5"
        if not (repo / "indextts").is_dir() or not (model_dir / "config.yaml").is_file():
            raise SynthesisError("IndexTTS 原始碼或模型不完整，請重新執行安裝")
        os.chdir(repo)
        sys.path.insert(0, str(repo))
        from indextts.infer_v2_5 import IndexTTS2

        cuda = torch.cuda.is_available()
        self.tts = IndexTTS2(
            cfg_path=str(model_dir / "config.yaml"),
            model_dir=str(model_dir),
            use_bf16=cuda,
            use_cuda_kernel=False,
            use_deepspeed=False,
            use_qwen_emo=False,
        )
        print(f"[indextts] 已載入，裝置 {pick_device()}", flush=True)

    def info(self) -> AdapterInfo:
        return AdapterInfo(sample_rate=22050)

    def synthesize(self, seg: Segment) -> None:
        if not seg.reference_audio:
            raise SynthesisError("IndexTTS 沒有內建聲音，請先在聲音庫用參考音檔建立聲音")
        set_seed(seg.seed)
        lang = LANG.get((seg.language or "").lower()) or ("ZH" if CJK.search(seg.text) else "EN")
        kwargs: dict = {}
        emo = seg.emotion or {}
        vector = emo.get("vector")
        if vector is None and emo.get("text"):
            vector = emotion_from_text(emo["text"])
        if vector is None and seg.instruct:
            vector = emotion_from_text(seg.instruct)
        if vector is not None:
            if len(vector) != 8:
                raise SynthesisError("情緒向量要有 8 個數字：喜、怒、哀、懼、厭惡、低落、驚訝、平靜")
            kwargs["emo_vector"] = [max(0.0, min(1.0, float(v))) for v in vector]
            kwargs["emo_alpha"] = float(emo.get("strength") or 0.6)
        if seg.speed:
            kwargs["duration_factor"] = max(0.5, min(2.0, 1.0 / float(seg.speed)))
        self.tts.infer(
            spk_audio_prompt=seg.reference_audio,
            text=seg.text,
            output_path=seg.output_path,
            lang=lang,
            **kwargs,
        )
