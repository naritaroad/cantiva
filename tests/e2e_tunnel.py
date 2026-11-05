"""端到端測試：通道模式的 cantiva-mcp（Streamable HTTP）。

驗證：必須有一次性密鑰、只接受該密鑰、工具權限縮小、回傳不含本機路徑。
用法：cargo build 之後執行 `python tests/e2e_tunnel.py`（Linux／macOS）。
"""

import json
import os
import shutil
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / "debug"


def post(url, body, session=None, bearer=None):
    headers = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream",
               "MCP-Protocol-Version": "2025-11-25"}
    if session:
        headers["Mcp-Session-Id"] = session
    if bearer:
        headers["Authorization"] = f"Bearer {bearer}"
    req = urllib.request.Request(url, data=json.dumps(body).encode(), headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            raw = r.read().decode()
            sid = r.headers.get("Mcp-Session-Id")
            if raw.startswith("data:") or "\ndata:" in raw:
                raw = "\n".join(l[5:].strip() for l in raw.splitlines() if l.startswith("data:"))
            return r.status, (json.loads(raw) if raw.strip() else None), sid
    except urllib.error.HTTPError as e:
        return e.code, None, None


def main():
    home = Path(tempfile.mkdtemp(prefix="cantiva-tunnel-"))
    eng = home / "engines" / "dev-sine" / ".venv" / "bin"
    eng.mkdir(parents=True)
    os.symlink(shutil.which("python3"), eng / "python")
    (home / "engines" / "dev-sine" / "installed.json").write_text("{}")
    (home / "settings.toml").write_text('active_engine = "dev-sine"\n[mcp]\napi_port = 0\n', encoding="utf-8")
    env = {**os.environ, "CANTIVA_HOME": str(home), "CANTIVA_DEV": "1", "CANTIVA_ENGINES_DIR": str(ROOT / "engines")}

    # 沒有一次性密鑰時拒絕啟動
    r = subprocess.run([str(BIN / "cantiva-mcp"), "--tunnel", "--port", "0"], env=env, capture_output=True, text=True, timeout=20)
    assert r.returncode != 0 and "CANTIVA_TUNNEL_SECRET" in r.stderr, r.stderr

    headless = subprocess.Popen([str(BIN / "cantiva"), "--headless"], env=env)
    secret = "t" * 40
    mcp = subprocess.Popen([str(BIN / "cantiva-mcp"), "--tunnel", "--no-autostart", "--port", "0"],
                           env={**env, "CANTIVA_TUNNEL_SECRET": secret}, stdout=subprocess.PIPE, text=True)
    try:
        line = mcp.stdout.readline()
        assert line.startswith("CANTIVA_PORT "), line
        port = int(line.split()[1])
        for _ in range(50):
            if (home / "api-port").exists():
                break
            time.sleep(0.2)
        base = f"http://127.0.0.1:{port}"
        init = {"jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}}

        # 本機長期密鑰（mcp-secret）不能用在通道上
        local_secret = (home / "mcp-secret").read_text() if (home / "mcp-secret").exists() else "x" * 40
        assert post(f"{base}/mcp/{local_secret}", init)[0] in (401, 404)
        assert post(f"{base}/mcp", init)[0] == 401
        assert post(f"{base}/mcp", init, bearer="wrong" * 8)[0] == 401

        status, body, sid = post(f"{base}/mcp/{secret}", init)
        assert status == 200 and body["result"]["serverInfo"]["name"] == "cantiva", (status, body)
        post(f"{base}/mcp/{secret}", {"jsonrpc": "2.0", "method": "notifications/initialized"}, sid)
        _, body, _ = post(f"{base}/mcp/{secret}", {"jsonrpc": "2.0", "id": 2, "method": "tools/list"}, sid)
        names = {t["name"] for t in body["result"]["tools"]}
        assert {"speak", "list_voices", "status"} <= names, names
        assert not names & {"create_voice", "play_take", "load_engine", "unload_engine"}, names

        _, body, _ = post(f"{base}/mcp/{secret}", {"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                                                    "params": {"name": "speak", "arguments": {"text": "通道測試。", "voice": "dev-sine/high", "play": True}}}, sid)
        result = body["result"]
        assert not result.get("isError"), result
        take = json.loads(result["content"][0]["text"])
        assert "/" not in take["path"] and take["path"].endswith(".wav"), take

        _, body, _ = post(f"{base}/mcp/{secret}", {"jsonrpc": "2.0", "id": 4, "method": "tools/call",
                                                    "params": {"name": "status", "arguments": {}}}, sid)
        st = json.loads(body["result"]["content"][0]["text"])
        assert str(home) not in json.dumps(st, ensure_ascii=False), st
        print("通道模式測試全部通過")
    finally:
        mcp.kill()
        headless.terminate()
        try:
            headless.wait(timeout=15)
        except subprocess.TimeoutExpired:
            headless.kill()
        shutil.rmtree(home, ignore_errors=True)


if __name__ == "__main__":
    main()
