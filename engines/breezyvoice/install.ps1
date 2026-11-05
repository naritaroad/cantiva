# BreezyVoice-300M：官方原始碼 + PyTorch 2.3.1（CUDA 11.8，官方固定版本）
# 官方 requirements 依賴 pynini（沒有 Windows 版）與 Linux 專用的 ttsfrd，
# 這裡把它們拿掉，改裝 wetext；adapter.py 會用 wetext 頂替原本的文字正規化模組。
. "$PSScriptRoot\..\_common.ps1"
Initialize-Engine 'breezyvoice'
$repo = Sync-Repo 'https://github.com/mtkresearch/BreezyVoice.git' ''
New-EngineVenv '3.10'

Write-Step '安裝 PyTorch 2.3.1（CUDA 11.8）'
Install-Packages 'torch==2.3.1' 'torchaudio==2.3.1' --index-url $TorchIndexCu118

Write-Step '安裝 BreezyVoice 相依套件（Windows 相容版）'
$skip = 'ttsfrd|WeTextProcessing|pynini|onnxruntime-gpu|^\s*torch(audio)?\s*=='
$filtered = Get-Content (Join-Path $repo 'requirements.txt') | Where-Object { $_ -notmatch $skip }
$reqFile = Join-Path $EngineHome 'requirements.cantiva.txt'
[System.IO.File]::WriteAllLines($reqFile, $filtered, (New-Object System.Text.UTF8Encoding($false)))
Install-Packages -r $reqFile 'wetext==0.1.8' 'soundfile' 'huggingface_hub' --index-strategy unsafe-best-match

Save-HfModel 'MediaTek-Research/BreezyVoice-300M' 'BreezyVoice-300M' | Out-Null

# 第一次載入時 g2pW 會下載注音模型到原始碼資料夾的 G2PWModel\。
Invoke-SelfTest
Complete-Install @{ model = 'MediaTek-Research/BreezyVoice-300M'; patched = 'wetext 取代 WeTextProcessing' }
