# GPT-SoVITS（v2ProPlus）：官方原始碼 + PyTorch 2.7.1（CUDA 12.8，官方測試過的組合）
# 日文前端 pyopenjtalk 與 jieba_fast 只有原始碼套件，需要 CMake 與 MSVC 編譯器（scripts\setup.ps1 會裝）。
. "$PSScriptRoot\..\_common.ps1"
Initialize-Engine 'gpt-sovits'
Assert-Command 'cmake' '日文前端需要 CMake：winget install --id Kitware.CMake，裝完重新開 PowerShell'
$repo = Sync-Repo 'https://github.com/RVC-Boss/GPT-SoVITS.git' ''
New-EngineVenv '3.11'

Write-Step '安裝 PyTorch 2.7.1（CUDA 12.8）'
# 固定在 2.7.x：torchaudio 2.9 起的 load 改走 torchcodec，在 Windows 需要額外的 FFmpeg DLL。
Install-Packages 'torch==2.7.1' 'torchaudio==2.7.1' --index-url $TorchIndexCu128

Write-Step '安裝 GPT-SoVITS 相依套件'
# 官方第一行 --no-binary=opencc 會強迫從原始碼編譯 opencc；PyPI 有 Windows wheel，拿掉即可。
$filtered = Get-Content (Join-Path $repo 'requirements.txt') | Where-Object { $_ -notmatch '^\s*--no-binary' -and $_ -notmatch '^\s*torchaudio\s*$' }
$reqFile = Join-Path $EngineHome 'requirements.cantiva.txt'
[System.IO.File]::WriteAllLines($reqFile, $filtered, (New-Object System.Text.UTF8Encoding($false)))
Install-Packages -r $reqFile 'resampy' 'soundfile' 'huggingface_hub' 'transformers==4.51.3'

Write-Step '下載預訓練模型與語言資料（約 1.5 GB）'
Invoke-Checked $VenvPython (Join-Path $EngineSource 'fetch_assets.py') $repo

Invoke-SelfTest
Complete-Install @{ version = 'v2ProPlus'; models = 'lj1995/GPT-SoVITS（推論必要部分）' }
