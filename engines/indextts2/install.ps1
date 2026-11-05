# IndexTTS-2.5：官方原始碼，用 uv sync 依官方 uv.lock 安裝（PyTorch 2.8，CUDA 12.8）。
# 不安裝 deepspeed 與 flash-attn（Windows 上難以編譯，也不是必要的）。
. "$PSScriptRoot\..\_common.ps1"
Initialize-Engine 'indextts2'
$repo = Sync-Repo 'https://github.com/index-tts/index-tts.git' ''

Write-Step '依官方 uv.lock 安裝相依套件'
$env:UV_PROJECT_ENVIRONMENT = Join-Path $EngineHome '.venv'
Push-Location $repo
try {
    Invoke-Checked uv sync --frozen
} finally {
    Pop-Location
    Remove-Item Env:\UV_PROJECT_ENVIRONMENT -ErrorAction SilentlyContinue
}
Install-Packages 'huggingface_hub'

Save-HfModel 'IndexTeam/IndexTTS-2.5' 'IndexTTS-2.5' | Out-Null

# 第一次載入時還會下載幾個輔助小模型（w2v-bert、MaskGCT codec、CAMPPlus、BigVGAN）。
Invoke-SelfTest
Complete-Install @{ model = 'IndexTeam/IndexTTS-2.5' }
