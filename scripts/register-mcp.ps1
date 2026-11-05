# 把 cantiva-mcp 註冊到本機的 AI agent，並安裝配套 skill。
#
# 用法：
#   powershell -ExecutionPolicy Bypass -File scripts\register-mcp.ps1              只印出設定，不修改任何檔案
#   powershell -ExecutionPolicy Bypass -File scripts\register-mcp.ps1 -Claude -Codex -Cursor -Grok -Skill
#
# -Claude  用 claude mcp add 註冊（Claude Code，--scope user）
# -Codex   在 %USERPROFILE%\.codex\config.toml 加上 [mcp_servers.cantiva]（已存在就略過）
# -Cursor  在 %USERPROFILE%\.cursor\mcp.json 加上 cantiva（會先備份成 mcp.json.bak）
# -Grok    用 grok mcp add 註冊（Grok Build CLI）
# -Skill   把 skill\cantiva-voice 複製到 %USERPROFILE%\.claude\skills 與 %USERPROFILE%\.agents\skills
# Claude Desktop 請照 cantiva-mcp --print-config 的輸出手動加入，改完要重新開啟 Claude Desktop。

param([switch]$Claude, [switch]$Codex, [switch]$Cursor, [switch]$Grok, [switch]$Skill)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$Dist = Join-Path $Root 'dist\Cantiva'
$Mcp = Join-Path $Dist 'cantiva-mcp.exe'
if (-not (Test-Path $Mcp)) { throw "找不到 $Mcp，請先執行 scripts\setup.ps1" }

if (-not ($Claude -or $Codex -or $Cursor -or $Grok -or $Skill)) {
    & $Mcp --print-config
    Write-Host ''
    Write-Host '加上 -Claude -Codex -Cursor -Grok -Skill 其中幾個參數，就會直接幫你註冊。'
    exit 0
}

if ($Claude) {
    if (Get-Command claude -ErrorAction SilentlyContinue) {
        & claude mcp add --scope user cantiva -- $Mcp
        Write-Host 'Claude Code：已註冊。'
    } else { Write-Host 'Claude Code：找不到 claude 指令，略過。' -ForegroundColor Yellow }
}

if ($Codex) {
    $cfg = Join-Path $env:USERPROFILE '.codex\config.toml'
    New-Item -ItemType Directory -Force -Path (Split-Path $cfg) | Out-Null
    $text = if (Test-Path $cfg) { [System.IO.File]::ReadAllText($cfg) } else { '' }
    if ($text -match '(?m)^\[mcp_servers\.cantiva\]') {
        Write-Host 'Codex：config.toml 已經有 [mcp_servers.cantiva]，略過。'
    } else {
        # TOML 基本字串：跳脫反斜線與雙引號，路徑裡有單引號也沒問題。
        $tomlPath = $Mcp.Replace('\', '\\').Replace('"', '\"')
        $block = "`n[mcp_servers.cantiva]`ncommand = `"$tomlPath`"`nstartup_timeout_sec = 30`ntool_timeout_sec = 1800`n"
        [System.IO.File]::AppendAllText($cfg, $block, (New-Object System.Text.UTF8Encoding($false)))
        Write-Host "Codex：已加入 $cfg"
    }
}

if ($Cursor) {
    $cfg = Join-Path $env:USERPROFILE '.cursor\mcp.json'
    New-Item -ItemType Directory -Force -Path (Split-Path $cfg) | Out-Null
    if (Test-Path $cfg) {
        Copy-Item $cfg "$cfg.bak" -Force
        $json = Get-Content $cfg -Raw -Encoding UTF8 | ConvertFrom-Json
    } else {
        $json = [pscustomobject]@{}
    }
    if (-not ($json.PSObject.Properties.Name -contains 'mcpServers')) {
        $json | Add-Member -NotePropertyName mcpServers -NotePropertyValue ([pscustomobject]@{})
    }
    $entry = [pscustomobject]@{ type = 'stdio'; command = $Mcp; args = @() }
    if ($json.mcpServers.PSObject.Properties.Name -contains 'cantiva') {
        $json.mcpServers.cantiva = $entry
    } else {
        $json.mcpServers | Add-Member -NotePropertyName cantiva -NotePropertyValue $entry
    }
    [System.IO.File]::WriteAllText($cfg, ($json | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding($false)))
    Write-Host "Cursor：已寫入 $cfg（備份在 mcp.json.bak）"
}

if ($Grok) {
    if (Get-Command grok -ErrorAction SilentlyContinue) {
        & grok mcp add cantiva -- $Mcp
        Write-Host 'Grok Build CLI：已註冊。'
    } else { Write-Host 'Grok：找不到 grok 指令，略過。' -ForegroundColor Yellow }
}

if ($Skill) {
    $src = Join-Path $Dist 'skill\cantiva-voice'
    foreach ($base in @("$env:USERPROFILE\.claude\skills", "$env:USERPROFILE\.agents\skills")) {
        $dst = Join-Path $base 'cantiva-voice'
        New-Item -ItemType Directory -Force -Path $base | Out-Null
        Copy-Item $src $base -Recurse -Force
        Write-Host "skill：已複製到 $dst"
    }
}
