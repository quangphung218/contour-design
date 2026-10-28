# Smoke test of the Windows installer on a clean Windows (a GitHub runner or
# a throwaway VM, as an administrator): installs the .msi silently, checks
# the files and the Start Menu shortcut, opens Contour from that shortcut,
# waits for the in-process daemon's /api/health to report the expected
# version, checks that an agent CLI installed the way npm installs one (a
# `.cmd` shim in a folder on the user's Path, which the app's own
# environment does not have yet) is found and runs, that Check again finds
# one installed while the app runs, that Integrations installs the MCP
# server into both and the command it registers answers MCP over stdio,
# exports a PDF through Edge, saves a screenshot of the window, closes it
# and uninstalls. Logs go to the out dir.
# Usage: pwsh scripts/windows-smoke.ps1 <Contour_*.msi> [out-dir]
# EXPECTED_VERSION overrides the version read from Cargo.toml; MSI_VERSION
# is the installer's numeric version (scripts/msi-version.sh), checked when set.
param(
  [Parameter(Mandatory = $true)][string]$Msi,
  [string]$Out = "smoke-out"
)
$ErrorActionPreference = "Continue"
$here = $PSScriptRoot
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path
# Loaded machines start slowly; the daemon gets this long.
$timeout = if ($env:SMOKE_TIMEOUT) { [int]$env:SMOKE_TIMEOUT } else { 120 }
$expected = $env:EXPECTED_VERSION
if (-not $expected) {
  $inPackage = $false
  foreach ($line in Get-Content "$here/../Cargo.toml") {
    if ($line -match '^\[workspace\.package\]') { $inPackage = $true; continue }
    if ($line -match '^\[') { $inPackage = $false }
    if ($inPackage -and $line -match '^version\s*=\s*"([^"]+)"') { $expected = $Matches[1]; break }
  }
}
$data = Join-Path $env:APPDATA "contour"
$script:failures = 0
function Ok([string]$what) { Write-Host "ok   $what" }
function Fail([string]$what, [string]$detail = "") {
  Write-Host "FAIL $what"
  if ($detail) { $detail -split "`n" | ForEach-Object { Write-Host "     $_" } }
  $script:failures++
}
function Finish {
  Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $script:installDir -and $_.Path.StartsWith($script:installDir) } |
    Stop-Process -Force -ErrorAction SilentlyContinue
  if (Test-Path "$data/logs") { Copy-Item -Recurse -Force "$data/logs" "$Out/logs" }
  $name = Split-Path -Leaf $Msi
  if ($script:failures -eq 0) { Write-Host "windows smoke ($name): all checks passed"; exit 0 }
  Write-Host "windows smoke ($name): $($script:failures) failed"
  exit 1
}
function Stop-Smoke([string]$what, [string]$detail = "") { Fail $what $detail; Finish }
function Tail([string]$path, [int]$n = 20) { if (Test-Path $path) { (Get-Content $path -Tail $n) -join "`n" } }
# The uninstall entry Windows Installer writes for Contour.
function Get-UninstallEntry {
  Get-ItemProperty "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*" -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName -eq "Contour" }
}
function Find-Shortcut {
  @("$env:ProgramData\Microsoft\Windows\Start Menu\Programs", "$env:APPDATA\Microsoft\Windows\Start Menu\Programs") |
    Where-Object { Test-Path $_ } |
    ForEach-Object { Get-ChildItem -Path $_ -Recurse -Filter "Contour.lnk" -ErrorAction SilentlyContinue } |
    Select-Object -First 1
}

if (-not (Test-Path $Msi -PathType Leaf)) { Stop-Smoke "no installer at $Msi" }
$Msi = (Resolve-Path $Msi).Path
if (-not $expected) { Stop-Smoke "no expected version (Cargo.toml or EXPECTED_VERSION)" }
$os = Get-CimInstance Win32_OperatingSystem
Write-Host "$($os.Caption) $($os.Version), installer $(Split-Path -Leaf $Msi), expecting $expected"
if (Test-Path $data) { Stop-Smoke "$data already exists: not a clean machine" }
$webview = Get-ItemProperty "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" -ErrorAction SilentlyContinue
Write-Host "note WebView2 runtime before install: $(if ($webview) { $webview.pv } else { 'none' })"

# Install, as a user does from a script or Intune: silent, with a log.
$install = Start-Process msiexec.exe -Wait -PassThru -ArgumentList @("/i", "`"$Msi`"", "/qn", "/norestart", "/l*v", "`"$Out\msi-install.log`"")
if ($install.ExitCode -eq 0) { Ok "msiexec /i /qn" } else { Stop-Smoke "msiexec /i exited $($install.ExitCode)" (Tail "$Out\msi-install.log") }
$entry = Get-UninstallEntry
if ($entry) {
  Ok "uninstall entry '$($entry.DisplayName)' $($entry.DisplayVersion) by $($entry.Publisher)"
  if ($env:MSI_VERSION) {
    if ($entry.DisplayVersion -eq $env:MSI_VERSION) { Ok "installer version $($entry.DisplayVersion)" }
    else { Fail "installer version is '$($entry.DisplayVersion)', want $env:MSI_VERSION" }
  }
} else { Fail "no uninstall entry named Contour" }

# The Start Menu shortcut, and the app with its files where it points.
$lnk = Find-Shortcut
if (-not $lnk) { Stop-Smoke "no Contour.lnk in the Start Menu" }
Ok "Start Menu shortcut $($lnk.FullName)"
$app = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk.FullName).TargetPath
if (-not (Test-Path $app -PathType Leaf)) { Stop-Smoke "the shortcut points at '$app', which does not exist" }
$script:installDir = Split-Path -Parent $app
Ok "the shortcut starts $app"
if ($script:installDir.StartsWith($env:ProgramFiles)) { Ok "installed under $env:ProgramFiles" } else { Fail "installed in $($script:installDir), not under Program Files" }
foreach ($file in @("ui\index.html", "content\skills", "LICENSE", "NOTICE", "THIRD-PARTY-LICENSES")) {
  if (Test-Path (Join-Path $script:installDir $file)) { Ok "ships $file" } else { Fail "no $file next to the app" (Get-ChildItem $script:installDir | Out-String) }
}

# An agent CLI the way `npm install -g` leaves one on Windows: a `sh` script
# and a `.cmd` shim side by side in a folder the installer adds to the user's
# Path in the registry. The app is started below with this session's
# environment, which does not have the folder: it must read the registry.
# `claude mcp …` (the MCP install below) only records its arguments.
$agentDir = Join-Path $env:LOCALAPPDATA "smoke-npm"
New-Item -ItemType Directory -Force -Path $agentDir | Out-Null
Copy-Item "$here\..\crates\od-ui\e2e\mock-agent\claude" "$agentDir\claude"
$bash = Join-Path $env:ProgramFiles "Git\bin\bash.exe"
if (-not (Test-Path $bash)) { Stop-Smoke "no Git Bash at $bash for the mock agent" }
Set-Content -Path "$agentDir\claude.cmd" -Encoding ascii -Value @(
  "@echo off",
  "if not `"%~1`"==`"mcp`" goto run",
  ">>`"%~dp0mcp-args.log`" echo %*",
  "exit /b 0",
  ":run",
  "`"$bash`" `"%~dp0claude`" %*"
)
if (Get-Command claude -ErrorAction SilentlyContinue) { Stop-Smoke "a claude is already on this machine's PATH" (Get-Command claude).Source }
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
[Environment]::SetEnvironmentVariable("Path", (@($userPath, $agentDir) | Where-Object { $_ }) -join ";", "User")
$env:Path -split ";" | Where-Object { $_ -eq $agentDir } | ForEach-Object { Stop-Smoke "the agent folder is already on this session's Path" }

# Open it as the Start Menu does: the shortcut, through the shell.
Start-Process -FilePath $lnk.FullName
$url = ""
for ($i = 0; $i -lt $timeout; $i++) {
  $url = (Get-Content "$data\daemon.lock" -Raw -ErrorAction SilentlyContinue)
  if ($url) {
    $url = $url.Trim()
    try { Invoke-RestMethod "$url/api/health" -TimeoutSec 5 -OutFile "$Out\health.json"; break } catch { }
  }
  $url = ""
  if ($i -gt 10 -and -not (Get-Process | Where-Object { $_.Path -eq $app })) { Stop-Smoke "the app exited during startup" (Get-ChildItem "$data\logs" -ErrorAction SilentlyContinue | Get-Content -Tail 20 | Out-String) }
  Start-Sleep -Seconds 1
}
if (-not $url) { Stop-Smoke "no /api/health within ${timeout}s" (Get-ChildItem "$data\logs" -ErrorAction SilentlyContinue | Get-Content -Tail 20 | Out-String) }
$got = (Get-Content "$Out\health.json" -Raw | ConvertFrom-Json).version
if ($got -eq $expected) { Ok "/api/health at $url reports $got" } else { Fail "/api/health reports '$got', want $expected" }
Ok "data folder $data"

# The window shows what / serves: the UI, not "UI bundle not found".
$index = (Invoke-WebRequest "$url/" -UseBasicParsing).Content
Set-Content -Path "$Out\index.html" -Value $index
if ($index -match "<title>Contour</title>") { Ok "the window's page is the UI bundle" } else { Fail "GET / is not the UI bundle" ($index.Substring(0, [Math]::Min(300, $index.Length))) }

$agents = Invoke-RestMethod "$url/api/agents"
$agents | ConvertTo-Json -Depth 6 | Set-Content "$Out\agents.json"
$claude = $agents.agents | Where-Object { $_.id -eq "claude" }
if ($claude.available -and $claude.path -eq "$agentDir\claude.cmd") { Ok "finds the npm-style agent through the user's Path ($($claude.path))" }
else { Fail "claude.cmd not found through the user's Path" ($claude | ConvertTo-Json -Compress) }

# One turn with the agent, as the first design after onboarding.
$project = Invoke-RestMethod -Method Post -ContentType "application/json" -Body '{"name":"smoke"}' "$url/api/projects"
$runBody = @{ agentId = "claude"; projectId = $project.project.id; conversationId = $project.conversationId; message = "A landing page" } | ConvertTo-Json -Compress
$run = Invoke-RestMethod -Method Post -ContentType "application/json" -Body $runBody "$url/api/runs"
$status = "running"
for ($i = 0; $i -lt 60; $i++) {
  $status = (Invoke-RestMethod "$url/api/runs/$($run.runId)").run.status
  if ($status -ne "running") { break }
  Start-Sleep -Seconds 1
}
$page = Join-Path $data "projects\$($project.project.id)\index.html"
if ($status -eq "succeeded" -and (Test-Path $page) -and (Get-Item $page).Length -gt 0) { Ok "an agent run wrote index.html" }
else {
  $events = try { (Invoke-WebRequest "$url/api/runs/$($run.runId)" -UseBasicParsing).Content } catch { "$_" }
  Fail "agent run $($run.runId) ended '$status'" $events
}

# An agent installed while the app runs: its folder joins the user's Path
# in the registry only, and Check again (POST /api/agents/rescan) finds it.
$laterDir = Join-Path $env:LOCALAPPDATA "smoke-later"
New-Item -ItemType Directory -Force -Path $laterDir | Out-Null
Set-Content -Path "$laterDir\codex.cmd" -Encoding ascii -Value @(
  "@echo off",
  "if `"%~1`"==`"mcp`" >>`"%~dp0mcp-args.log`" echo %*",
  "echo codex-cli 0.0.0-smoke"
)
if (Get-Command codex -ErrorAction SilentlyContinue) { Fail "a codex is already on this machine's PATH" (Get-Command codex).Source }
$before = (Invoke-RestMethod "$url/api/agents").agents | Where-Object { $_.id -eq "codex" }
if ($before.available) { Fail "codex is found before it is installed" ($before | ConvertTo-Json -Compress) }
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
[Environment]::SetEnvironmentVariable("Path", "$userPath;$laterDir", "User")
$rescan = Invoke-RestMethod -Method Post "$url/api/agents/rescan"
$codex = $rescan.agents | Where-Object { $_.id -eq "codex" }
if ($codex.available -and $codex.path -eq "$laterDir\codex.cmd") { Ok "Check again finds an agent installed while the app runs ($($codex.path))" }
else { Fail "Check again does not find codex.cmd added to the user's Path" ($codex | ConvertTo-Json -Compress) }

# Integrations > Connect Contour to your coding agent: Install runs
# `<agent> mcp add … -- <this app> mcp` through the same `.cmd` shims; no
# `--daemon-url`, as the app's port changes per launch.
$registered = $null
foreach ($mcp in @(@{ id = "claude"; dir = $agentDir; add = "mcp add --scope user contour -- " }, @{ id = "codex"; dir = $laterDir; add = "mcp add contour -- " })) {
  $res = try { Invoke-RestMethod -Method Post "$url/api/mcp/install/$($mcp.id)" } catch { [pscustomobject]@{ ok = $false; message = "$_" } }
  $log = Get-Content "$($mcp.dir)\mcp-args.log" -ErrorAction SilentlyContinue
  $line = $log | Where-Object { $_.StartsWith($mcp.add) } | Select-Object -First 1
  # The command after `--`, unquoted, then its arguments.
  $server = ""; $rest = ""
  if ($line -and $line.Substring($mcp.add.Length) -match '^"?(.+?\.exe)"? (.*)$') { $server = $Matches[1]; $rest = $Matches[2] }
  if ($res.ok -and $server -eq $app -and $rest -eq "mcp") {
    Ok "Integrations installs the MCP server into $($mcp.id): $($mcp.id) $line"
    $registered = @{ exe = $server; args = $rest }
  }
  else { Fail "installing the MCP server into $($mcp.id) (ok=$($res.ok), server '$server' '$rest', want $app mcp)" "$($res.message)`n$($log -join "`n")" }
}

# The registered command, as the agent starts it: a GUI-subsystem binary
# with pipes for stdin and stdout. It answers initialize and tools/list,
# and list_projects reaches this app's daemon through daemon.lock (no
# window, no second daemon).
if ($registered) {
  $psi = New-Object System.Diagnostics.ProcessStartInfo $registered.exe, $registered.args
  $psi.UseShellExecute = $false; $psi.RedirectStandardInput = $true; $psi.RedirectStandardOutput = $true
  $mcpProc = [System.Diagnostics.Process]::Start($psi)
  @(
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"1"}}}',
    '{"jsonrpc":"2.0","method":"notifications/initialized"}',
    '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}',
    '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_projects","arguments":{}}}'
  ) | ForEach-Object { $mcpProc.StandardInput.WriteLine($_) }
  $mcpProc.StandardInput.Flush()
  $replies = @{}; $mcpLines = @()
  $deadline = (Get-Date).AddSeconds(60)
  while ($replies.Count -lt 3 -and (Get-Date) -lt $deadline) {
    $read = $mcpProc.StandardOutput.ReadLineAsync()
    if (-not $read.Wait([int][Math]::Max(1, ($deadline - (Get-Date)).TotalMilliseconds)) -or $null -eq $read.Result) { break }
    $mcpLines += $read.Result
    $msg = try { $read.Result | ConvertFrom-Json } catch { $null }
    if ($msg -and $null -ne $msg.id) { $replies[[int]$msg.id] = $msg }
  }
  $mcpProc.StandardInput.Close()
  $exited = $mcpProc.WaitForExit(20000)
  if (-not $exited) { $mcpProc.Kill() }
  $mcpLines | Set-Content "$Out\mcp-stdio.log"
  $tools = @($replies[2].result.tools | ForEach-Object { $_.name })
  $listed = "$($replies[3].result.content | ForEach-Object { $_.text })"
  if ($replies[1].result.serverInfo.name -eq "contour" -and $tools -contains "list_projects") { Ok "the registered command answers initialize and tools/list ($($tools.Count) tools)" }
  else { Fail "the registered command '$($registered.exe) $($registered.args)' is not an MCP server" ($mcpLines -join "`n") }
  if (-not $replies[3].result.isError -and $listed.Contains($project.project.id)) { Ok "its list_projects reaches this app's daemon" }
  else { Fail "list_projects through the registered command" ($mcpLines -join "`n") }
  if ($exited) { Ok "it exits when stdin closes" } else { Fail "the MCP server still runs 20 s after stdin closed" }
}

# PDF export drives the Microsoft Edge that Windows ships.
try {
  Invoke-WebRequest -Method Post -ContentType "application/json" -Body '{"fileName":"index.html"}' -OutFile "$Out\export.pdf" `
    "$url/api/projects/$($project.project.id)/export/pdf" -UseBasicParsing -TimeoutSec 120
  $magic = [System.Text.Encoding]::ASCII.GetString([System.IO.File]::ReadAllBytes("$Out\export.pdf")[0..3])
  if ($magic -eq "%PDF") { Ok "PDF export through Edge" } else { Fail "PDF export is not a PDF" (Get-Content "$Out\export.pdf" -Raw -ErrorAction SilentlyContinue) }
} catch { Fail "PDF export" "$_" }

# The window: titled Contour, and not blank once the UI has loaded.
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class SmokeWin {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
"@
[SmokeWin]::SetProcessDPIAware() | Out-Null
$proc = $null
for ($i = 0; $i -lt 30; $i++) {
  $proc = Get-Process | Where-Object { $_.Path -eq $app -and $_.MainWindowTitle -eq "Contour" } | Select-Object -First 1
  if ($proc) { break }
  Start-Sleep -Seconds 1
}
if ($proc) { Ok "a window titled Contour is open" } else { Fail "no window titled Contour" (Get-Process | Where-Object { $_.Path -eq $app } | Format-Table Id, MainWindowTitle | Out-String) }
if ($proc) {
  [SmokeWin]::SetForegroundWindow($proc.MainWindowHandle) | Out-Null
  $settle = if ($env:SMOKE_SETTLE) { [int]$env:SMOKE_SETTLE } else { 8 }
  Start-Sleep -Seconds $settle
  $r = New-Object SmokeWin+RECT
  [SmokeWin]::GetWindowRect($proc.MainWindowHandle, [ref]$r) | Out-Null
  $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
  $shot = Join-Path $Out "window-windows-$($os.BuildNumber).png"
  if ($w -gt 0 -and $h -gt 0) {
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
    $bmp.Save($shot, [System.Drawing.Imaging.ImageFormat]::Png)
    # Grey spread over a grid of samples: a blank (white or black) window has none.
    $values = for ($y = 0; $y -lt $h; $y += 8) { for ($x = 0; $x -lt $w; $x += 8) { $p = $bmp.GetPixel($x, $y); (0.299 * $p.R + 0.587 * $p.G + 0.114 * $p.B) / 255 } }
    $g.Dispose(); $bmp.Dispose()
    $mean = ($values | Measure-Object -Average).Average
    $spread = [Math]::Sqrt((($values | ForEach-Object { ($_ - $mean) * ($_ - $mean) }) | Measure-Object -Average).Average)
    if ($spread -gt 0.02) { Ok ("screenshot {0} ({1}x{2}) is not blank" -f (Split-Path -Leaf $shot), $w, $h) }
    else { Fail ("the window looks blank (grey spread {0:N4}); see {1}" -f $spread, (Split-Path -Leaf $shot)) }
  } else { Fail "the window has no size ($w x $h)" }

  # Closing the window quits the app (and its daemon).
  $proc.CloseMainWindow() | Out-Null
  if ($proc.WaitForExit(20000)) { Ok "closing the window quits the app" } else { Fail "the app is still running 20 s after its window closed" }
}
if (Get-ChildItem "$data\logs" -Recurse -File -ErrorAction SilentlyContinue | Select-String -Pattern "panicked" -Quiet) {
  Fail "the log has a panic" (Get-ChildItem "$data\logs" -Recurse -File | Select-String -Pattern "panicked" -Context 0, 3 | Select-Object -First 2 | Out-String)
}
Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($script:installDir) } | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

# Uninstall: the files, the shortcut and the entry go; the data stays.
$remove = Start-Process msiexec.exe -Wait -PassThru -ArgumentList @("/x", "`"$Msi`"", "/qn", "/norestart", "/l*v", "`"$Out\msi-uninstall.log`"")
if ($remove.ExitCode -eq 0) { Ok "msiexec /x /qn" } else { Fail "msiexec /x exited $($remove.ExitCode)" (Tail "$Out\msi-uninstall.log") }
if (Test-Path $app) { Fail "$app is still there after uninstalling" } else { Ok "uninstalling removes the app" }
if (Find-Shortcut) { Fail "the Start Menu shortcut is still there after uninstalling" } else { Ok "uninstalling removes the Start Menu shortcut" }
if (Get-UninstallEntry) { Fail "the uninstall entry is still there" } else { Ok "uninstalling removes the uninstall entry" }
if (Test-Path "$data\app.sqlite") { Ok "the user's data stays in $data" } else { Write-Host "note no store file in $data after uninstalling" }
Finish
