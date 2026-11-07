# 建置 GitHub Release 要上傳的檔案。
#
# 用法（在專案根目錄開 PowerShell）：
#   powershell -ExecutionPolicy Bypass -File scripts\package.ps1
# 參數：
#   -OutDir <資料夾>   輸出位置，預設 target\release-assets
#   -SkipBuild         不重新編譯，直接用上次的 release 輸出
#   -NoInstaller       不產生安裝程式（沒有 Inno Setup 時）
#
# 產出（<版本> 取自 Cargo.toml 的 [workspace.package] version）：
#   Cantiva-<版本>-windows-x64-setup.exe      安裝程式（Inno Setup，不需要系統管理員權限）
#   Cantiva-<版本>-windows-x64-portable.zip   免安裝版，解壓縮後執行 cantiva.exe
#   cantiva-voice-skill-<版本>.zip            agent skill（解壓縮到 skills 資料夾，或上傳到 claude.ai）
#   SHA256SUMS.txt

param(
    [string]$OutDir = 'target\release-assets',
    [switch]$SkipBuild,
    [switch]$NoInstaller
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $Root
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem

function Write-Step([string]$Message) { Write-Host ''; Write-Host "==> $Message" -ForegroundColor Cyan }

$cargoToml = [System.IO.File]::ReadAllText((Join-Path $Root 'Cargo.toml'))
if ($cargoToml -notmatch '(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"') { throw 'Cargo.toml 找不到 [workspace.package] version' }
$Version = $Matches[1]
Write-Host "版本：$Version"

$Target = 'x86_64-pc-windows-msvc'
if (-not $SkipBuild) {
    Write-Step '編譯 release'
    # 靜態連結 C 執行階段，使用者不必另外裝 Visual C++ 可轉散發套件。
    # 只對 --target 指定的產物生效，build script 與 proc-macro 不受影響。
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
    & cargo build --release --locked --target $Target -p cantiva -p cantiva-mcp
    if ($LASTEXITCODE -ne 0) { throw 'cargo build 失敗' }
}
$Bin = Join-Path $Root "target\$Target\release"

Write-Step '組成安裝內容'
$Stage = Join-Path $Root 'target\package\Cantiva'
if (Test-Path $Stage) { Remove-Item $Stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path $Stage, (Join-Path $Stage 'engines'), (Join-Path $Stage 'scripts') | Out-Null
foreach ($exe in 'cantiva.exe', 'cantiva-mcp.exe') {
    Copy-Item (Join-Path $Bin $exe) $Stage
}
# dev-sine 是測試用的假引擎，不隨安裝版發佈。
Get-ChildItem (Join-Path $Root 'engines') -Force | Where-Object { $_.Name -ne 'dev-sine' } | ForEach-Object {
    Copy-Item $_.FullName (Join-Path $Stage 'engines') -Recurse -Force
}
Get-ChildItem (Join-Path $Stage 'engines') -Recurse -Directory | Where-Object { $_.Name -in '__pycache__', 'tests' } |
    Sort-Object { $_.FullName.Length } -Descending | Remove-Item -Recurse -Force
Copy-Item (Join-Path $Root 'skill') $Stage -Recurse -Force
foreach ($s in 'register-mcp.ps1', 'install-engine.ps1', 'install-tools.ps1') {
    Copy-Item (Join-Path $Root "scripts\$s") (Join-Path $Stage 'scripts')
}
foreach ($f in 'LICENSE', 'THIRD_PARTY_NOTICES.md') { Copy-Item (Join-Path $Root $f) $Stage }

$Out = Join-Path $Root $OutDir
if (Test-Path $Out) { Remove-Item $Out -Recurse -Force }
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path

function New-Zip([string]$SourceDir, [string]$ZipPath) {
    # 自己寫 zip 項目：Windows PowerShell 5.1 的 Compress-Archive 會用反斜線當路徑分隔，
    # 在 macOS、Linux 與 claude.ai 上解開會變成一個個帶反斜線的檔名。
    $base = Split-Path $SourceDir -Parent
    $zip = [System.IO.Compression.ZipFile]::Open($ZipPath, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        Get-ChildItem $SourceDir -Recurse -File | Sort-Object FullName | ForEach-Object {
            $name = $_.FullName.Substring($base.Length + 1).Replace('\', '/')
            [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $_.FullName, $name, [System.IO.Compression.CompressionLevel]::Optimal) | Out-Null
        }
    } finally {
        $zip.Dispose()
    }
}

Write-Step '壓縮免安裝版與 skill'
New-Zip $Stage (Join-Path $Out "Cantiva-$Version-windows-x64-portable.zip")
New-Zip (Join-Path $Root 'skill\cantiva-voice') (Join-Path $Out "cantiva-voice-skill-$Version.zip")

if (-not $NoInstaller) {
    Write-Step '建置安裝程式（Inno Setup）'
    $iscc = @(
        (Get-Command 'ISCC.exe' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
        (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe'),
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe')
    ) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
    if (-not $iscc) { throw '找不到 Inno Setup 6（ISCC.exe）。winget install --id JRSoftware.InnoSetup，或加上 -NoInstaller。' }
    & $iscc /Qp "/DAppVersion=$Version" "/DStageDir=$Stage" "/DOutputDir=$Out" (Join-Path $Root 'packaging\cantiva.iss')
    if ($LASTEXITCODE -ne 0) { throw "ISCC 失敗（代碼 $LASTEXITCODE）" }
}

Write-Step '計算 SHA-256'
$sums = Get-ChildItem $Out -File | Where-Object { $_.Name -ne 'SHA256SUMS.txt' } | Sort-Object Name | ForEach-Object {
    "$((Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant())  $($_.Name)"
}
[System.IO.File]::WriteAllText((Join-Path $Out 'SHA256SUMS.txt'), (($sums -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding($false)))

Write-Host ''
Get-ChildItem $Out -File | ForEach-Object { '{0,-48} {1,8:N1} MB' -f $_.Name, ($_.Length / 1MB) }
Write-Host "輸出位置：$Out" -ForegroundColor Green
