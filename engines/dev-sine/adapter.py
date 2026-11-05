"""開發用假引擎：每個字 0.12 秒的正弦波，音高依說話人而定。"""

from __future__ import annotations

import math
import time

from cantiva_worker.base import Adapter as BaseAdapter
from cantiva_worker.base import AdapterInfo, Segment, SynthesisError, write_wav_int16

RATE = 24000


class Adapter(BaseAdapter):
    def load(self) -> None:
        time.sleep(0.3)  # 模擬載入

    def info(self) -> AdapterInfo:
        return AdapterInfo(sample_rate=RATE, speakers=["low", "high"])

    def synthesize(self, segment: Segment) -> None:
        if "FAIL" in segment.text:
            raise SynthesisError("測試用的合成失敗")
        freq = 660.0 if segment.speaker == "high" else 220.0
        if segment.reference_audio:
            freq = 440.0
        if segment.gpt_weights or segment.sovits_weights:
            # 測試用：確認權重路徑真的有傳進來而且檔案存在
            from pathlib import Path

            if not (Path(segment.gpt_weights or "").is_file() and Path(segment.sovits_weights or "").is_file()):
                raise SynthesisError("角色模型權重檔不存在")
            freq = 550.0
        speed = segment.speed or 1.0
        per_char = int(RATE * 0.12 / speed)
        # 每個字一個「音節」：Hann 包絡、音量依字元變化，標點處停頓，波形看起來像說話。
        tone: list[float] = []
        for k, ch in enumerate(segment.text):
            if not ch.strip():
                continue
            if ch in "，。！？、；：,.!?;:」「（）":
                tone.extend([0.0] * (per_char // 2))
                continue
            amp = 0.15 + 0.3 * ((ord(ch) * 2654435761 >> 7) % 100) / 100
            for i in range(per_char):
                env = math.sin(math.pi * i / per_char) ** 2
                tone.append(amp * env * math.sin(2 * math.pi * freq * (k * per_char + i) / RATE))
        if not tone:
            tone = [0.0] * int(RATE * 0.2)
        # 前後各加一點靜音，讓 Rust 端的修剪邏輯有東西可修。
        pad = [0.0] * int(RATE * 0.2)
        write_wav_int16(segment.output_path, pad + tone + pad, RATE)
