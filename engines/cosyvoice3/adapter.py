"""Fun-CosyVoice3-0.5B-2512 adapter。

官方程式要在 repo 根目錄執行（相對路徑匯入 third_party/Matcha-TTS），
所以 load() 會切換工作目錄。必須提供參考音檔；有語氣指示時改用 instruct2。
"""

from __future__ import annotations

import os
import sys

from cantiva_worker.base import Adapter as BaseAdapter
from cantiva_worker.base import AdapterInfo, Segment, SynthesisError, pick_device, save_tensor_wav, set_seed

PREFIX = "You are a helpful assistant."
END = "<|endofprompt|>"


class Adapter(BaseAdapter):
    def load(self) -> None:
        repo = self.engine_home / "repo"
        model_dir = self.engine_home / "models" / "Fun-CosyVoice3-0.5B"
        if not (repo / "cosyvoice").is_dir() or not model_dir.is_dir():
            raise SynthesisError("CosyVoice 原始碼或模型不完整，請重新執行安裝")
        os.chdir(repo)
        sys.path[:0] = [str(repo), str(repo / "third_party" / "Matcha-TTS")]
        from cosyvoice.cli.cosyvoice import AutoModel

        self.cv = AutoModel(model_dir=str(model_dir), load_trt=False, load_vllm=False, fp16=False)
        self.sr = int(self.cv.sample_rate)
        print(f"[cosyvoice3] 已載入，{self.sr} Hz，裝置 {pick_device()}", flush=True)

    def info(self) -> AdapterInfo:
        return AdapterInfo(sample_rate=24000)

    def synthesize(self, seg: Segment) -> None:
        import torch

        if not seg.reference_audio:
            raise SynthesisError("CosyVoice3 沒有內建聲音，請先在聲音庫用參考音檔建立聲音")
        set_seed(seg.seed)
        speed = float(seg.speed or 1.0)
        if seg.instruct:
            instruct = f"{PREFIX} {seg.instruct.strip()}{END}"
            gen = self.cv.inference_instruct2(seg.text, instruct, seg.reference_audio, stream=False, speed=speed)
        else:
            if not seg.reference_text:
                raise SynthesisError("CosyVoice3 需要參考音檔的逐字稿")
            prompt = f"{PREFIX}{END}{seg.reference_text}"
            gen = self.cv.inference_zero_shot(seg.text, prompt, seg.reference_audio, stream=False, speed=speed)
        chunks = [item["tts_speech"] for item in gen]
        if not chunks:
            raise SynthesisError("模型沒有產生音訊")
        save_tensor_wav(seg.output_path, torch.cat(chunks, dim=1), self.sr)
