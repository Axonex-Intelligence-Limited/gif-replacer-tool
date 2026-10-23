#Requires -Version 5.1
<#
.SYNOPSIS
    One-shot setup for the GIF Replacer Tool.

.DESCRIPTION
    Installs, in order: ESP-IDF 5.5.2, the GIF Replacer Tool, and (if bundled)
    the EmotionDisplay project. Then writes the tool's config, creates a desktop
    shortcut, and reports on the USB-serial driver.

    Safe to re-run: every step checks current state first, so a second run is a
    fast no-op. No administrator rights are required and no Python, Git, VS Code
    or Rust installation is needed by the end user.

.PARAMETER InstallDir
    Where the app and its launcher are placed. Defaults to %LOCALAPPDATA%.

.PARAMETER ProjectDest
    Where the bundled EmotionDisplay project is copied. Defaults to C:\EmotionDisplay.

.PARAMETER IdfDir
    ESP-IDF tools root. Must match what setup-windows.ps1 used. Defaults to C:\Espressif.

.PARAMETER SkipIdf
    Do not install or verify ESP-IDF (used for testing the rest of the kit).

.PARAMETER SkipProject
    Do not copy the bundled EmotionDisplay project.

.PARAMETER NoShortcut
    Do not create the desktop shortcut.
#>
[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'GIF Replacer Tool'),
    [string]$ProjectDest = 'C:\EmotionDisplay',
    [string]$IdfDir = 'C:\Espressif',
    [switch]$SkipIdf,
    [switch]$SkipProject,
    [switch]$NoShortcut
)

$ErrorActionPreference = 'Stop'

$script:KitDir     = $PSScriptRoot
$script:AppName    = 'GIF.Replacer.Tool.exe'
$script:AppSha256  = '980275ff605e60cf67c9ed1cd03c931759af2436ab30686be24f97da6ebdce42'
$script:LogDir     = Join-Path $env:LOCALAPPDATA 'GifReplacerSetup'
$script:ConfigPath = Join-Path $env:USERPROFILE '.gif-tool-config.json'
$script:Failures   = New-Object System.Collections.Generic.List[string]
$script:IdfPath    = $null
$script:ProjectOk  = $false

# ----------------------------------------------------------------- output

function Write-Step {
    param([string]$Message)
    Write-Host ''
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Write-Ok {
    param([string]$Message)
    Write-Host "  ok  $Message" -ForegroundColor Green
}

function Write-Warn {
    param([string]$Message)
    Write-Host "  !!  $Message" -ForegroundColor Yellow
}

function Add-Failure {
    param([string]$Message)
    Write-Host "FAIL: $Message" -ForegroundColor Red
    $script:Failures.Add($Message)
}

# ------------------------------------------------------------ tool config

function Read-ToolConfig {
    if (-not (Test-Path -LiteralPath $script:ConfigPath)) { return $null }
    try {
        $text = Get-Content -LiteralPath $script:ConfigPath -Raw -ErrorAction Stop
        return ($text | ConvertFrom-Json)
    }
    catch { return $null }
}

# Writes UTF-8 WITHOUT a BOM on purpose. Windows PowerShell 5.1's
# `Set-Content -Encoding UTF8` adds one, and Rust's serde_json rejects a BOM,
# which makes the app treat its own config as corrupt and reset it.
function Save-ToolConfig {
    param([hashtable]$Values)

    $obj = Read-ToolConfig
    if ($null -eq $obj) { $obj = New-Object PSObject }

    foreach ($key in $Values.Keys) {
        if ($obj.PSObject.Properties[$key]) {
            $obj.$key = [string]$Values[$key]
        }
        else {
            $obj | Add-Member -NotePropertyName $key -NotePropertyValue ([string]$Values[$key])
        }
    }

    $json = $obj | ConvertTo-Json -Depth 4
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($script:ConfigPath, $json, $utf8NoBom)
}

# ------------------------------------------------------------------- IDF

function Get-IdfCandidates {
    $list = New-Object System.Collections.Generic.List[string]

    $cfg = Read-ToolConfig
    if ($cfg -and $cfg.idf_path) { $list.Add([string]$cfg.idf_path) }

    if ($env:IDF_TOOLS_PATH) {
        $list.Add((Join-Path $env:IDF_TOOLS_PATH 'frameworks\esp-idf-v5.5.2'))
    }
    $list.Add((Join-Path $IdfDir 'frameworks\esp-idf-v5.5.2'))
    if ($env:USERPROFILE) {
        $list.Add((Join-Path $env:USERPROFILE 'esp\esp-idf-v5.5.2'))
    }
    $list.Add('C:\esp\esp-idf-v5.5.2')

    return $list
}

# Runs `idf.py --version` through a temporary .bat, the same way the app does.
# The venv's Scripts directory goes first on PATH so export.bat's internal
# `python activate.py --export` runs under the IDF Python (3.11) rather than
# whatever system Python happens to be first. Without this, export.bat looks
# for an environment named after the system Python (e.g. idf5.5_py3.14_env)
# that was never created.
function Get-IdfVersion {
    param([string]$IdfPathCandidate)

    $tempBat = Join-Path ([System.IO.Path]::GetTempPath()) ("gif_tool_verify_{0}.bat" -f [guid]::NewGuid().ToString('N'))
    $lines = @(
        '@echo off'
        'set "IDF_PATH="'
        'set "IDF_PYTHON_ENV_PATH="'
        'set "VIRTUAL_ENV="'
        ('call "' + $IdfPathCandidate + '\export.bat" >nul 2>&1')
        'if errorlevel 1 exit /b 1'
        'idf.py --version'
    )
    [System.IO.File]::WriteAllText($tempBat, ($lines -join "`r`n"), [System.Text.Encoding]::ASCII)

    $venv = Get-ChildItem -LiteralPath (Join-Path $IdfDir 'python_env') -Directory -Filter 'idf*_env' -ErrorAction SilentlyContinue |
            Select-Object -First 1

    $oldPath = $env:PATH
    try {
        if ($venv) { $env:PATH = (Join-Path $venv.FullName 'Scripts') + ';' + $env:PATH }
        $out = & cmd.exe /c $tempBat 2>&1
        return (($out | Out-String).Trim())
    }
    catch { return '' }
    finally {
        $env:PATH = $oldPath
        Remove-Item -LiteralPath $tempBat -Force -ErrorAction SilentlyContinue
    }
}

function Find-Idf552 {
    $candidates = Get-IdfCandidates
    foreach ($candidate in $candidates) {
        if (-not $candidate) { continue }
        if (-not (Test-Path -LiteralPath (Join-Path $candidate 'export.bat'))) { continue }

        $version = Get-IdfVersion -IdfPathCandidate $candidate
        if ($version -match '5\.5\.2') { return $candidate }

        Write-Warn "ESP-IDF found at $candidate but it reported '$version'; 5.5.2 is required."
    }
    return $null
}

function Resolve-Idf {
    Write-Step 'ESP-IDF 5.5.2'

    if ($SkipIdf) {
        $cfg = Read-ToolConfig
        if ($cfg -and $cfg.idf_path) {
            $script:IdfPath = [string]$cfg.idf_path
            Write-Ok "Using $($script:IdfPath) from config (not verified: -SkipIdf)"
        }
        else { Write-Warn 'Skipped (-SkipIdf); no idf_path in config.' }
        return
    }

    $found = Find-Idf552
    if ($found) {
        $script:IdfPath = $found
        Write-Ok "Already installed at $found"
        return
    }

    $setupScript = Join-Path $script:KitDir 'setup-windows.ps1'
    if (-not (Test-Path -LiteralPath $setupScript)) {
        Add-Failure 'setup-windows.ps1 is missing from the kit folder. The kit is incomplete.'
        return
    }

    Write-Host '  ESP-IDF is not installed. Starting the official installer.' -ForegroundColor Gray
    Write-Host '  It downloads about 1.6 GB and takes 15-30 minutes. Do not close this window.' -ForegroundColor Gray

    # Child process, not dot-sourcing: setup-windows.ps1 calls `exit 1` on
    # failure, which would terminate this script too if it shared the process.
    $argumentLine = '-NoProfile -ExecutionPolicy Bypass -File "' + $setupScript + '" -IdfDir "' + $IdfDir + '"'
    $process = Start-Process -FilePath 'powershell.exe' -ArgumentList $argumentLine -NoNewWindow -Wait -PassThru

    if ($process.ExitCode -ne 0) {
        Add-Failure "The ESP-IDF installer failed (exit code $($process.ExitCode)). See the messages above."
        return
    }

    $found = Find-Idf552
    if ($found) {
        $script:IdfPath = $found
        Write-Ok "Installed at $found"
    }
    else {
        Add-Failure 'The installer reported success, but no working ESP-IDF 5.5.2 was found afterwards.'
    }
}

# ------------------------------------------------------------------- app

function Write-Launcher {
    # @@PLACEHOLDER@@ tokens keep this template free of PowerShell escaping.
    # The venv glob mirrors the app's own discovery: any idf*_env directory.
    $template = @'
@echo off
setlocal
set "VENV="
for /d %%D in ("@@IDF_DIR@@\python_env\idf*_env") do set "VENV=%%D"
if defined VENV set "PATH=%VENV%\Scripts;%PATH%"
start "" "%~dp0@@APP_NAME@@"
endlocal
'@

    $content = $template.Replace('@@IDF_DIR@@', $IdfDir).Replace('@@APP_NAME@@', $script:AppName)
    $launcherPath = Join-Path $InstallDir 'launch.bat'
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($launcherPath, $content, $utf8NoBom)

    if (-not (Test-Path -LiteralPath $launcherPath)) {
        Add-Failure 'Could not create launch.bat.'
    }
    else {
        Write-Ok "Launcher created: $launcherPath"
    }
}

function Install-App {
    Write-Step 'GIF Replacer Tool application'

    $sourceExe = Join-Path $script:KitDir $script:AppName
    if (-not (Test-Path -LiteralPath $sourceExe)) {
        Add-Failure "$($script:AppName) is missing from the kit folder."
        return
    }

    $sourceHash = (Get-FileHash -LiteralPath $sourceExe -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($sourceHash -ne $script:AppSha256) {
        Add-Failure "The app binary does not match the expected release. Expected $($script:AppSha256), got $sourceHash. Do not run this file."
        return
    }
    Write-Ok 'App binary hash verified'

    try {
        New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
        $targetExe = Join-Path $InstallDir $script:AppName
        Copy-Item -LiteralPath $sourceExe -Destination $targetExe -Force

        $targetHash = (Get-FileHash -LiteralPath $targetExe -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($targetHash -ne $script:AppSha256) {
            Add-Failure 'The copied app is corrupted (hash mismatch after copy).'
            return
        }
    }
    catch {
        Add-Failure "Could not copy the app to ${InstallDir}: $($_.Exception.Message)"
        return
    }

    Write-Launcher
    Write-Ok "Installed to $InstallDir"
}

# --------------------------------------------------------------- project

function Test-ProjectRoot {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return $false }
    return ((Test-Path -LiteralPath (Join-Path $Path 'sdkconfig.defaults')) -and
            (Test-Path -LiteralPath (Join-Path $Path 'main')) -and
            (Test-Path -LiteralPath (Join-Path $Path 'tools\gif2c.py')))
}

# A zip made by right-click -> Send to -> Compressed folder often wraps the
# project in one extra directory. Move it up so the expected layout is at root.
function Repair-ExpandedProject {
    if (Test-ProjectRoot -Path $ProjectDest) { return }
    $children = @(Get-ChildItem -LiteralPath $ProjectDest -Directory -ErrorAction SilentlyContinue)
    if ($children.Count -eq 1 -and (Test-ProjectRoot -Path $children[0].FullName)) {
        Get-ChildItem -LiteralPath $children[0].FullName -Force |
            Move-Item -Destination $ProjectDest -Force
        Remove-Item -LiteralPath $children[0].FullName -Force -Recurse
    }
}

function Install-Project {
    Write-Step 'EmotionDisplay project'

    if ($SkipProject) {
        Write-Warn 'Skipped (-SkipProject).'
        return
    }

    if (Test-ProjectRoot -Path $ProjectDest) {
        $script:ProjectOk = $true
        Write-Ok "Already present at $ProjectDest"
        return
    }

    $sourceDir = Join-Path $script:KitDir 'EmotionDisplay'
    $sourceZip = Join-Path $script:KitDir 'EmotionDisplay.zip'

    if (Test-Path -LiteralPath $sourceDir) {
        Write-Host "  Copying to $ProjectDest (this can take a minute)..." -ForegroundColor Gray
        # .git and build are excluded: the first is large and unnecessary for a
        # non-developer, the second is stale output that would be rebuilt anyway.
        $robocopyArgs = @(
            $sourceDir, $ProjectDest,
            '/E', '/R:1', '/W:1',
            '/NFL', '/NDL', '/NJH', '/NJS', '/NP',
            '/XD', (Join-Path $sourceDir '.git'), (Join-Path $sourceDir 'build')
        )
        & robocopy.exe @robocopyArgs | Out-Null
        # Robocopy exit codes 0-7 are success; 8 and above are failures.
        if ($LASTEXITCODE -ge 8) {
            Add-Failure "Project copy failed (robocopy exit code $LASTEXITCODE)."
            return
        }
    }
    elseif (Test-Path -LiteralPath $sourceZip) {
        try {
            Expand-Archive -LiteralPath $sourceZip -DestinationPath $ProjectDest -Force
            Repair-ExpandedProject
        }
        catch {
            Add-Failure "Could not unpack ${sourceZip}: $($_.Exception.Message)"
            return
        }
    }
    else {
        Write-Warn 'No EmotionDisplay folder or zip was bundled with this kit.'
        Write-Warn 'The app will ask you to Browse to the project folder on first run.'
        return
    }

    if (Test-ProjectRoot -Path $ProjectDest) {
        $script:ProjectOk = $true
        Write-Ok "Ready at $ProjectDest"
    }
    else {
        Add-Failure "The copy at $ProjectDest is missing sdkconfig.defaults, main\ or tools\gif2c.py."
    }
}

function Write-ToolConfig {
    Write-Step 'Tool configuration'

    $values = @{}
    if ($script:IdfPath) { $values['idf_path'] = $script:IdfPath }
    if ($script:ProjectOk) { $values['project_path'] = $ProjectDest }

    if ($values.Count -eq 0) {
        Write-Warn 'Nothing to configure.'
        return
    }

    try {
        Save-ToolConfig -Values $values
        Write-Ok "Updated $script:ConfigPath"
    }
    catch {
        Add-Failure "Could not update the tool config: $($_.Exception.Message)"
    }
}

# -------------------------------------------------------------- shortcut

function New-DesktopShortcut {
    Write-Step 'Desktop shortcut'

    if ($NoShortcut) {
        Write-Warn 'Skipped (-NoShortcut).'
        return
    }

    $launcherPath = Join-Path $InstallDir 'launch.bat'
    if (-not (Test-Path -LiteralPath $launcherPath)) {
        Add-Failure 'Cannot create the shortcut: launch.bat was not created.'
        return
    }

    try {
        $desktop = [Environment]::GetFolderPath('Desktop')
        $shortcutPath = Join-Path $desktop 'GIF Replacer Tool.lnk'
        $shell = New-Object -ComObject WScript.Shell
        $shortcut = $shell.CreateShortcut($shortcutPath)
        $shortcut.TargetPath = $launcherPath
        $shortcut.WorkingDirectory = $InstallDir
        $shortcut.WindowStyle = 7          # minimized: hides the .bat flash
        $shortcut.Description = 'GIF Replacer Tool'

        $exePath = Join-Path $InstallDir $script:AppName
        if (Test-Path -LiteralPath $exePath) { $shortcut.IconLocation = $exePath + ',0' }

        $shortcut.Save()
        Write-Ok "Created 'GIF Replacer Tool' on the Desktop"
    }
    catch {
        Add-Failure "Could not create the desktop shortcut: $($_.Exception.Message)"
    }
}

# ------------------------------------------------------------- USB check

function Show-UsbStatus {
    Write-Step 'USB serial port'

    try {
        $devices = @(Get-CimInstance Win32_PnPEntity -ErrorAction Stop)
    }
    catch {
        Write-Warn "Could not enumerate USB devices: $($_.Exception.Message)"
        return
    }

    $ports = @($devices | Where-Object { $_.Name -match '\(COM\d+\)' })
    if ($ports.Count -gt 0) {
        $names = ($ports | ForEach-Object { $_.Name }) -join '; '
        Write-Ok "COM port(s): $names"
        return
    }

    $bridges = @($devices | Where-Object {
        $_.HardwareID -and ($_.HardwareID -match 'VID_1A86|VID_10C4|VID_0403')
    })

    if ($bridges.Count -gt 0) {
        Write-Warn 'A USB-serial chip is attached but Windows has not given it a COM port.'
        Write-Host '      Install the driver from the official WCH page:' -ForegroundColor Yellow
        Write-Host '        https://www.wch-ic.com/downloads/CH341SER.EXE.html' -ForegroundColor Yellow
        Write-Host '      Then unplug/replug the board and run setup again.' -ForegroundColor Yellow
    }
    else {
        Write-Ok 'No board attached. That is fine; the build works without one.'
    }
}

# -------------------------------------------------------------- preflight

function Test-DiskSpace {
    param([string]$Path, [double]$NeedGB, [string]$Label)

    try {
        $root = [System.IO.Path]::GetPathRoot($Path)
        if (-not $root) { return $true }
        $drive = New-Object System.IO.DriveInfo($root)
        $freeGB = $drive.AvailableFreeSpace / 1GB
        if ($freeGB -lt $NeedGB) {
            Add-Failure "$Label needs about $NeedGB GB free on $root; only $([math]::Round($freeGB, 1)) GB is available."
            return $false
        }
        Write-Ok "${Label}: $([math]::Round($freeGB, 1)) GB free on $root"
        return $true
    }
    catch {
        Write-Warn "Could not check disk space for ${Path}: $($_.Exception.Message)"
        return $true
    }
}

function Test-Preflight {
    Write-Step 'Preflight'
    $ok = $true

    if (-not [Environment]::Is64BitOperatingSystem) {
        Add-Failure '64-bit Windows is required.'
        $ok = $false
    }
    else { Write-Ok '64-bit Windows' }

    if ($PSVersionTable.PSVersion.Major -lt 5) {
        Add-Failure 'Windows PowerShell 5.1 or newer is required.'
        $ok = $false
    }

    if (-not (Test-DiskSpace -Path $IdfDir -NeedGB 4 -Label 'ESP-IDF')) { $ok = $false }
    if (-not (Test-DiskSpace -Path $InstallDir -NeedGB 0.1 -Label 'App')) { $ok = $false }

    if (-not $SkipIdf) {
        $reachable = $false
        try {
            $null = Invoke-WebRequest -Uri 'https://github.com' -Method Head -UseBasicParsing -TimeoutSec 15 -ErrorAction Stop
            $reachable = $true
        }
        catch { }

        if ($reachable) {
            Write-Ok 'Internet reachable'
        }
        elseif (Find-Idf552) {
            Write-Warn 'github.com is unreachable, but ESP-IDF is already installed; continuing.'
        }
        else {
            Add-Failure 'No internet access and ESP-IDF is not installed. Connect to the internet and run setup again.'
            $ok = $false
        }
    }

    return $ok
}

# ------------------------------------------------------------------- main

function Invoke-Setup {
    Write-Host ''
    Write-Host 'GIF Replacer Tool - one-time setup' -ForegroundColor White
    Write-Host '===================================' -ForegroundColor White

    if (-not (Test-Preflight)) { return }

    Resolve-Idf
    Install-App
    Install-Project
    Write-ToolConfig
    New-DesktopShortcut
    Show-UsbStatus
}

function Write-Summary {
    Write-Step 'Summary'
    $logPath = Join-Path $script:LogDir 'install.log'

    if ($script:Failures.Count -eq 0) {
        Write-Host 'PASS: setup complete.' -ForegroundColor Green
        Write-Host ''
        Write-Host 'Start the app from the "GIF Replacer Tool" shortcut on your Desktop.' -ForegroundColor White
        Write-Host "Log: $logPath"
        return 0
    }

    foreach ($failure in $script:Failures) {
        Write-Host "FAIL: $failure" -ForegroundColor Red
    }
    Write-Host ''
    Write-Host "Log: $logPath" -ForegroundColor Yellow
    Write-Host 'Send that log file to whoever supports this tool.' -ForegroundColor Yellow
    return 1
}

$exitCode = 1
try {
    try {
        New-Item -ItemType Directory -Force -Path $script:LogDir | Out-Null
        Start-Transcript -Path (Join-Path $script:LogDir 'install.log') -Force | Out-Null
    }
    catch { }

    Invoke-Setup
    $exitCode = Write-Summary
}
catch {
    Add-Failure "Unexpected error: $($_.Exception.Message)"
    $exitCode = Write-Summary
}
finally {
    try { Stop-Transcript | Out-Null } catch { }
}

exit $exitCode
