# 用 winget 安裝語音引擎需要的工具（安裝版 Cantiva 用；從原始碼建置的話 setup.ps1 已經裝過）。
#
# 用法：
#   powershell -ExecutionPolicy Bypass -File scripts\install-tools.ps1              uv 與 Git
#   powershell -ExecutionPolicy Bypass -File scripts\install-tools.ps1 -GptSovits   再加 CMake 與 Visual Studio Build Tools（C++）
#   powershell -ExecutionPolicy Bypass -File scripts\install-tools.ps1 -Tunnel      再加 cloudflared（讓 ChatGPT 等雲端 agent 連線）
# 已經裝好的工具會略過。裝完請重新開啟 Cantiva，新的 PATH 才會生效。

param([switch]$GptSovits, [switch]$Tunnel, [switch]$Pause)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

function Update-SessionPath {
    $machine = [Environment]::GetEnvironmentVariable('Path', 'Machine')
    $user = [Environment]::GetEnvironmentVariable('Path', 'User')
    $env:Path = "$machine;$user"
}

function Install-WithWinget([string]$Id, [string]$Label, [string[]]$Extra = @()) {
    Write-Host ''
    Write-Host "==> 安裝 $Label（winget：$Id）" -ForegroundColor Cyan
    $wingetArgs = @('install', '--id', $Id, '-e', '--source', 'winget', '--accept-package-agreements', '--accept-source-agreements') + $Extra
    & winget @wingetArgs
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335189) {
        # -1978335189 = 已經是最新版
        throw "$Label 安裝失敗（代碼 $LASTEXITCODE）。請手動安裝後再試。"
    }
    Update-SessionPath
}

function Test-Command([string]$Name) { [bool](Get-Command $Name -ErrorAction SilentlyContinue) }

function Test-MsvcTools {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { return $false }
    return [bool](& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
}

try {
    if (-not (Test-Command 'winget')) { throw '找不到 winget。請從 Microsoft Store 更新「應用程式安裝程式」後再試。' }
    if (Test-Command 'uv') { Write-Host 'uv：已安裝' } else { Install-WithWinget 'astral-sh.uv' 'uv' }
    if (Test-Command 'git') { Write-Host 'Git：已安裝' } else { Install-WithWinget 'Git.Git' 'Git' }
    if ($GptSovits) {
        if (Test-Command 'cmake') { Write-Host 'CMake：已安裝' } else { Install-WithWinget 'Kitware.CMake' 'CMake' }
        if (Test-MsvcTools) { Write-Host 'Visual Studio Build Tools（C++）：已安裝' } else {
            Install-WithWinget 'Microsoft.VisualStudio.2022.BuildTools' 'Visual Studio Build Tools（C++）' @(
                '--override', '--wait --passive --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 --add Microsoft.VisualStudio.Component.Windows11SDK.22621'
            )
        }
    }
    if ($Tunnel) {
        if (Test-Command 'cloudflared') { Write-Host 'cloudflared：已安裝' } else { Install-WithWinget 'Cloudflare.cloudflared' 'cloudflared' }
    }
    Write-Host ''
    Write-Host '完成。請重新開啟 Cantiva。' -ForegroundColor Green
    if (-not $GptSovits) {
        Write-Host 'GPT-SoVITS 另外需要 CMake 與 C++ 編譯器：在 Cantiva 資料夾執行 scripts\install-tools.ps1 -GptSovits'
    }
} catch {
    Write-Host $_.Exception.Message -ForegroundColor Red
    if ($Pause) { Read-Host '按 Enter 關閉' | Out-Null }
    exit 1
}
if ($Pause) { Read-Host '按 Enter 關閉' | Out-Null }
