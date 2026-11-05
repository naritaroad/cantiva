# Cantiva 一鍵建置（Windows 10/11）
#
# 做的事：
#   1. 用 winget 補齊缺少的工具：Git、Rust、Visual Studio Build Tools（C++）、uv、CMake、cloudflared
#   2. 編譯 cantiva.exe 與 cantiva-mcp.exe（release）
#   3. 組成 dist\Cantiva 資料夾（含 engines 與 skill），並建立開始功能表捷徑
#   4. 專案資料夾還不是 git repo 時，用你自己的 git 身分建立第一個 commit
#
# 用法（在專案根目錄開 PowerShell）：
#   powershell -ExecutionPolicy Bypass -File scripts\setup.ps1
# 參數：
#   -SkipTools      不檢查或安裝工具
#   -SkipTunnel     不安裝 cloudflared（不打算讓 ChatGPT／Grok 雲端版使用時）
#   -NoShortcut     不建立開始功能表捷徑

param(
    [switch]$SkipTools,
    [switch]$SkipTunnel,
    [switch]$NoShortcut
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $Root

function Write-Step([string]$Message) { Write-Host ''; Write-Host "==> $Message" -ForegroundColor Cyan }

function Test-Command([string]$Name) { [bool](Get-Command $Name -ErrorAction SilentlyContinue) }

function Update-SessionPath {
    $machine = [Environment]::GetEnvironmentVariable('Path', 'Machine')
    $user = [Environment]::GetEnvironmentVariable('Path', 'User')
    $env:Path = "$machine;$user;$env:USERPROFILE\.cargo\bin"
}

function Install-WithWinget([string]$Id, [string]$Label, [string[]]$Extra = @()) {
    Write-Host "安裝 $Label（winget：$Id）…"
    $wingetArgs = @('install', '--id', $Id, '-e', '--source', 'winget', '--accept-package-agreements', '--accept-source-agreements') + $Extra
    & winget @wingetArgs
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335189) {
        # -1978335189 = 已經是最新版
        throw "$Label 安裝失敗（代碼 $LASTEXITCODE）。請手動安裝後重新執行。"
    }
    Update-SessionPath
}

function Test-MsvcTools {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { return $false }
    $found = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    return [bool]$found
}

if (-not $SkipTools) {
    Write-Step '檢查工具'
    if (-not (Test-Command 'winget')) { throw '找不到 winget。請從 Microsoft Store 更新「應用程式安裝程式」後再試。' }
    if (-not (Test-Command 'git')) { Install-WithWinget 'Git.Git' 'Git' }
    if (-not (Test-MsvcTools)) {
        # Rust（MSVC 版）、pyopenjtalk、jieba_fast 都需要 C++ 編譯器。
        Install-WithWinget 'Microsoft.VisualStudio.2022.BuildTools' 'Visual Studio Build Tools（C++）' @(
            '--override', '--wait --passive --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 --add Microsoft.VisualStudio.Component.Windows11SDK.22621'
        )
    }
    if (-not (Test-Command 'cargo')) {
        Install-WithWinget 'Rustlang.Rustup' 'Rust'
        & rustup default stable
    }
    if (-not (Test-Command 'uv')) { Install-WithWinget 'astral-sh.uv' 'uv' }
    if (-not (Test-Command 'cmake')) { Install-WithWinget 'Kitware.CMake' 'CMake（GPT-SoVITS 日文前端需要）' }
    if (-not $SkipTunnel -and -not (Test-Command 'cloudflared')) { Install-WithWinget 'Cloudflare.cloudflared' 'cloudflared（對外通道）' }
    Write-Host "rustc：$(rustc --version)"
    Write-Host "uv：$(uv --version)"
}

Write-Step '編譯 Cantiva（第一次約 5–10 分鐘）'
& cargo build --release -p cantiva -p cantiva-mcp
if ($LASTEXITCODE -ne 0) { throw 'cargo build 失敗，請把上面的錯誤訊息貼給我。' }

Write-Step '組成 dist\Cantiva'
$Dist = Join-Path $Root 'dist\Cantiva'
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
Copy-Item (Join-Path $Root 'target\release\cantiva.exe') $Dist -Force
Copy-Item (Join-Path $Root 'target\release\cantiva-mcp.exe') $Dist -Force
$EnginesOut = Join-Path $Dist 'engines'
if (Test-Path $EnginesOut) { Remove-Item $EnginesOut -Recurse -Force }
New-Item -ItemType Directory -Force -Path $EnginesOut | Out-Null
Get-ChildItem (Join-Path $Root 'engines') -Force | Where-Object { $_.Name -ne 'dev-sine' } | ForEach-Object {
    Copy-Item $_.FullName $EnginesOut -Recurse -Force
}
Get-ChildItem $EnginesOut -Recurse -Directory -Filter '__pycache__' | Remove-Item -Recurse -Force
Get-ChildItem $EnginesOut -Recurse -Directory -Filter 'tests' | Remove-Item -Recurse -Force
Copy-Item (Join-Path $Root 'skill') $Dist -Recurse -Force
Write-Host "輸出位置：$Dist"

if (-not $NoShortcut) {
    Write-Step '建立開始功能表捷徑'
    $lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'Cantiva.lnk'
    $shell = New-Object -ComObject WScript.Shell
    $s = $shell.CreateShortcut($lnk)
    $s.TargetPath = Join-Path $Dist 'cantiva.exe'
    $s.WorkingDirectory = $Dist
    $s.Description = 'Cantiva 本機語音工作室'
    $s.Save()
    Write-Host "捷徑：$lnk"
}

if (-not (Test-Path (Join-Path $Root '.git'))) {
    Write-Step '建立 git repo（使用你自己的 git 身分）'
    $name = (git config --global user.name)
    $email = (git config --global user.email)
    if ($name -and $email) {
        git init -q
        git add -A
        git commit -q -m 'Cantiva 0.1.0：本機語音工作室'
        Write-Host "已用 $name <$email> 建立第一個 commit。"
    } else {
        Write-Host '略過：還沒設定 git config --global user.name / user.email。' -ForegroundColor Yellow
    }
}

Write-Host ''
Write-Host '完成。下一步：' -ForegroundColor Green
Write-Host "  1. 開啟 Cantiva（開始功能表，或 $Dist\cantiva.exe）"
Write-Host '  2. 到「引擎」頁按「安裝」（建議先裝 Qwen3-TTS 與 GPT-SoVITS）'
Write-Host '  3. 讓 AI agent 使用：powershell -ExecutionPolicy Bypass -File scripts\register-mcp.ps1'
