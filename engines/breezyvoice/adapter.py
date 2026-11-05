"""BreezyVoice-300M adapter（聯發創新基地，台灣華語）。

BreezyVoice 的文字正規化依賴 WeTextProcessing → pynini，pynini 沒有 Windows 版。
這裡在匯入前放一個相容模組 `tn.chinese.normalizer` / `tn.english.normalizer`，
內部改用有 Windows wheel 的 wetext，不必修改 BreezyVoice 原始碼。

注音語法：在字後面寫 `[:注音聲調]`，例如「好[:ㄏㄠ3]」。
"""

from __future__ import annotations

import os
import sys
import types

from cantiva_worker.base import Adapter as BaseAdapter
from cantiva_worker.base import AdapterInfo, Segment, SynthesisError, pick_device, set_seed


def install_wetext_shim() -> None:
    try:
        import tn.chinese.normalizer  # noqa: F401  已有 WeTextProcessing（例如 Linux）就用原本的
        return
    except Exception:
        pass
    from wetext import Normalizer

    class ZhNormalizer:
        def __init__(self, remove_erhua: bool = False, full_to_half: bool = False, **_: object) -> None:
            self._n = Normalizer(lang="zh", operator="tn", remove_erhua=remove_erhua, full_to_half=full_to_half)

        def normalize(self, text: str) -> str:
            return self._n.normalize(text)

    class EnNormalizer:
        def __init__(self, **_: object) -> None:
            self._n = Normalizer(lang="en", operator="tn")

        def normalize(self, text: str) -> str:
            return self._n.normalize(text)

    tn = types.ModuleType("tn")
    zh_pkg = types.ModuleType("tn.chinese")
    en_pkg = types.ModuleType("tn.english")
    zh_mod = types.ModuleType("tn.chinese.normalizer")
    en_mod = types.ModuleType("tn.english.normalizer")
    zh_mod.Normalizer = ZhNormalizer
    en_mod.Normalizer = EnNormalizer
    tn.chinese, tn.english = zh_pkg, en_pkg
    zh_pkg.normalizer, en_pkg.normalizer = zh_mod, en_mod
    sys.modules.update({
        "tn": tn, "tn.chinese": zh_pkg, "tn.english": en_pkg,
        "tn.chinese.normalizer": zh_mod, "tn.english.normalizer": en_mod,
    })


class Adapter(BaseAdapter):
    def load(self) -> None:
        repo = self.engine_home / "repo"
        model_dir = self.engine_home / "models" / "BreezyVoice-300M"
        if not (repo / "single_inference.py").is_file() or not model_dir.is_dir():
            raise SynthesisError("BreezyVoice 原始碼或模型不完整，請重新執行安裝")
        os.environ["PYTHONUTF8"] = "1"
        install_wetext_shim()
        os.chdir(repo)
        sys.path[:0] = [str(repo), str(repo / "third_party" / "Matcha-TTS")]
        from g2pw import G2PWConverter
        from single_inference import CustomCosyVoice, single_inference

        self.cv = CustomCosyVoice(str(model_dir))
        # G2PW 模型第一次會下載到工作目錄（repo）底下的 G2PWModel/。
        self.converter = G2PWConverter()
        self.run = single_inference
        print(f"[breezyvoice] 已載入，裝置 {pick_device()}", flush=True)

    def info(self) -> AdapterInfo:
        return AdapterInfo(sample_rate=22050)

    def synthesize(self, seg: Segment) -> None:
        if not seg.reference_audio:
            raise SynthesisError("BreezyVoice 沒有內建聲音，請先在聲音庫用參考音檔建立聲音")
        if not seg.reference_text:
            # 沒有逐字稿時 BreezyVoice 會自己跑 Whisper 轉錄，多吃顯示記憶體也比較不準。
            raise SynthesisError("BreezyVoice 需要參考音檔的逐字稿")
        set_seed(seg.seed)
        self.run(
            seg.reference_audio,
            seg.text,
            seg.output_path,
            self.cv,
            self.converter,
            speaker_prompt_text_transcription=seg.reference_text,
        )
