#!/usr/bin/env pwsh
#
# setup-windows.ps1 - install the ESP-IDF v5.5.2 toolchain required by the
# EmotionDisplay firmware and the GIF Replacer Tool.
#
# Safe to re-run: every step checks current state before acting, so a second
# run is a fast no-op and a partially-failed run can be resumed by re-running.
#
# On success this script prints a line containing PASS and exits 0.
# On failure it prints a line containing FAIL and exits non-zero.
#
# Requires no administrator rights. See the driver step for why.
#
# VERIFICATION STATUS
#   See scripts/setup-windows-laptop-checklist.md. The pure logic in this file
#   is unit-tested with Pester on macOS. The platform-touching paths - the
#   silent installer, the detached-process wait, the device enumeration - have
#   NOT been exercised on Windows. Test on a fresh Windows machine before
#   telling a non-technical user to run this.

# Write-Host is deliberate: this is an interactive console installer whose
# whole job is coloured progress output the user watches for 20 minutes. The
# rule's usual concern - that output cannot be captured or redirected - does not
# apply, because nothing consumes this script's stdout programmatically.
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '',
    Justification = 'Interactive console installer; coloured progress is the point.')]
# PSUseSingularNouns wants Get-IdfCandidate / Get-InstallerArg / Get-BridgeFromHardwareId.
# The plurals are the meaningful part of the name here: every one of these
# returns a collection, and the singular form would misdescribe the return.
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '',
    Justification = 'These functions return collections; the plural noun is the accurate form.')]
[CmdletBinding()]
param(
    # Where ESP-IDF lands. Short and ASCII on purpose: ESP-IDF's nested build
    # output is a realistic route past Windows' 260-character path limit, and
    # the app refuses non-ASCII paths because cmd.exe parses the .bat in the
    # console OEM codepage.
    [string]$IdfDir = 'C:\Espressif'
)

$ErrorActionPreference = 'Stop'
$script:IdfVersion = 'v5.5.2'
$script:InstallerUrl = 'https://dl.espressif.com/dl/idf-installer/esp-idf-tools-setup-offline-5.5.2.exe'
$script:InstallerBytes = 1577539256

# --------------------------------------------------------------- contract
#
# One place that decides what a PASS/FAIL/skip line looks like and which
# stream it goes to, so the Mac and Windows scripts read the same to a caller.
function Get-ContractLine {
    param(
        [Parameter(Mandatory)][ValidateSet('Pass', 'Fail', 'Skip', 'Step', 'Ok')][string]$Kind,
        [Parameter(Mandatory)][string]$Message,
        [switch]$Stream   # return 'Error' or 'Output' instead of the text
    )

    # NOT `return if (...) {...} else {...}` - `if` is a statement, and only
    # works in expression position when assigned to something.
    if ($Stream) {
        if ($Kind -eq 'Fail') { return 'Error' }
        return 'Output'
    }

    switch ($Kind) {
        'Pass' { return "PASS: $Message" }
        'Fail' { return "FAIL: $Message" }
        'Skip' { return "-- $Message (already done)" }
        'Step' { return "==> $Message" }
        'Ok'   { return "  ok  $Message" }
    }
}

function Write-Step { param([string]$Message) Write-Host (Get-ContractLine -Kind Step -Message $Message) -ForegroundColor Blue }
function Write-Ok   { param([string]$Message) Write-Host (Get-ContractLine -Kind Ok   -Message $Message) -ForegroundColor Green }
function Write-Skip { param([string]$Message) Write-Host (Get-ContractLine -Kind Skip -Message $Message) -ForegroundColor DarkGray }

function Fail {
    param([Parameter(Mandatory)][string]$Message)
    Write-Host (Get-ContractLine -Kind Fail -Message $Message) -ForegroundColor Red -ErrorAction Continue
    exit 1
}

# ------------------------------------------------------ IDF discovery (pure)
#
# Mirrors builder.rs::idf_candidates. The Rust and PowerShell copies must agree;
# if one changes, change the other. Inputs are explicit so this is testable on
# macOS, where none of these paths exist.

# Windows-path join, deliberately NOT Join-Path.
#
# Join-Path is wrong here twice over: it resolves the drive (throwing
# DriveNotFoundException for a path on a drive this host does not have), and it
# uses the HOST separator - so on macOS it would build
# "/frameworks/esp-idf-v5.5.2" for what is meant to be a Windows path. These
# are Windows paths and must be built identically wherever they are built.
function Join-WindowsPath {
    param([Parameter(Mandatory)][string]$Base, [Parameter(Mandatory)][string]$Child)
    return $Base.TrimEnd('\', '/') + '\' + $Child.TrimStart('\', '/')
}

function Get-IdfCandidates {
    param(
        [string]$Configured,
        [hashtable]$Environment,
        # NOT -Profile: $Profile is a PowerShell automatic variable holding the
        # profile script path. A parameter of that name shadows it.
        [string]$UserProfile
    )

    $v = [System.Collections.Generic.List[string]]::new()

    if ($Configured) { $v.Add($Configured) }

    # IDF_TOOLS_PATH points at the tools root; the framework sits under it.
    if ($Environment.ContainsKey('IDF_TOOLS_PATH') -and $Environment['IDF_TOOLS_PATH']) {
        $v.Add((Join-WindowsPath $Environment['IDF_TOOLS_PATH'] 'frameworks\esp-idf-v5.5.2'))
    }

    # The official installer's default layout, then the two common hand-clone
    # locations. Same order as the Rust function.
    $v.Add('C:\Espressif\frameworks\esp-idf-v5.5.2')
    if ($UserProfile) { $v.Add((Join-WindowsPath $UserProfile 'esp\esp-idf-v5.5.2')) }
    $v.Add('C:\esp\esp-idf-v5.5.2')

    return $v.ToArray()
}

# export.bat is the marker builder.rs uses (idf_marker() returns it on Windows).
#
# The $IDF_PATH fallback is not decoration: resolve_idf_path checks the probe
# list first and then falls back to the environment variable, WITHOUT requiring
# the marker to be there. Dropping it would make this script miss an install
# the app would use.
function Find-IdfPath {
    param([string[]]$Candidates, [hashtable]$Environment = @{})

    foreach ($c in $Candidates) {
        if (-not $c) { continue }

        # BOTH calls are inside the try on purpose. Join-Path resolves the
        # drive as well as Test-Path, so a candidate naming a drive that does
        # not exist throws from Join-Path - before Test-Path is even reached.
        # That is not hypothetical: a configured idf_path pointing at an
        # unplugged removable drive hits it, and the throw would abort setup
        # with a raw exception instead of trying the next candidate.
        $found = $false
        try {
            $marker = Join-Path $c 'export.bat'
            $found  = Test-Path -LiteralPath $marker
        }
        catch { $found = $false }

        if ($found) { return $c }
    }

    if ($Environment.ContainsKey('IDF_PATH') -and $Environment['IDF_PATH']) {
        return $Environment['IDF_PATH']
    }
    return $null
}

# ------------------------------------------------------ download (pure)

function Get-PartialPath {
    param([Parameter(Mandatory)][string]$Target)
    return "$Target.part"
}

# An unknown expected length is a failure, not a pass. Downloading 1.58 GB and
# installing from a truncated file produces a broken ESP-IDF that reports
# success - the one outcome this check exists to prevent.
function Test-DownloadIntegrity {
    param([long]$Expected, [long]$Actual)
    if ($Expected -le 0) { return $false }
    return ($Expected -eq $Actual)
}

# ------------------------------------------------------ silent install

function Get-InstallerArgs {
    param([Parameter(Mandatory)][string]$IdfDir, [Parameter(Mandatory)][string]$LogPath)

    # /USEEMBEDDEDPYTHON=yes is the default, but stated explicitly: it is what
    # removes Python from the prerequisites, and a silent run gives no
    # opportunity to notice the default changing.
    # /IDFVERSION is deliberately absent: the offline installer is already the
    # 5.5.2 build, and the flag drives a dropdown fed by idf_versions.txt,
    # which does not list 5.5.2.
    return @(
        '/VERYSILENT'
        '/SUPPRESSMSGBOXES'
        '/SP-'
        '/NOCANCEL'
        '/USEEMBEDDEDPYTHON=yes'
        '/SKIPSYSTEMCHECK=yes'
        "/IDFDIR=$IdfDir"
        "/LOG=$LogPath"
    )
}

function Wait-ProcessGone {
    param(
        [Parameter(Mandatory)][string]$Name,
        [int]$TimeoutSeconds = 3600,
        [int]$PollSeconds = 5
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if (-not (Get-Process -Name $Name -ErrorAction SilentlyContinue)) { return $true }
        Start-Sleep -Seconds $PollSeconds
    }
    return $false
}

# The installer's exit code is unreliable once it has re-launched itself, so the
# log is the authority. An empty log means it never got far enough to write one,
# which is a failure - treating it as success would verify a tree that is not
# there.
#
# Failure markers are checked FIRST, so an ambiguous log errs toward reporting a
# problem rather than proceeding. That is the deliberate direction: a false
# FAIL costs a re-run, a false PASS would flash a half-installed toolchain.
function Get-InstallerLogVerdict {
    param([string]$LogText)

    if ([string]::IsNullOrWhiteSpace($LogText)) { return 'Failed' }
    if ($LogText -match 'failed|error')      { return 'Failed' }
    if ($LogText -match 'completed successfully|Installation completed|success') { return 'Ok' }
    return 'Failed'
}

# ------------------------------------------------------ driver detection
#
# Vendor IDs, not port names. A CH340 is 1A86, a CP210x is 10C4, an FTDI is
# 0403. Matching the hardware ID is exact; matching "/dev/cu.usbserial-*" is a
# guess that happens to be right on macOS.

$script:BridgeVendorIds = [ordered]@{
    '1A86' = 'CH340'    # WCH - needs WCH's own driver
    '10C4' = 'CP210x'   # Silicon Labs - covered by the ESP-IDF installer
    '0403' = 'FTDI'     # FTDI - covered by the ESP-IDF installer
}

function Get-BridgeFromHardwareIds {
    param([string[]]$HardwareIds)

    foreach ($id in $HardwareIds) {
        if ($id -match 'VID_([0-9A-Fa-f]{4})') {
            $vid = $Matches[1].ToUpperInvariant()
            if ($script:BridgeVendorIds.Contains($vid)) { return $script:BridgeVendorIds[$vid] }
        }
    }
    return $null
}

# ------------------------------------------------------ config wiring

# The app owns %USERPROFILE%\.gif-tool-config.json. This merges one key so the
# other two survive; a corrupted file is replaced rather than allowed to abort
# setup at the last step.
function Merge-ToolConfig {
    param([string]$ExistingJson, [Parameter(Mandatory)][string]$IdfPath)

    $cfg = $null
    if (-not [string]::IsNullOrWhiteSpace($ExistingJson)) {
        try { $cfg = $ExistingJson | ConvertFrom-Json } catch { $cfg = $null }
    }
    if (-not $cfg) {
        $cfg = [pscustomobject]@{ project_path = $null; idf_path = $null; last_serial_port = $null }
    }

    $cfg.idf_path = $IdfPath
    return ($cfg | ConvertTo-Json -Depth 4)
}

function Main {
    # Taken as a parameter rather than read from the script scope. It is the
    # only thing Main needs from outside, an explicit dependency is easier to
    # follow, and PSScriptAnalyzer cannot see a script-scope read from inside a
    # function - so this also removes a standing false positive.
    param([string]$IdfDir)

    Write-Host "ESP-IDF $script:IdfVersion setup for Windows"
    Write-Host "This takes 15-30 minutes and downloads about 1.6 GB. It is safe to re-run."

    # ---- preflight ------------------------------------------------------
    Write-Step 'Preflight'
    if (-not [Environment]::Is64BitOperatingSystem) {
        Fail 'A 64-bit Windows installation is required; this is 32-bit.'
    }
    $drive = (Split-Path -Qualifier $IdfDir)

    # Get-PSDrive THROWS for a drive that does not exist, which would abort
    # with a raw exception instead of the FAIL line this script promises. That
    # happens as soon as someone passes -IdfDir Z:\Espressif for a drive they
    # do not have, so it is worth a real message.
    $psDrive = Get-PSDrive -Name $drive.TrimEnd(':') -ErrorAction SilentlyContinue
    if (-not $psDrive) {
        Fail "Drive $drive does not exist. Pass a drive that does, e.g. -IdfDir C:\Espressif"
    }

    $free = $psDrive.Free
    if ($free -lt 4GB) {
        Fail ("Only {0:N1} GB free on {1}. About 4 GB is needed (1.6 GB download, ~2 GB extracted)." -f ($free / 1GB), $drive)
    }
    Write-Ok '64-bit Windows, enough free disk'

    # ---- ESP-IDF --------------------------------------------------------
    Write-Step "ESP-IDF $script:IdfVersion"

    # NOT $profile: $Profile is a PowerShell automatic variable (the profile
    # script path), and assigning to it shadows that for the rest of the scope.
    $userProfile = $env:USERPROFILE
    $configPath = Join-Path $userProfile '.gif-tool-config.json'
    $configured = ''
    if (Test-Path $configPath) {
        try { $configured = (Get-Content -Raw $configPath | ConvertFrom-Json).idf_path } catch { $configured = '' }
    }

    # Built once and reused: Get-IdfCandidates and Find-IdfPath must see the same
    # environment, or the two halves of discovery disagree about what exists.
    $envTable = @{
        IDF_TOOLS_PATH = $env:IDF_TOOLS_PATH
        IDF_PATH       = $env:IDF_PATH
    }
    $candidates = Get-IdfCandidates -Configured $configured -Environment $envTable -UserProfile $userProfile
    $found = Find-IdfPath -Candidates $candidates -Environment $envTable

    if ($found) {
        Write-Skip "ESP-IDF already at $found"
        $idfPath = $found
    }
    else {
        $tmp  = Join-Path ([System.IO.Path]::GetTempPath()) 'esp-idf-tools-setup-offline-5.5.2.exe'
        $part = Get-PartialPath -Target $tmp
        $log  = Join-Path ([System.IO.Path]::GetTempPath()) 'gif-tool-idf-install.log'

        Write-Host "  Downloading 1.6 GB to $part (this is the slow part)..."
        try {
            Invoke-WebRequest -Uri $script:InstallerUrl -OutFile $part -UseBasicParsing
        }
        catch {
            Remove-Item -Force $part -ErrorAction SilentlyContinue
            Fail "Download failed: $($_.Exception.Message). Check your network and re-run."
        }

        $actual = (Get-Item $part).Length
        if (-not (Test-DownloadIntegrity -Expected $script:InstallerBytes -Actual $actual)) {
            Remove-Item -Force $part -ErrorAction SilentlyContinue
            Fail ("Download is {0} bytes, expected {1}. The partial file was deleted; re-run to retry." -f $actual, $script:InstallerBytes)
        }

        Move-Item -Force $part $tmp
        Write-Ok 'Downloaded and length-verified'

        Remove-Item -Force $log -ErrorAction SilentlyContinue
        Write-Host '  Installing. Progress output follows.'
        # NOT $args: that is a PowerShell automatic variable, and assigning to
        # it silently breaks argument handling rather than erroring.
        $installerArgs = Get-InstallerArgs -IdfDir $IdfDir -LogPath $log
        Start-Process -FilePath $tmp -ArgumentList $installerArgs -Wait

        # The installer detaches; the log, not the exit code, is the authority.
        if (-not (Wait-ProcessGone -Name 'esp-idf-tools-setup-offline-5.5.2')) {
            Fail "The installer is still running after the timeout. Check $log."
        }

        $logText = if (Test-Path $log) { Get-Content -Raw $log } else { '' }
        if ((Get-InstallerLogVerdict -LogText $logText) -ne 'Ok') {
            Fail "The installer reported a problem. See $log."
        }
        Remove-Item -Force $tmp -ErrorAction SilentlyContinue
        Write-Ok "Installed ESP-IDF $script:IdfVersion"

        $candidates = Get-IdfCandidates -Configured $IdfDir -Environment $envTable -UserProfile $userProfile
        $idfPath = Find-IdfPath -Candidates $candidates -Environment $envTable
        if (-not $idfPath) {
            Fail "Installed, but no export.bat found. Looked in: $($candidates -join ', ')"
        }
    }

    # ---- driver ---------------------------------------------------------
    Write-Step 'USB-serial driver'
    $ids = @(Get-CimInstance Win32_PnPEntity -ErrorAction SilentlyContinue |
             Where-Object { $_.HardwareID } | ForEach-Object { $_.HardwareID })
    $bridge = Get-BridgeFromHardwareIds -HardwareIds $ids

    if (-not $bridge) {
        Write-Host '  No USB-serial bridge detected. That does not block setup - the'
        Write-Host '  build works without a board attached.'
        Write-Host '  Plug the board in and re-run when you want to flash.'
    }
    elseif ($bridge -eq 'CH340') {
        Write-Host '  A CH340 is attached. If Windows shows it with a warning triangle in'
        Write-Host '  Device Manager, install WCH''s driver from the official page:'
        Write-Host '    https://www.wch-ic.com/downloads/CH341SER.EXE.html'
        Write-Host '  This script will not download a kernel driver from an unverified URL.'
    }
    else {
        Write-Ok "$bridge detected and enumerated by Windows"
    }

    # ---- wire the app ---------------------------------------------------
    Write-Step 'Pointing the GIF Replacer Tool at this install'
    $existing = if (Test-Path $configPath) { Get-Content -Raw $configPath } else { '' }
    $merged = Merge-ToolConfig -ExistingJson $existing -IdfPath $idfPath
    Set-Content -Path $configPath -Value $merged -Encoding UTF8
    Write-Ok "idf_path = $idfPath"

    # ---- verify ---------------------------------------------------------
    Write-Step 'Verifying installation'
    $verify = Join-Path ([System.IO.Path]::GetTempPath()) "gif_tool_verify_$PID.bat"
    @"
@echo off
set "IDF_PATH="
set "IDF_PYTHON_ENV_PATH="
set "VIRTUAL_ENV="
call "$idfPath\export.bat" >nul 2>&1
if errorlevel 1 exit /b 1
idf.py --version
"@ | Set-Content -Path $verify -Encoding ASCII

    $reported = (& cmd /c $verify 2>&1 | Select-Object -First 1)
    Remove-Item -Force $verify -ErrorAction SilentlyContinue

    if ("$reported" -notmatch '5\.5\.2') {
        Fail "Expected ESP-IDF 5.5.2, got: $(if ($reported) { $reported } else { '<no output>' })"
    }

    Write-Host ''
    Write-Host (Get-ContractLine -Kind Pass -Message "ESP-IDF $script:IdfVersion") -ForegroundColor Green
    Write-Host "`nESP-IDF is installed at $idfPath"
    Write-Host 'You can now open the GIF Replacer Tool.'
    exit 0
}

# Dot-sourcing (how the tests load this file) leaves InvocationName as '.';
# running it as a script sets it to the script path. The tests need the
# functions without the main body firing.
#
# This guard sits AFTER Main's definition and BEFORE its invocation on purpose.
# Placed above the definition - as it first was - dot-sourcing returns before
# Main exists, and the shape test finds nothing.
if ($MyInvocation.InvocationName -eq '.') { return }

Main -IdfDir $IdfDir
