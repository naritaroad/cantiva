# 各引擎 install.ps1 共用的函式。相容 Windows PowerShell 5.1 與 PowerShell 7。
# 用法：在 install.ps1 開頭 `. "$PSScriptRoot\..\_common.ps1"`

$ErrorActionPreference = 'Stop'
# dot-source 時 $PSScriptRoot 是 engines 資料夾；先記下來，函式裡才不會拿錯路徑。
$CantivaEnginesRoot = $PSScriptRoot
Set-StrictMode -Version 3.0
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$env:PYTHONUTF8 = '1'
$env:PYTHONIOENCODING = 'utf-8'

function Get-CantivaHome {
    if ($env:CANTIVA_HOME) { return $env:CANTIVA_HOME }
    # 與 Rust 端 cantiva_core::paths::default_root 相同。不用 %APPDATA%：從 MSIX 套件程式
    # （例如 Claude 桌面版）啟動時，寫入 AppData 會被轉到套件的私有資料夾，Cantiva 看不到。
    return (Join-Path $env:USERPROFILE 'Cantiva')
}

function Write-Step([string]$Message) {
    Write-Host ''
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Assert-Command([string]$Name, [string]$Hint) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "找不到 $Name。$Hint"
    }
}

function Invoke-Checked {
    # 執行外部程式，失敗就停下來（PowerShell 5.1 不會因外部程式失敗而丟例外）。
    # 刻意寫成簡單函式並用 $args：這樣 `--allow-existing` 這類參數會原封不動交給外部程式。
    # 輸出導向 Out-Host，避免混進呼叫端函式的回傳值。
    $file = $args[0]
    $rest = @($args | Select-Object -Skip 1)
    & $file @rest | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "$file $($rest -join ' ') 失敗（代碼 $LASTEXITCODE）"
    }
}

function Initialize-Engine([string]$Id) {
    Assert-Command 'uv' '請先執行 scripts\setup.ps1，或 winget install --id astral-sh.uv'
    $script:EngineId = $Id
    $script:EngineSource = (Resolve-Path (Join-Path $CantivaEnginesRoot $Id)).Path
    $script:WorkerPath = (Resolve-Path (Join-Path $CantivaEnginesRoot '_worker')).Path
    $script:EngineHome = Join-Path (Get-CantivaHome) "engines\$Id"
    $script:VenvPython = Join-Path $script:EngineHome '.venv\Scripts\python.exe'
    New-Item -ItemType Directory -Force -Path $script:EngineHome, (Join-Path $script:EngineHome 'models') | Out-Null
    # 權重和快取都放在引擎資料夾，不影響使用者其他專案。
    $env:HF_HOME = Join-Path $script:EngineHome 'hf'
    $env:MODELSCOPE_CACHE = Join-Path $script:EngineHome 'modelscope'
    $env:UV_LINK_MODE = 'copy'
    Remove-Item (Join-Path $script:EngineHome 'installed.json') -ErrorAction SilentlyContinue
    Write-Host "引擎：$Id"
    Write-Host "安裝位置：$($script:EngineHome)"
}

function New-EngineVenv([string]$PythonVersion) {
    Write-Step "建立 Python $PythonVersion 虛擬環境"
    Invoke-Checked uv venv --allow-existing --python $PythonVersion (Join-Path $script:EngineHome '.venv')
}

function Install-Packages {
    $rest = @($args)
    Invoke-Checked uv pip install --python $script:VenvPython @rest
}

function Sync-Repo([string]$Url, [string]$Ref, [switch]$Recursive) {
    Assert-Command 'git' '請先安裝 Git：winget install --id Git.Git'
    $repo = Join-Path $script:EngineHome 'repo'
    if (-not (Test-Path (Join-Path $repo '.git'))) {
        Write-Step "下載原始碼 $Url"
        if ($Recursive) { Invoke-Checked git clone --recursive $Url $repo } else { Invoke-Checked git clone $Url $repo }
    } else {
        Write-Step '原始碼已存在，略過下載'
    }
    if ($Ref) {
        Invoke-Checked git -C $repo fetch --tags origin
        Invoke-Checked git -C $repo checkout $Ref
        if ($Recursive) { Invoke-Checked git -C $repo submodule update --init --recursive }
    }
    return $repo
}

function Get-RepoCommit {
    $repo = Join-Path $script:EngineHome 'repo'
    if (Test-Path (Join-Path $repo '.git')) { return (git -C $repo rev-parse HEAD).Trim() }
    return $null
}

function Save-HfModel([string]$RepoId, [string]$FolderName) {
    $dest = Join-Path $script:EngineHome "models\$FolderName"
    Write-Step "下載模型 $RepoId（第一次會比較久）"
    # 參數透過 sys.argv 傳入，路徑裡有單引號也不會壞。
    $code = 'import sys; from huggingface_hub import snapshot_download; snapshot_download(repo_id=sys.argv[1], local_dir=sys.argv[2])'
    Invoke-Checked $script:VenvPython -c $code $RepoId $dest
    return $dest
}

function Invoke-SelfTest {
    Write-Step '自我測試：載入模型並試合成'
    $env:PYTHONPATH = $script:WorkerPath
    Push-Location $script:EngineHome
    try {
        Invoke-Checked $script:VenvPython -m cantiva_worker --adapter (Join-Path $script:EngineSource 'adapter.py') --engine-home $script:EngineHome --selftest
    } finally {
        Pop-Location
    }
}

function Complete-Install([hashtable]$Details) {
    $torch = & $script:VenvPython -c "import torch; print(torch.__version__, torch.version.cuda, torch.cuda.is_available())"
    $info = [ordered]@{
        engine       = $script:EngineId
        installed_at = (Get-Date).ToString('o')
        torch        = "$torch".Trim()
        repo_commit  = Get-RepoCommit
    }
    foreach ($k in $Details.Keys) { $info[$k] = $Details[$k] }
    $json = $info | ConvertTo-Json -Depth 4
    # 不用 Set-Content -Encoding UTF8，因為 5.1 會加 BOM。
    [System.IO.File]::WriteAllText((Join-Path $script:EngineHome 'installed.json'), $json, (New-Object System.Text.UTF8Encoding($false)))
    Write-Host ''
    Write-Host "完成：$($script:EngineId) 已可在 Cantiva 使用。" -ForegroundColor Green
}

# PyTorch 與 CUDA 版本集中在這裡，方便日後一起升級（2026-11-05 查證）。
$TorchIndexCu128 = 'https://download.pytorch.org/whl/cu128'
$TorchIndexCu121 = 'https://download.pytorch.org/whl/cu121'
$TorchIndexCu118 = 'https://download.pytorch.org/whl/cu118'
