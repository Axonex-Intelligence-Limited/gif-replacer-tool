Building the GIF Replacer Tool setup kit
========================================

This folder is assembled by scripts/setup-kit/build-kit.ps1 in the
gif-replacer-tool repository. Do not edit the copies in the assembled kit;
edit the source in the repository and rebuild.


Prerequisites on the build machine
----------------------------------
- Windows with PowerShell 5.1 or newer
- Network access to github.com (unless -AppExe is supplied)
- A copy of the private EmotionDisplay project


Build
-----
From PowerShell:

  cd <repo>\scripts\setup-kit
  .\build-kit.ps1 -ProjectSource "D:\path\to\EmotionDisplay" -OutDir "D:\Axonex\tools\gif-replacer-setup-kit" -Force

Or pass a .zip instead of a folder:

  .\build-kit.ps1 -ProjectSource "D:\handoff\EmotionDisplay.zip"

Or reuse an already-downloaded binary:

  .\build-kit.ps1 -ProjectSource "D:\path\to\EmotionDisplay" -AppExe "D:\downloads\GIF.Replacer.Tool.exe"

The .git and build directories are excluded from the project copy: the first
is unnecessary for a non-developer, the second is stale output.

The script verifies the app binary's SHA-256 (release v1.1.1) and refuses to
build if it does not match. To ship a different release, update
$script:AppUrl and $script:AppSha256 in BOTH build-kit.ps1 and install.ps1.


What ends up in the kit
-----------------------
  Setup.bat               entry point for the end user
  install.ps1             setup automation
  setup-windows.ps1       copy of the repository's ESP-IDF installer
  GIF.Replacer.Tool.exe   pinned release binary
  START-HERE.txt          one-page quick reference for the end user
  WINDOWS-SETUP-GUIDE.txt full step-by-step guide for the end user
  EmotionDisplay\ | .zip  private project (never commit this)


Distribution rules
------------------
- The EmotionDisplay project is confidential. Do not publish the kit, upload
  it to a public file host, or commit it to a public repository.
- The app binary is unsigned, so expect a SmartScreen warning on first run.


Verification status
-------------------
setup-windows.ps1 has never been executed on a real Windows machine (see
scripts/setup-windows-laptop-checklist.md in the repository). The first full
Setup.bat run on a clean PC is also its first real test. Record the result.


Testing the kit without installing anything
-------------------------------------------
Run the app/config/shortcut half only:

  powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1 `
      -SkipIdf -SkipProject -InstallDir "$env:TEMP\gif-replacer-kit-test" -NoShortcut

Expected: app hash verified, launch.bat created, config merged, COM port
report printed, exit code 0.
