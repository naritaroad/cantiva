"""worker 與 adapter 的單元測試（不需要 GPU 或模型）：python -m unittest discover engines/_worker/tests"""

import importlib.util
import os
import subprocess
import sys
import tempfile
import types
import unittest
from pathlib import Path

ENGINES = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ENGINES / "_worker"))

from cantiva_worker.base import Segment  # noqa: E402


def load(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


class SegmentTests(unittest.TestCase):
    def test_rejects_empty_text_and_ignores_unknown_fields(self):
        with self.assertRaises(ValueError):
            Segment.from_json({"text": "  ", "output_path": "/tmp/x.wav"})
        seg = Segment.from_json({"text": "嗨", "output_path": "/tmp/x.wav", "unknown": 1, "speed": 1.2})
        self.assertEqual(seg.speed, 1.2)


class SelftestTests(unittest.TestCase):
    def test_dev_engine_selftest_writes_audio(self):
        with tempfile.TemporaryDirectory() as home:
            # 子程序輸出 UTF-8；Windows 預設會用系統字碼頁（例如 cp950）解碼，所以兩邊都明確指定。
            env = {**os.environ, "PYTHONPATH": str(ENGINES / "_worker"), "PYTHONIOENCODING": "utf-8", "PYTHONUTF8": "1"}
            r = subprocess.run(
                [sys.executable, "-m", "cantiva_worker", "--adapter", str(ENGINES / "dev-sine" / "adapter.py"),
                 "--engine-home", home, "--selftest"],
                env=env, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60,
            )
            self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
            self.assertTrue((Path(home) / "selftest.wav").is_file())


class IndexTtsEmotionTests(unittest.TestCase):
    def test_keywords_map_to_vector_order(self):
        mod = load(ENGINES / "indextts2" / "adapter.py", "idx_adapter")
        vec = mod.emotion_from_text("有點難過但保持冷靜")
        self.assertEqual(vec[2], 1.0)
        self.assertEqual(vec[7], 1.0)
        self.assertIsNone(mod.emotion_from_text("沒有情緒字"))


class BreezyShimTests(unittest.TestCase):
    def test_shim_wraps_wetext(self):
        calls = []

        class FakeNormalizer:
            def __init__(self, **kw):
                calls.append(kw)

            def normalize(self, text):
                return text.replace("1", "一")

        sys.modules["wetext"] = types.SimpleNamespace(Normalizer=FakeNormalizer)
        for m in [k for k in sys.modules if k == "tn" or k.startswith("tn.")]:
            del sys.modules[m]
        mod = load(ENGINES / "breezyvoice" / "adapter.py", "breezy_adapter")
        mod.install_wetext_shim()
        from tn.chinese.normalizer import Normalizer as Zh
        from tn.english.normalizer import Normalizer as En

        self.assertEqual(Zh(remove_erhua=False, full_to_half=False).normalize("第1"), "第一")
        En()
        self.assertEqual(calls[0]["lang"], "zh")
        self.assertEqual(calls[1]["lang"], "en")


if __name__ == "__main__":
    unittest.main()
