"""GPT-SoVITS adapter（v2ProPlus，GPT_SoVITS.TTS_infer_pack.TTS）。

- 必須提供 3–10 秒的參考音檔（GPT-SoVITS 原始碼強制檢查）。
- 聲音庫裡的「角色模型」會帶 GPT（.ckpt）與 SoVITS（.pth）權重；換聲音時才切換權重。
- 官方程式要以 repo 根目錄為工作目錄，並把根目錄與 GPT_SoVITS 放進 sys.path。
"""

from __future__ import annotations

import os
import re
import sys
import wave
from pathlib import Path

from cantiva_worker.base import Adapter as BaseAdapter
from cantiva_worker.base import AdapterInfo, Segment, SynthesisError, pick_device

VERSION = "v2ProPlus"
BASE_GPT = "GPT_SoVITS/pretrained_models/s1v3.ckpt"
BASE_SOVITS = "GPT_SoVITS/pretrained_models/v2Pro/s2Gv2ProPlus.pth"
LANG = {"zh": "zh", "zh-tw": "zh", "zh-cn": "zh", "ja": "ja", "en": "en", "ko": "ko", "yue": "yue"}
KANA = re.compile(r"[぀-ヿ]")
HANGUL = re.compile(r"[가-힯]")
CJK = re.compile(r"[一-鿿]")


def guess_language(text: str) -> str:
    """有假名就當日文、有諺文就當韓文；其他交給 GPT-SoVITS 的自動判斷。"""
    if KANA.search(text):
        return "ja"
    if HANGUL.search(text):
        return "ko"
    if CJK.search(text):
        return "zh"
    return "auto"


def wav_seconds(path: str) -> float:
    try:
        with wave.open(path, "rb") as w:
            return w.getnframes() / float(w.getframerate())
    except Exception:
        return -1.0


class Adapter(BaseAdapter):
    def load(self) -> None:
        repo = self.engine_home / "repo"
        if not (repo / "GPT_SoVITS" / "TTS_infer_pack" / "TTS.py").is_file():
            raise SynthesisError("GPT-SoVITS 原始碼不完整，請重新執行安裝")
        os.chdir(repo)
        sys.path[:0] = [str(repo), str(repo / "GPT_SoVITS")]
        try:
            from tools.portable_runtime import activate

            activate(change_cwd=True)
        except Exception as e:  # 舊版沒有 portable_runtime，不影響推論
            print(f"[gpt-sovits] 略過 portable_runtime：{e}", flush=True)
        from GPT_SoVITS.TTS_infer_pack.TTS import TTS, TTS_Config

        device = "cuda" if pick_device().startswith("cuda") else "cpu"
        config = TTS_Config({
            "custom": {
                "device": device,
                "is_half": device == "cuda",
                "version": VERSION,
                "t2s_weights_path": BASE_GPT,
                "vits_weights_path": BASE_SOVITS,
                "bert_base_path": "GPT_SoVITS/pretrained_models/chinese-roberta-wwm-ext-large",
                "cnhuhbert_base_path": "GPT_SoVITS/pretrained_models/chinese-hubert-base",
            }
        })
        self.tts = TTS(config)
        self.weights: tuple[str | None, str | None] = (BASE_GPT, BASE_SOVITS)
        print(f"[gpt-sovits] 已載入 {VERSION}，裝置 {device}", flush=True)

    def info(self) -> AdapterInfo:
        return AdapterInfo(sample_rate=32000)

    def _use_weights(self, gpt: str | None, sovits: str | None) -> None:
        want = (gpt or BASE_GPT, sovits or BASE_SOVITS)
        if want == self.weights:
            return
        print(f"[gpt-sovits] 切換權重：{Path(want[0]).name} / {Path(want[1]).name}", flush=True)
        # 每載入一個就記一個：第二個失敗時，下次請求仍會重新載入它。
        if want[0] != self.weights[0]:
            self.weights = (None, self.weights[1])
            self.tts.init_t2s_weights(want[0])
            self.weights = (want[0], self.weights[1])
        if want[1] != self.weights[1]:
            self.weights = (self.weights[0], None)
            self.tts.init_vits_weights(want[1])
            self.weights = (self.weights[0], want[1])

    def synthesize(self, seg: Segment) -> None:
        import numpy as np

        if not seg.reference_audio:
            raise SynthesisError("GPT-SoVITS 沒有內建聲音，請先在聲音庫用參考音檔建立聲音")
        secs = wav_seconds(seg.reference_audio)
        if not (3.0 <= secs <= 10.0):
            raise SynthesisError(f"GPT-SoVITS 的參考音檔要 3–10 秒，這個是 {secs:.1f} 秒。請重新剪一段")
        self._use_weights(seg.gpt_weights, seg.sovits_weights)
        text_lang = LANG.get((seg.language or "").lower()) or guess_language(seg.text)
        prompt_text = seg.reference_text or ""
        prompt_lang = LANG.get((seg.reference_language or "").lower()) or guess_language(prompt_text or seg.text)
        if prompt_lang == "auto":
            prompt_lang = text_lang if text_lang != "auto" else "zh"
        inputs = {
            "text": seg.text,
            "text_lang": text_lang,
            "ref_audio_path": seg.reference_audio,
            "prompt_text": prompt_text,
            "prompt_lang": prompt_lang,
            "text_split_method": "cut5",
            "batch_size": 1,
            "speed_factor": float(seg.speed or 1.0),
            "seed": int(seg.seed) if seg.seed is not None else -1,
            "top_k": 15,
            "top_p": 1.0,
            "temperature": 1.0,
            "repetition_penalty": 1.35,
            "parallel_infer": True,
        }
        try:
            sr, audio = next(self.tts.run(inputs))
        except StopIteration:
            raise SynthesisError("GPT-SoVITS 沒有產生音訊") from None
        pcm = np.asarray(audio, dtype=np.int16).reshape(-1)
        if pcm.size <= int(sr) and not np.any(pcm):
            raise SynthesisError("GPT-SoVITS 回傳了空白音訊，請檢查文字與語言設定")
        tmp = Path(seg.output_path + ".part")
        with wave.open(str(tmp), "wb") as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(int(sr))
            w.writeframes(pcm.tobytes())
        tmp.replace(seg.output_path)
