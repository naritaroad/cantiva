"""worker 入口：`python -m cantiva_worker --adapter <adapter.py> --engine-home <dir> --port <n>`。

權杖從環境變數 CANTIVA_WORKER_TOKEN 讀取；只綁定 127.0.0.1。
"""

from __future__ import annotations

import argparse
import hmac
import importlib.util
import json
import os
import sys
import threading
import time
import traceback
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

from .base import Adapter, Segment, SynthesisError


class State:
    def __init__(self) -> None:
        self.status = "loading"
        self.error: str | None = None
        self.adapter: Adapter | None = None
        self.lock = threading.Lock()  # 一次只合成一段（GPU 資源）
        self.stop = threading.Event()


def load_adapter_class(path: Path) -> type[Adapter]:
    spec = importlib.util.spec_from_file_location("cantiva_engine_adapter", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"讀不到 adapter：{path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["cantiva_engine_adapter"] = module
    spec.loader.exec_module(module)
    cls = getattr(module, "Adapter", None)
    if cls is None:
        raise RuntimeError(f"{path} 裡沒有 Adapter 類別")
    return cls


def make_handler(state: State, token: str):
    class Handler(BaseHTTPRequestHandler):
        server_version = "cantiva-worker"

        def log_message(self, fmt: str, *args: Any) -> None:  # 交給 stdout 記錄
            print(f"[http] {self.command} {self.path} -> {fmt % args}", flush=True)

        def _auth(self) -> bool:
            got = self.headers.get("X-Cantiva-Token", "")
            if not hmac.compare_digest(got, token):
                self._send(401, {"error": "權杖錯誤"})
                return False
            return True

        def _send(self, code: int, body: dict[str, Any]) -> None:
            data = json.dumps(body, ensure_ascii=False).encode("utf-8")
            self.send_response(code)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self) -> None:  # noqa: N802
            if not self._auth():
                return
            if self.path == "/health":
                body: dict[str, Any] = {"state": state.status}
                if state.error:
                    body["error"] = state.error
                if state.adapter is not None and state.status == "ready":
                    body["vram_mb"] = state.adapter.vram_mb()
                self._send(200, body)
            elif self.path == "/info":
                if state.adapter is None or state.status != "ready":
                    self._send(503, {"error": "模型還在載入"})
                    return
                info = state.adapter.info()
                self._send(200, {"sample_rate": info.sample_rate, "speakers": info.speakers})
            else:
                self._send(404, {"error": "沒有這個路徑"})

        def do_POST(self) -> None:  # noqa: N802
            if not self._auth():
                return
            if self.path == "/shutdown":
                self._send(200, {"ok": True})
                state.stop.set()
                return
            if self.path != "/synthesize":
                self._send(404, {"error": "沒有這個路徑"})
                return
            if state.adapter is None or state.status != "ready":
                self._send(503, {"error": "模型還在載入"})
                return
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if length <= 0 or length > 10 * 1024 * 1024:
                    raise ValueError("請求內容大小不合理")
                seg = Segment.from_json(json.loads(self.rfile.read(length).decode("utf-8")))
                out = Path(seg.output_path)
                if not out.is_absolute():
                    raise ValueError("output_path 必須是絕對路徑")
                out.parent.mkdir(parents=True, exist_ok=True)
                started = time.time()
                with state.lock:
                    state.adapter.synthesize(seg)
                if not out.is_file():
                    raise SynthesisError("模型沒有產生音檔")
                duration = wav_duration(out)
                print(f"[synth] {len(seg.text)} 字 -> {duration:.2f}s，耗時 {time.time() - started:.2f}s", flush=True)
                self._send(200, {"path": str(out), "sample_rate": state.adapter.info().sample_rate, "duration_sec": duration})
            except (ValueError, SynthesisError) as e:
                self._send(400, {"error": str(e)})
            except Exception as e:  # 模型內部錯誤：記錄完整 traceback，回傳簡短訊息
                traceback.print_exc()
                self._send(500, {"error": f"{type(e).__name__}: {e}"})

    return Handler


def make_parent_watch():
    """回傳一個函式：父程序還活著就回傳 True。"""
    pid = int(os.environ.get("CANTIVA_PARENT_PID", "0") or 0)
    if os.name == "nt" and pid > 0:
        import ctypes

        SYNCHRONIZE = 0x00100000
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel32.OpenProcess.restype = ctypes.c_void_p
        kernel32.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
        handle = kernel32.OpenProcess(SYNCHRONIZE, False, pid)
        if not handle:
            return lambda: False
        # WaitForSingleObject 回傳 0（WAIT_OBJECT_0）代表程序已結束。
        return lambda: kernel32.WaitForSingleObject(handle, 0) != 0
    original = os.getppid()
    if pid > 0:
        return lambda: os.getppid() == original and _pid_exists(pid)
    return lambda: os.getppid() == original


def _pid_exists(pid: int) -> bool:
    try:
        os.kill(pid, 0)
        return True
    except PermissionError:
        return True
    except OSError:
        return False


def wav_duration(path: Path) -> float:
    import wave

    with wave.open(str(path), "rb") as w:
        return w.getnframes() / float(w.getframerate())


def selftest(args: argparse.Namespace) -> int:
    """安裝後的自我測試：載入模型，有內建說話人就合成一句，確認整套環境可用。"""
    started = time.time()
    try:
        cls = load_adapter_class(args.adapter)
        adapter = cls(args.engine_home)
        adapter.load()
        info = adapter.info()
        print(f"[selftest] 模型載入成功（{time.time() - started:.1f}s），取樣率 {info.sample_rate}", flush=True)
        if info.speakers:
            out = args.engine_home / "selftest.wav"
            seg = Segment(text="安裝完成，聲音測試成功。", output_path=str(out), speaker=info.speakers[0])
            adapter.synthesize(seg)
            print(f"[selftest] 合成成功：{out}（{wav_duration(out):.2f}s）", flush=True)
        vram = adapter.vram_mb()
        if vram is not None:
            print(f"[selftest] 顯示記憶體使用約 {vram} MB", flush=True)
        adapter.close()
        return 0
    except Exception:
        traceback.print_exc()
        print("[selftest] 失敗", flush=True)
        return 1


def main() -> int:
    parser = argparse.ArgumentParser(prog="cantiva_worker")
    parser.add_argument("--adapter", required=True, type=Path)
    parser.add_argument("--engine-home", required=True, type=Path)
    parser.add_argument("--port", type=int)
    parser.add_argument("--selftest", action="store_true", help="載入模型並試合成一句後結束")
    args = parser.parse_args()
    args.engine_home = args.engine_home.resolve()
    if args.selftest:
        return selftest(args)
    if args.port is None:
        parser.error("需要 --port")

    token = os.environ.get("CANTIVA_WORKER_TOKEN", "")
    if len(token) < 16:
        print("缺少 CANTIVA_WORKER_TOKEN", file=sys.stderr)
        return 2

    state = State()
    server = ThreadingHTTPServer(("127.0.0.1", args.port), make_handler(state, token))
    server.daemon_threads = True

    def do_load() -> None:
        try:
            cls = load_adapter_class(args.adapter)
            adapter = cls(args.engine_home)
            print(f"[load] {args.adapter.parent.name} 載入中…", flush=True)
            started = time.time()
            adapter.load()
            state.adapter = adapter
            state.status = "ready"
            print(f"[load] 完成，耗時 {time.time() - started:.1f}s", flush=True)
        except Exception as e:
            traceback.print_exc()
            state.error = f"{type(e).__name__}: {e}"
            state.status = "error"

    threading.Thread(target=do_load, daemon=True).start()
    threading.Thread(target=server.serve_forever, daemon=True).start()
    print(f"[worker] 127.0.0.1:{args.port} 啟動", flush=True)

    # 父程序（Cantiva）消失時自己結束，避免殘留的程序佔住顯示記憶體。
    parent_alive = make_parent_watch()
    while not state.stop.wait(1.0):
        if not parent_alive():
            print("[worker] Cantiva 已結束，worker 跟著結束", flush=True)
            break
    server.shutdown()
    if state.adapter is not None:
        state.adapter.close()
    print("[worker] 結束", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
