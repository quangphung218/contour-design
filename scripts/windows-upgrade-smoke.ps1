# Smoke test of an upgrade with the Windows installer, as a user who has the
# previous release and runs the new .msi: installs the previous one, opens
# it and makes a project, installs the new one over it, then checks there
# is one uninstall entry with the new version, the app starts and reports
# that version, and the data folder still has the project and a file left
# there before. Uninstalls at the end. Wants no Contour installed and no
# data folder (windows-smoke.yml moves the one windows-smoke.ps1 leaves
# aside): the previous release must not open data a newer one migrated.
# Logs go to the out dir.
# Usage: pwsh scripts/windows-upgrade-smoke.ps1 <previous.msi> <new.msi> [out-dir]
# EXPECTED_VERSION overrides the version read from Cargo.toml; MSI_VERSION
# is the new installer's numeric version (scripts/msi-version.sh), checked
# when set.
param(
  [Parameter(Mandatory = $true)][string]$Previous,
  [Parameter(Mandatory = $true)][string]$Msi,
  [string]$Out = "upgrade-out"
)
$ErrorActionPreference = "Continue"
$here = $PSScriptRoot
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path
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
$script:installDir = ""
function Ok([string]$what) { Write-Host "ok   $what" }
function Fail([string]$what, [string]$detail = "") {
  Write-Host "FAIL $what"
  if ($detail) { $detail -split "`n" | ForEach-Object { Write-Host "     $_" } }
  $script:failures++
}
function Stop-App {
  if (-not $script:installDir) { return }
  Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($script:installDir) } |
    Stop-Process -Force -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 2
}
function Finish {
  Stop-App
  $name = "$(Split-Path -Leaf $Previous) -> $(Split-Path -Leaf $Msi)"
  if ($script:failures -eq 0) { Write-Host "windows upgrade smoke ($name): all checks passed"; exit 0 }
  Write-Host "windows upgrade smoke ($name): $($script:failures) failed"
  exit 1
}
function Stop-Smoke([string]$what, [string]$detail = "") { Fail $what $detail; Finish }
function Tail([string]$path, [int]$n = 20) { if (Test-Path $path) { (Get-Content $path -Tail $n) -join "`n" } }
function Get-UninstallEntry {
  Get-ItemProperty "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*" -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName -eq "Contour" }
}
function Install([string]$msi, [string]$log) {
  $p = Start-Process msiexec.exe -Wait -PassThru -ArgumentList @("/i", "`"$msi`"", "/qn", "/norestart", "/l*v", "`"$Out\$log`"")
  $p.ExitCode
}
# Opens the app from its Start Menu shortcut and returns the daemon's URL
# once /api/health answers. The daemon.lock an earlier run left goes first:
# the app rewrites it once it listens, possibly on the same port as before.
function Start-App {
  $lnk = @("$env:ProgramData\Microsoft\Windows\Start Menu\Programs", "$env:APPDATA\Microsoft\Windows\Start Menu\Programs") |
    Where-Object { Test-Path $_ } |
    ForEach-Object { Get-ChildItem -Path $_ -Recurse -Filter "Contour.lnk" -ErrorAction SilentlyContinue } |
    Select-Object -First 1
  if (-not $lnk) { Stop-Smoke "no Contour.lnk in the Start Menu" }
  $app = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk.FullName).TargetPath
  $script:installDir = Split-Path -Parent $app
  Remove-Item "$data\daemon.lock" -ErrorAction SilentlyContinue
  Start-Process -FilePath $lnk.FullName
  for ($i = 0; $i -lt $timeout; $i++) {
    $url = Get-Content "$data\daemon.lock" -Raw -ErrorAction SilentlyContinue
    if ($url) {
      $url = $url.Trim()
      try { $health = Invoke-RestMethod "$url/api/health" -TimeoutSec 5; return @{ url = $url; version = $health.version } } catch { }
    }
    Start-Sleep -Seconds 1
  }
  Stop-Smoke "no /api/health within ${timeout}s from $app" (Get-ChildItem "$data\logs" -ErrorAction SilentlyContinue | Get-Content -Tail 20 | Out-String)
}

foreach ($file in @($Previous, $Msi)) { if (-not (Test-Path $file -PathType Leaf)) { Stop-Smoke "no installer at $file" } }
$Previous = (Resolve-Path $Previous).Path
$Msi = (Resolve-Path $Msi).Path
if (-not $expected) { Stop-Smoke "no expected version (Cargo.toml or EXPECTED_VERSION)" }
Write-Host "upgrade $(Split-Path -Leaf $Previous) -> $(Split-Path -Leaf $Msi), expecting $expected"
if (Test-Path $data) { Stop-Smoke "$data already exists: not a clean machine" }
if (Get-UninstallEntry) { Stop-Smoke "Contour is already installed" (Get-UninstallEntry | Format-List DisplayName, DisplayVersion | Out-String) }

# The previous release, used: a project in the data folder, and a file.
$code = Install $Previous "msi-previous.log"
if ($code -eq 0) { Ok "msiexec /i previous release" } else { Stop-Smoke "msiexec /i previous release exited $code" (Tail "$Out\msi-previous.log") }
$old = Get-UninstallEntry
Ok "previous release installed: $($old.DisplayVersion)"
$running = Start-App
Ok "previous release runs: /api/health reports $($running.version)"
$project = Invoke-RestMethod -Method Post -ContentType "application/json" -Body '{"name":"upgrade smoke"}' "$($running.url)/api/projects"
$projectId = $project.project.id
if ($projectId) { Ok "previous release made project $projectId" } else { Fail "no project from the previous release" ($project | ConvertTo-Json -Compress) }
$marker = Join-Path $data "upgrade-smoke.txt"
Set-Content -Path $marker -Value "kept across the upgrade $(Get-Date -Format o)"
$markerHash = (Get-FileHash $marker).Hash
$store = Get-Item "$data\app.sqlite" -ErrorAction SilentlyContinue
Stop-App

# The new installer over it, as a user runs it: no uninstall first.
$code = Install $Msi "msi-upgrade.log"
if ($code -eq 0) { Ok "msiexec /i new release over the previous one" } else { Stop-Smoke "msiexec /i new release exited $code" (Tail "$Out\msi-upgrade.log") }
$entries = @(Get-UninstallEntry)
if ($entries.Count -eq 1) { Ok "one uninstall entry: $($entries[0].DisplayVersion)" }
else { Fail "want one uninstall entry, found $($entries.Count)" ($entries | Format-List DisplayName, DisplayVersion, PSChildName | Out-String) }
if ($env:MSI_VERSION) {
  if ($entries.Count -ge 1 -and @($entries | Where-Object { $_.DisplayVersion -ne $env:MSI_VERSION }).Count -eq 0) { Ok "installer version $env:MSI_VERSION" }
  else { Fail "installed version is '$(($entries | ForEach-Object DisplayVersion) -join ', ')', want $env:MSI_VERSION" }
}
$running = Start-App
if ($running.version -eq $expected) { Ok "the upgraded app runs: /api/health reports $expected" } else { Fail "after the upgrade /api/health reports '$($running.version)', want $expected" }

# The data folder as the previous release left it.
$projects = (Invoke-RestMethod "$($running.url)/api/projects").projects
if ($projectId -and ($projects | Where-Object { $_.id -eq $projectId })) { Ok "the upgraded app lists project $projectId" }
else { Fail "project $projectId is gone after the upgrade" ($projects | ConvertTo-Json -Depth 3 -Compress) }
if ((Test-Path $marker) -and (Get-FileHash $marker).Hash -eq $markerHash) { Ok "a file in $data is unchanged" } else { Fail "$marker changed or went during the upgrade" }
if ($store -and (Test-Path "$data\app.sqlite")) { Ok "the store $data\app.sqlite is still there" } elseif ($store) { Fail "$data\app.sqlite went during the upgrade" }
Stop-App

# Uninstall the upgraded app: nothing of either release is left.
$remove = Start-Process msiexec.exe -Wait -PassThru -ArgumentList @("/x", "`"$Msi`"", "/qn", "/norestart", "/l*v", "`"$Out\msi-uninstall.log`"")
if ($remove.ExitCode -eq 0) { Ok "msiexec /x new release" } else { Fail "msiexec /x exited $($remove.ExitCode)" (Tail "$Out\msi-uninstall.log") }
if (Get-UninstallEntry) { Fail "an uninstall entry is still there" (Get-UninstallEntry | Format-List DisplayName, DisplayVersion | Out-String) } else { Ok "uninstalling leaves no entry" }
Remove-Item $marker -ErrorAction SilentlyContinue
Finish
