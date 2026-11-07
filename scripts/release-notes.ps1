# 產生 GitHub Release 的說明：下載檔案說明 + CHANGELOG.md 裡這個版本的段落。
# release.yml 會呼叫；本機也可以先看看：
#   powershell -ExecutionPolicy Bypass -File scripts\release-notes.ps1 -Version 0.2.0 -OutFile release-notes.md

param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$OutFile = 'release-notes.md'
)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

$lines = [System.IO.File]::ReadAllLines((Join-Path $Root 'CHANGELOG.md'), [System.Text.Encoding]::UTF8)
$start = -1
for ($i = 0; $i -lt $lines.Length; $i++) {
    if ($lines[$i].StartsWith("## [$Version]")) { $start = $i; break }
}
if ($start -lt 0) { throw "CHANGELOG.md 沒有 $Version 的段落" }
$end = $lines.Length
for ($i = $start + 1; $i -lt $lines.Length; $i++) {
    if ($lines[$i] -match '^## \[|^\[[^\]]+\]: ') { $end = $i; break }
}
$changes = ($lines[($start + 1)..($end - 1)] -join "`n").Trim()

$v = $Version
$notes = @(
    '## Downloads'
    ''
    '| File | What it is |'
    '|---|---|'
    "| ``Cantiva-$v-windows-x64-setup.exe`` | Installer for Windows 10/11 (x64). Installs to your user folder, no administrator rights needed. |"
    "| ``Cantiva-$v-windows-x64-portable.zip`` | Same files without an installer. Unzip anywhere and run ``cantiva.exe``. |"
    "| ``cantiva-voice-skill-$v.zip`` | The ``cantiva-voice`` agent skill. Unzip into ``~/.claude/skills`` or ``~/.agents/skills``, or upload it in Claude's Skills settings. |"
    '| `SHA256SUMS.txt` | SHA-256 checksums of the files above. |'
    ''
    'The binaries are not code-signed yet, so Windows SmartScreen may warn the first time you run the installer (More info, then Run anyway).'
    'Voice engines are installed from the 引擎 (Engines) page after setup. They need `uv` and `Git`, which the installer can install with winget.'
    ''
    $changes
    ''
) -join "`n"

$out = if ([System.IO.Path]::IsPathRooted($OutFile)) { $OutFile } else { Join-Path (Get-Location) $OutFile }
[System.IO.File]::WriteAllText($out, $notes, (New-Object System.Text.UTF8Encoding($false)))
Write-Host "已寫入 $out"
