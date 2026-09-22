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

# Dot-sourcing (how the tests load this file) leaves InvocationName as '.';
# running it as a script sets it to the script path. The tests need the
# functions without the main body firing.
if ($MyInvocation.InvocationName -eq '.') { return }

function Main {
    Fail 'not implemented yet'
}

Main
