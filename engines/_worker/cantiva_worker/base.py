"""Adapter 介面與共用小工具。"""

from __future__ import annotations

import struct
import wave
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Sequence


@dataclass
class Segment:
    """一段要合成的文字，欄位對應 Rust 端的 SegmentRequest。"""

    text: str
    output_path: str
    speaker: str | None = None
    reference_audio: str | None = None
    reference_text: str | None = None
    reference_language: str | None = None
    gpt_weights: str | None = None
    sovits_weights: str | None = None
    instruct: str | None = None
    emotion: dict[str, Any] | None = None
    speed: float | None = None
    language: str | None = None
    seed: int | None = None

    @classmethod
    def from_json(cls, data: dict[str, Any]) -> "Segment":
        known = {f for f in cls.__dataclass_fields__}  # type: ignore[attr-defined]
        if not isinstance(data.get("text"), str) or not data["text"].strip():
            raise ValueError("text 不能是空的")
        if not isinstance(data.get("output_path"), str):
            raise ValueError("缺少 output_path")
        return cls(**{k: v for k, v in data.items() if k in known})


@dataclass
class AdapterInfo:
    sample_rate: int
    speakers: list[str] = field(default_factory=list)


class SynthesisError(Exception):
    """給使用者看的合成錯誤（例如參考音檔格式不對）。"""


class Adapter:
    """每個引擎實作這個類別。

    - `load()` 在背景執行緒呼叫一次；可以很久（載入權重）。
    - `synthesize()` 一次一段，必須把 WAV 寫到 segment.output_path。
    - 所有路徑都是絕對路徑。
    """

    def __init__(self, engine_home: Path) -> None:
        self.engine_home = engine_home

    def load(self) -> None:  # pragma: no cover - 由各引擎實作
        raise NotImplementedError

    def info(self) -> AdapterInfo:  # pragma: no cover
        raise NotImplementedError

    def synthesize(self, segment: Segment) -> None:  # pragma: no cover
        raise NotImplementedError

    def vram_mb(self) -> int | None:
        try:
            import torch  # type: ignore

            if torch.cuda.is_available():
                return int(torch.cuda.memory_allocated() / (1024 * 1024))
        except Exception:
            return None
        return None

    def close(self) -> None:
        try:
            import gc

            import torch  # type: ignore

            gc.collect()
            if torch.cuda.is_available():
                torch.cuda.empty_cache()
        except Exception:
            pass


def write_wav_int16(path: str | Path, samples: Sequence[float], sample_rate: int) -> None:
    """不依賴 numpy／soundfile 的 WAV 寫入，給沒有這些套件的 adapter 或測試用。"""
    frames = bytearray()
    for s in samples:
        v = max(-1.0, min(1.0, float(s)))
        frames += struct.pack("<h", int(round(v * 32767)))
    tmp = Path(str(path) + ".part")
    with wave.open(str(tmp), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(sample_rate)
        w.writeframes(bytes(frames))
    tmp.replace(path)


def save_tensor_wav(path: str | Path, audio: Any, sample_rate: int) -> None:
    """把 torch tensor 或 numpy array（形狀 [T]、[1, T] 或 [T, 1]）存成 16-bit WAV。"""
    import numpy as np  # type: ignore

    if hasattr(audio, "detach"):
        audio = audio.detach().float().cpu().numpy()
    arr = np.asarray(audio, dtype=np.float32)
    if arr.ndim == 2:
        arr = arr[0] if arr.shape[0] < arr.shape[1] else arr[:, 0]
    arr = np.clip(arr.reshape(-1), -1.0, 1.0)
    pcm = (arr * 32767.0).round().astype("<i2")
    tmp = Path(str(path) + ".part")
    with wave.open(str(tmp), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(int(sample_rate))
        w.writeframes(pcm.tobytes())
    tmp.replace(path)


def set_seed(seed: int | None) -> None:
    if seed is None:
        return
    import random

    random.seed(seed)
    try:
        import numpy as np  # type: ignore

        np.random.seed(seed % (2**32))
    except Exception:
        pass
    try:
        import torch  # type: ignore

        torch.manual_seed(seed)
        if torch.cuda.is_available():
            torch.cuda.manual_seed_all(seed)
    except Exception:
        pass


def pick_device() -> str:
    try:
        import torch  # type: ignore

        if torch.cuda.is_available():
            return "cuda:0"
    except Exception:
        pass
    return "cpu"
