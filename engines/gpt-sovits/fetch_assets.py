"""下載 GPT-SoVITS v2ProPlus 推論需要的預訓練模型與資料檔（由 install.ps1 用 venv 的 python 執行）。

只下載推論用得到的部分（約 1.5 GB），不下載 v3／v4／v5、訓練用的判別器與 ASR 模型。
來源：lj1995/GPT-SoVITS（官方權重）與 XXXXRT/GPT-SoVITS-Pretrained（官方 install.ps1 使用的打包）。
"""

from __future__ import annotations

import shutil
import sys
import tarfile
import zipfile
from pathlib import Path

from huggingface_hub import hf_hub_download, snapshot_download

repo = Path(sys.argv[1]).resolve()
pretrained = repo / "GPT_SoVITS" / "pretrained_models"
pretrained.mkdir(parents=True, exist_ok=True)

print("下載預訓練權重（chinese-roberta、chinese-hubert、s1v3、v2ProPlus、說話人驗證模型）…", flush=True)
snapshot_download(
    repo_id="lj1995/GPT-SoVITS",
    local_dir=str(pretrained),
    allow_patterns=[
        "chinese-roberta-wwm-ext-large/*",
        "chinese-hubert-base/*",
        "s1v3.ckpt",
        "v2Pro/s2Gv2ProPlus.pth",
        "sv/*",
    ],
)

bundle = "XXXXRT/GPT-SoVITS-Pretrained"

lid = pretrained / "fast_langdetect" / "lid.176.bin"
if not lid.is_file():
    print("下載語言偵測模型…", flush=True)
    hf_hub_download(bundle, "pretrained_models/fast_langdetect/lid.176.bin", local_dir=str(repo / "GPT_SoVITS"))

g2pw_dir = repo / "GPT_SoVITS" / "text" / "G2PWModel"
if not g2pw_dir.is_dir():
    print("下載中文破音字模型 G2PW…", flush=True)
    z = hf_hub_download(bundle, "G2PWModel.zip", local_dir=str(repo / ".downloads"))
    with zipfile.ZipFile(z) as f:
        f.extractall(repo / "GPT_SoVITS" / "text")

nltk_dir = Path(sys.prefix) / "nltk_data"
if not nltk_dir.is_dir():
    print("下載英文斷詞資料 nltk_data…", flush=True)
    z = hf_hub_download(bundle, "nltk_data.zip", local_dir=str(repo / ".downloads"))
    with zipfile.ZipFile(z) as f:
        f.extractall(Path(sys.prefix))

import pyopenjtalk  # noqa: E402  日文前端；裝不起來代表缺 CMake 或 MSVC

jtalk_dir = Path(pyopenjtalk.__file__).parent
if not (jtalk_dir / "open_jtalk_dic_utf_8-1.11").is_dir():
    print("下載日文辭典 open_jtalk…", flush=True)
    t = hf_hub_download(bundle, "open_jtalk_dic_utf_8-1.11.tar.gz", local_dir=str(repo / ".downloads"))
    with tarfile.open(t) as f:
        f.extractall(jtalk_dir)

shutil.rmtree(repo / ".downloads", ignore_errors=True)
print("預訓練資料就緒。", flush=True)
