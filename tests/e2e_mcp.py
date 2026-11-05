"""端到端測試：模擬 AI agent，透過 stdio 對 cantiva-mcp 送 MCP 請求。

cantiva-mcp 會自動以無視窗模式啟動 cantiva，再由開發假引擎（dev-sine）合成。
用法：cargo build 之後執行 `python tests/e2e_mcp.py [target/debug]`（Linux、macOS、Windows）。
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
BIN = Path(sys.argv[1] if len(sys.argv) > 1 else ROOT / "target" / "debug")
EXE = ".exe" if os.name == "nt" else ""


class Mcp:
    def __init__(self, env):
        self.p = subprocess.Popen([str(BIN / f"cantiva-mcp{EXE}")], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, env=env, text=True, encoding="utf-8")
        self.n = 0

    def send(self, method, params=None, notify=False):
        msg = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            msg["params"] = params
        if not notify:
            self.n += 1
            msg["id"] = self.n
        self.p.stdin.write(json.dumps(msg, ensure_ascii=False) + "\n")
        self.p.stdin.flush()
        if notify:
            return None
        while True:
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError("cantiva-mcp 結束了：" + self.p.stderr.read())
            resp = json.loads(line)
            if resp.get("id") == self.n:
                if "error" in resp:
                    raise RuntimeError(resp["error"])
                return resp["result"]

    def call(self, name, args=None):
        r = self.send("tools/call", {"name": name, "arguments": args or {}})
        text = r["content"][0]["text"]
        return r.get("isError", False), text

    def close(self):
        self.p.stdin.close()
        self.p.wait(timeout=10)


def main():
    home = Path(tempfile.mkdtemp(prefix="cantiva-e2e-"))
    venv = home / "engines" / "dev-sine" / ".venv"
    if os.name == "nt":
        # Windows 的 python.exe 不能單獨 symlink，建一個真的 venv（dev-sine 只用標準函式庫）。
        subprocess.run([sys.executable, "-m", "venv", "--without-pip", str(venv)], check=True)
    else:
        (venv / "bin").mkdir(parents=True)
        os.symlink(shutil.which("python3"), venv / "bin" / "python")
    (home / "engines" / "dev-sine" / "installed.json").write_text("{}")
    env = {**os.environ, "CANTIVA_HOME": str(home), "CANTIVA_DEV": "1", "CANTIVA_ENGINES_DIR": str(ROOT / "engines")}
    # 設定檔：預設引擎改成假引擎，api 連接埠用 0（自動挑選）避免和開發機上的 Cantiva 衝突。
    (home / "settings.toml").write_text('active_engine = "dev-sine"\n[mcp]\napi_port = 0\n', encoding="utf-8")

    mcp = Mcp(env)
    try:
        init = mcp.send("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                                       "clientInfo": {"name": "e2e", "version": "0"}})
        assert init["serverInfo"]["name"] == "cantiva", init
        assert "本機語音工作室" in init.get("instructions", "")
        mcp.send("notifications/initialized", notify=True)
        tools = {t["name"]: t for t in mcp.send("tools/list")["tools"]}
        expected = {"status", "list_engines", "list_voices", "speak", "render_script", "list_takes",
                    "create_voice", "load_engine", "unload_engine", "play_take"}
        assert expected <= set(tools), sorted(tools)
        assert tools["status"]["annotations"]["readOnlyHint"] is True

        err, text = mcp.call("status")
        assert not err, text
        print("status:", json.loads(text))

        err, text = mcp.call("list_voices", {"engine": "dev-sine"})
        voices = [v["name"] for v in json.loads(text)["voices"]]
        assert "dev-sine/high" in voices, voices

        err, text = mcp.call("speak", {"text": "你好，這是端到端測試。第二句話在這裡！", "voice": "dev-sine/high",
                                       "file_name": "e2e", "project": "測試專案"})
        assert not err, text
        take = json.loads(text)
        assert Path(take["path"]).is_file() and take["path"].endswith("e2e.wav"), take
        print("speak:", take)

        err, text = mcp.call("speak", {"text": "嗨", "voice": "不存在的聲音"})
        assert err and "找不到聲音" in text, text

        err, text = mcp.call("render_script", {"project": "廣播劇", "script": "@dev-sine/low：第一行。\n\n第二行。",
                                               "default_voice": "dev-sine/high"})
        assert not err, text
        assert json.loads(text)["lines"] == 2

        err, text = mcp.call("list_takes", {"limit": 5})
        assert len(json.loads(text)["takes"]) == 3

        err, text = mcp.call("play_take", {"take_id": take["take_id"]})
        assert err and "無視窗模式" in text, text
        err, text = mcp.call("unload_engine")
        assert not err, text
        print("全部通過")
    finally:
        mcp.close()
        stop_headless(home)
        shutil.rmtree(home, ignore_errors=True)


def stop_headless(home):
    """透過控制 API 請無視窗模式的 cantiva 結束（Windows 沒有 pkill）。"""
    port_file, token_file = home / "api-port", home / "api-token"
    if port_file.exists() and token_file.exists():
        req = urllib.request.Request(
            f"http://127.0.0.1:{port_file.read_text().strip()}/v1/shutdown", data=b"", method="POST",
            headers={"Authorization": f"Bearer {token_file.read_text().strip()}"},
        )
        try:
            urllib.request.urlopen(req, timeout=5).close()
        except OSError:
            pass
    elif os.name != "nt":
        subprocess.run(["pkill", "-f", f"{BIN}/cantiva --headless"], check=False)
    # 等它放開資料夾裡的檔案（Windows 上檔案開著就刪不掉）。
    time.sleep(1.5)


if __name__ == "__main__":
    main()
