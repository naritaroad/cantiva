# 安裝單一引擎（與 Cantiva「引擎」頁的「安裝」按鈕相同）。
# 用法：powershell -ExecutionPolicy Bypass -File scripts\install-engine.ps1 qwen3-tts
# 可用的 id：qwen3-tts、gpt-sovits、cosyvoice3、indextts2、breezyvoice

param([Parameter(Mandatory = $true)][string]$Id)

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$script = Join-Path $Root "engines\$Id\install.ps1"
if (-not (Test-Path $script)) {
    $ids = (Get-ChildItem (Join-Path $Root 'engines') -Directory | Where-Object { Test-Path (Join-Path $_.FullName 'install.ps1') }).Name -join '、'
    throw "沒有引擎「$Id」。可用的 id：$ids"
}
& $script
