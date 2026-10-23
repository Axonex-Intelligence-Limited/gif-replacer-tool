#Requires -Version 5.1
<#
.SYNOPSIS
    Assembles the distributable one-double-click setup kit.

.DESCRIPTION
    Developer-side script. It collects the kit's scripts, the ESP-IDF setup
    script, the pinned release binary, and (optionally) the private
    EmotionDisplay project into one folder that can be handed to an end user.

    The app binary and the project are inputs, not repository contents: the
    first would bloat the repo, the second is confidential.

.PARAMETER ProjectSource
    Path to the EmotionDisplay project folder or a .zip of it. Omit to build a
    kit that does not carry the project (the app then asks the user to Browse).

.PARAMETER OutDir
    Where the kit is assembled.

.PARAMETER AppExe
    Use this already-downloaded GIF.Replacer.Tool.exe instead of downloading it.

.PARAMETER Force
    Delete OutDir first instead of merging into it.

.EXAMPLE
    .\build-kit.ps1 -ProjectSource D:\Axonex\companent_robot\eye\EmotionEyeDisplay_2\EmotionDisplay
#>
[CmdletBinding()]
param(
    [string]$ProjectSource,
    [string]$OutDir = 'D:\Axonex\tools\gif-replacer-setup-kit',
    [string]$AppExe,
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

$script:AppName   = 'GIF.Replacer.Tool.exe'
$script:AppUrl    = 'https://github.com/Axonex-Intelligence-Limited/gif-replacer-tool/releases/download/v1.1.1/GIF.Replacer.Tool.exe'
$script:AppSha256 = '980275ff605e60cf67c9ed1cd03c931759af2436ab30686be24f97da6ebdce42'

$scriptDir = $PSScriptRoot
$repoRoot  = Split-Path -Parent (Split-Path -Parent $scriptDir)
$idfSetup  = Join-Path $repoRoot 'scripts\setup-windows.ps1'

if (-not (Test-Path -LiteralPath $idfSetup)) {
    throw "Cannot find scripts\setup-windows.ps1 under $repoRoot. Run this script from the repository."
}

# ---------------------------------------------------------------- out dir

if (Test-Path -LiteralPath $OutDir) {
    if ($Force) {
        Remove-Item -LiteralPath $OutDir -Recurse -Force
    }
    else {
        Write-Host "Merging into the existing $OutDir (use -Force for a clean build)."
    }
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# ---------------------------------------------------------------- scripts

$kitFiles = @('Setup.bat', 'install.ps1', 'START-HERE.txt', 'WINDOWS-SETUP-GUIDE.txt', 'KIT-BUILDER-README.txt')
foreach ($name in $kitFiles) {
    $source = Join-Path $scriptDir $name
    if (-not (Test-Path -LiteralPath $source)) { throw "Missing kit source file: $source" }
    Copy-Item -LiteralPath $source -Destination (Join-Path $OutDir $name) -Force
}
Copy-Item -LiteralPath $idfSetup -Destination (Join-Path $OutDir 'setup-windows.ps1') -Force
Write-Host "  ok  scripts copied"

# ------------------------------------------------------------------- app

$targetExe = Join-Path $OutDir $script:AppName

if ($AppExe) {
    if (-not (Test-Path -LiteralPath $AppExe)) { throw "AppExe not found: $AppExe" }
    Copy-Item -LiteralPath $AppExe -Destination $targetExe -Force
}
else {
    Write-Host "  Downloading $($script:AppName)..."
    Invoke-WebRequest -Uri $script:AppUrl -OutFile $targetExe -UseBasicParsing
}

$hash = (Get-FileHash -LiteralPath $targetExe -Algorithm SHA256).Hash.ToLowerInvariant()
if ($hash -ne $script:AppSha256) {
    Remove-Item -LiteralPath $targetExe -Force -ErrorAction SilentlyContinue
    throw "App binary hash mismatch. Expected $($script:AppSha256), got $hash."
}
Write-Host "  ok  app binary verified"

# --------------------------------------------------------------- project

if ($ProjectSource) {
    if (-not (Test-Path -LiteralPath $ProjectSource)) { throw "ProjectSource not found: $ProjectSource" }
    $item = Get-Item -LiteralPath $ProjectSource

    if ($item.PSIsContainer) {
        $projectDest = Join-Path $OutDir 'EmotionDisplay'
        $robocopyArgs = @(
            $item.FullName, $projectDest,
            '/E', '/R:1', '/W:1',
            '/NFL', '/NDL', '/NJH', '/NJS', '/NP',
            '/XD', (Join-Path $item.FullName '.git'), (Join-Path $item.FullName 'build')
        )
        & robocopy.exe @robocopyArgs | Out-Null
        if ($LASTEXITCODE -ge 8) { throw "Project copy failed (robocopy exit code $LASTEXITCODE)." }

        $valid = (Test-Path -LiteralPath (Join-Path $projectDest 'sdkconfig.defaults')) -and
                 (Test-Path -LiteralPath (Join-Path $projectDest 'main')) -and
                 (Test-Path -LiteralPath (Join-Path $projectDest 'tools\gif2c.py'))
        if (-not $valid) { throw "The copied project is not an EmotionDisplay checkout (sdkconfig.defaults, main\, tools\gif2c.py)." }
        Write-Host "  ok  project copied"
    }
    elseif ($item.Extension -ieq '.zip') {
        Copy-Item -LiteralPath $item.FullName -Destination (Join-Path $OutDir 'EmotionDisplay.zip') -Force
        Write-Host "  ok  project zip copied"
    }
    else {
        throw "-ProjectSource must be a folder or a .zip file."
    }
}
else {
    Write-Warning 'No -ProjectSource given. The kit will not contain the private EmotionDisplay project.'
}

# --------------------------------------------------------------- summary

Write-Host ''
Write-Host "Kit assembled at $OutDir" -ForegroundColor Green
Get-ChildItem -LiteralPath $OutDir | Sort-Object Name | ForEach-Object {
    if ($_.PSIsContainer) {
        $count = @(Get-ChildItem -LiteralPath $_.FullName -Force).Count
        Write-Host ("  {0}\  ({1} items)" -f $_.Name, $count)
    }
    else {
        Write-Host ("  {0}  ({1:N0} bytes)" -f $_.Name, $_.Length)
    }
}
Write-Host ''
Write-Host 'Hand the whole folder to the end user; they run Setup.bat.'
