"""建立示範資料並在 Xvfb 上截取每一頁（Linux 開發機用）。

用法：python tests/screenshots.py <輸出資料夾>
用開發假引擎（dev-sine）產生示範片段；畫面上的音檔是嗶聲，不是真的語音。
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / "debug"
OUT = Path(sys.argv[1] if len(sys.argv) > 1 else "shots")
OUT.mkdir(parents=True, exist_ok=True)

home = Path(tempfile.mkdtemp(prefix="cantiva-shots-"))
for eng in ["dev-sine", "qwen3-tts", "gpt-sovits"]:
    d = home / "engines" / eng / ".venv" / "bin"
    d.mkdir(parents=True)
    os.symlink(shutil.which("python3"), d / "python")
    (home / "engines" / eng / "installed.json").write_text("{}")
(home / "settings.toml").write_text('active_engine = "dev-sine"\nlast_project = "港口夜話"\n[mcp]\napi_port = 0\n', encoding="utf-8")
env = {**os.environ, "CANTIVA_HOME": str(home), "CANTIVA_DEV": "1", "CANTIVA_ENGINES_DIR": str(ROOT / "engines"),
       "SLINT_BACKEND": "winit-software"}


def api(path, body=None):
    port = (home / "api-port").read_text().strip()
    token = (home / "api-token").read_text().strip()
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", method="POST" if body is not None else "GET",
                                 data=json.dumps(body).encode() if body is not None else None,
                                 headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=120) as r:
        return json.loads(r.read() or b"null")


# 1. 用無視窗模式做示範資料
h = subprocess.Popen([str(BIN / "cantiva"), "--headless"], env=env)
for _ in range(50):
    if (home / "api-port").exists():
        try:
            api("/v1/health")
            break
        except Exception:
            pass
    time.sleep(0.2)
script = """# 第一幕　港口
@dev-sine/low：{低沉、緩慢}夜深了，港口只剩下最後一盞燈。

@dev-sine/high：你還在等那艘船嗎？

@dev-sine/low：嗯。它說好今晚會回來的。

@dev-sine/high：{輕聲}那我陪你等。風有點冷，靠過來一點。

@dev-sine/low：遠方好像有光。是它嗎？"""
take = api("/v1/speak", {"text": "這是一段拿來當參考音檔的示範錄音，長度大約四秒鐘。", "voice": "dev-sine/low", "project": "快速合成"})
api("/v1/voices", {"name": "我的旁白", "engine": "dev-sine", "audio_path": take["path"], "transcript": "這是一段拿來當參考音檔的示範錄音",
                   "consent_basis": "own_voice", "consent_statement": "我本人的聲音"})
api("/v1/script", {"project": "港口夜話", "script": script, "default_voice": "dev-sine/low"})
try:
    api("/v1/shutdown", {})
except Exception:
    pass
h.wait(timeout=20)

# 2. 每一頁截一張
for page, name in enumerate(["voices", "script", "timeline", "deliver", "engines", "connect"]):
    e = {**env, "CANTIVA_PAGE": str(page)}
    p = subprocess.Popen(["xvfb-run", "-a", "-s", "-screen 0 1440x860x24", "sh", "-c",
                          f"'{BIN / 'cantiva'}' & sleep 6; import -window root '{OUT / (str(page) + '-' + name + '.png')}'; kill %1"],
                         env=e)
    p.wait(timeout=60)
    time.sleep(0.5)
print("截圖完成：", sorted(x.name for x in OUT.glob("*.png")))
shutil.rmtree(home, ignore_errors=True)
