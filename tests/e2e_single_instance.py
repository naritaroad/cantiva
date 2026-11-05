"""單一實例：第二個 cantiva --headless 要立刻結束，不能搶走控制 API 或另外載入模型。"""

import os
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / "debug"

home = Path(tempfile.mkdtemp(prefix="cantiva-single-"))
(home / "settings.toml").write_text("[mcp]\napi_port = 0\n", encoding="utf-8")
env = {**os.environ, "CANTIVA_HOME": str(home), "CANTIVA_ENGINES_DIR": str(ROOT / "engines")}
first = subprocess.Popen([str(BIN / "cantiva"), "--headless"], env=env)
try:
    for _ in range(50):
        if (home / "api-port").exists():
            break
        time.sleep(0.2)
    port_before = (home / "api-port").read_text()
    second = subprocess.run([str(BIN / "cantiva"), "--headless"], env=env, timeout=30)
    assert second.returncode == 0
    assert (home / "api-port").read_text() == port_before, "第二個實例覆蓋了 api-port"
    assert first.poll() is None, "第一個實例不應該被影響"
    print("單一實例測試通過")
finally:
    first.terminate()
    first.wait(timeout=15)
    shutil.rmtree(home, ignore_errors=True)
