# Qwen3-TTS：qwen-tts 0.1.1 + PyTorch 2.8.0（CUDA 12.8）
# 下載約 6 GB：1.7B CustomVoice（內建說話人）與 0.6B Base（複製聲音）。
. "$PSScriptRoot\..\_common.ps1"
Initialize-Engine 'qwen3-tts'
New-EngineVenv '3.12'

Write-Step '安裝 PyTorch 2.8.0（CUDA 12.8）'
Install-Packages 'torch==2.8.0' 'torchaudio==2.8.0' --index-url $TorchIndexCu128

Write-Step '安裝 qwen-tts 0.1.1'
Install-Packages 'qwen-tts==0.1.1' 'soundfile' 'huggingface_hub'

Save-HfModel 'Qwen/Qwen3-TTS-12Hz-1.7B-CustomVoice' 'Qwen3-TTS-12Hz-1.7B-CustomVoice' | Out-Null
Save-HfModel 'Qwen/Qwen3-TTS-12Hz-0.6B-Base' 'Qwen3-TTS-12Hz-0.6B-Base' | Out-Null

Invoke-SelfTest
Complete-Install @{ package = 'qwen-tts==0.1.1'; models = @('Qwen3-TTS-12Hz-1.7B-CustomVoice', 'Qwen3-TTS-12Hz-0.6B-Base') }
