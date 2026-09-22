#!/usr/bin/env pwsh
#
# setup-windows.ps1 — install the ESP-IDF v5.5.2 toolchain required by the
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
#   is unit-tested with Pester on macOS. The platform-touching paths — the
#   silent installer, the detached-process wait, the device enumeration — have
#   NOT been exercised on Windows. Test on a fresh Windows machine before
#   telling a non-technical user to run this.

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

    # NOT `return if (...) {...} else {...}` — `if` is a statement, and only
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
# uses the HOST separator — so on macOS it would build
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
        [string]$Profile
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
    if ($Profile) { $v.Add((Join-WindowsPath $Profile 'esp\esp-idf-v5.5.2')) }
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
        # not exist throws from Join-Path — before Test-Path is even reached.
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
# success — the one outcome this check exists to prevent.
function Test-DownloadIntegrity {
    param([long]$Expected, [long]$Actual)
    if ($Expected -le 0) { return $false }
    return ($Expected -eq $Actual)
}

# Dot-sourcing (how the tests load this file) leaves InvocationName as '.';
# running it as a script sets it to the script path. The tests need the
# functions without the main body firing.
if ($MyInvocation.InvocationName -eq '.') { return }

function Main {
    Fail 'not implemented yet'
}

Main
