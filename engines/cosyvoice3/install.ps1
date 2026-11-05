# Fun-CosyVoice3-0.5B-2512：官方原始碼 + PyTorch 2.3.1（CUDA 12.1，官方固定版本）
# torchaudio 2.9 起改了 load/save，CosyVoice 會壞掉，所以不升級 torch。
. "$PSScriptRoot\..\_common.ps1"
Initialize-Engine 'cosyvoice3'
$repo = Sync-Repo 'https://github.com/FunAudioLLM/CosyVoice.git' '' -Recursive
New-EngineVenv '3.10'

Write-Step '安裝 CosyVoice 相依套件（含 PyTorch 2.3.1）'
Install-Packages -r (Join-Path $repo 'requirements.txt') --index-strategy unsafe-best-match
Install-Packages 'soundfile' 'huggingface_hub'

Save-HfModel 'FunAudioLLM/Fun-CosyVoice3-0.5B-2512' 'Fun-CosyVoice3-0.5B' | Out-Null

Invoke-SelfTest
Complete-Install @{ model = 'FunAudioLLM/Fun-CosyVoice3-0.5B-2512' }
