# Windows setup script — laptop verification checklist

`scripts/setup-windows.ps1` was written on macOS. Its pure logic is covered by 39 Pester tests,
but **the platform-touching half has never been executed** — no part of it has run on Windows, and
it cannot: `Get-CimInstance`, `Get-PSDrive`, `cmd.exe`, the ESP-IDF installer and the registry are
all absent from the machine it was written on.

Until every item below is run on a real Windows machine, the script is **unproven**. PSScriptAnalyzer
returning clean and 39 tests passing says the logic is right; it says nothing about whether this
works.

Record the actual output next to each item. Do not mark this file verified until all ten are done.

---

## Before you start

- A Windows 10 or 11 machine, ideally one that has **never** had ESP-IDF on it. A machine that
  already has ESP-IDF exercises the "already installed" branch and silently skips the hard part.
- ~4 GB free disk and a working internet connection.
- The board and its USB cable, for items 6 and 10.

```powershell
Invoke-WebRequest -Uri https://raw.githubusercontent.com/Axonex-Intelligence-Limited/gif-replacer-tool/main/scripts/setup-windows.ps1 -OutFile setup-windows.ps1
Unblock-File .\setup-windows.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\setup-windows.ps1
```

---

## 1. Preflight

**Expect:** `ok  64-bit Windows, enough free disk`.

**Actually:** _

**Watch for:** the "Drive C: does not exist" path is guarded, but it has never been exercised on a
machine where the drive *does* exist. Confirm the free-space number is plausible for the drive
`-IdfDir` points at.

---

## 2. Download completes and length verifies

**Expect:** `ok  Downloaded and length-verified`, after a ~1.6 GB download.

**Actually:** _

**Watch for:** the truncation guard. If the download is interrupted, the script must print
`FAIL: Download is <n> bytes, expected 1577539256` and delete the `.part` file. Interrupt one
deliberately (disable the network mid-download) and confirm — this is the single most valuable
check here, because a silently-truncated 1.6 GB file is what the guard exists to catch.

---

## 3. The installer runs silently

**Expect:** no installer window appears. `Wait-ProcessGone` returns, and `==> Installing` is
followed by `ok  Installed ESP-IDF v5.5.2`.

**Actually:** _

**Watch for:** Espressif's docs warn the installer **detaches its process**. The dangerous outcome
is `Wait-ProcessGone` returning early and the script proceeding while the install is still running.
Check by comparing the log's last-write timestamp against the moment the script moved on. If they
disagree, the wait is broken.

Also confirm: **did it request admin?** The script is supposed to need none. A UAC prompt here
contradicts the design in the spec's §5.4 and must be recorded.

---

## 4. Where the install actually landed

**Expect:** the script finds `export.bat` afterwards and continues.

**Actually:** _ (write the real path here)

**Watch for:** this resolves spec §8.1, which is still open. `idf_tools_path_for()` in `builder.rs`
only derives `IDF_TOOLS_PATH` when the checkout's **parent is named `frameworks`**. The legacy
installer may not produce that layout. The script works either way because step 4 writes `idf_path`
into the config, but record where it landed — it decides whether that Rust function is exercised on
Windows at all.

---

## 5. `idf.py --version` reports 5.5.2

**Expect:** `PASS: ESP-IDF v5.5.2` and exit code 0.

**Actually:** _

Verify the exit code explicitly — `echo $LASTEXITCODE` after the run. A `PASS` line with a non-zero
exit is a contradiction the contract forbids.

---

## 6. Bridge detection

**Expect:**
- Board **unplugged** → `No USB-serial bridge detected`, and the script still completes.
- Board **plugged in** → either `CH340 is attached` (with the WCH page printed) or
  `CH340 detected and enumerated by Windows`.

**Actually:** _

**Watch for:** which chip it reports. The developer manual says "CH340/CP210x" and this script
guesses neither — it reports what the hardware IDs say. If it reports CP210x or FTDI, the manual is
wrong and the CH340 note should be corrected.

Also confirm Device Manager shows the port **without** a warning triangle. If it has one, install
WCH's driver from <https://www.wch-ic.com/downloads/CH341SER.EXE.html> and confirm the port then
appears in the app's Scan list.

---

## 7. The config is written without clobbering

**Expect:** `%USERPROFILE%\.gif-tool-config.json` contains `idf_path`, and any pre-existing
`project_path` and `last_serial_port` survive.

**Actually:** _

**Watch for:** run the app **first** on this machine so the config already exists with a real
`project_path`, then run the script and diff. Testing against a non-existent config misses the
merge bug this is meant to catch.

---

## 8. A second run is a fast no-op

**Expect:** the script skips the download, prints `-- ESP-IDF already at <path> (already done)`, and
finishes in seconds.

**Actually:** _

**Watch for:** this is the idempotency claim. A re-run that re-downloads 1.6 GB means
`Find-IdfPath` and the installer's output layout disagree.

---

## 9. The app finds the install and builds

**Expect:** launch `GIF Replacer Tool.exe`, load a profile, click Build, and reach a successful
`idf.py build`.

**Actually:** _

**Watch for:** if the app reports `ESP-IDF v5.5.2 not found`, it lists every path it probed —
compare that list against what step 4 recorded. A mismatch means the config write did not take, or
`resolve_idf_path` is not consulting `idf_path` first.

---

## 10. A flash reaches the board over `COM<n>`

**Expect:** the port appears in Scan, esptool opens it, and the flash completes.

**Actually:** _

**Watch for:** the Windows-specific guard. Unplugging the board after the Scan but before clicking
Flash must report the port as missing — not surface an esptool error. Then, with another program
holding the port (PuTTY, `idf.py monitor`), the failure output must carry the Windows hint
("Close any program holding the port"), **not** the `dialout` hint, which is Unix-only.

---

## Result

| Item | Result | Notes |
| --- | --- | --- |
| 1. Preflight | | |
| 2. Download + integrity | | |
| 3. Silent install | | |
| 4. Install layout | | |
| 5. `idf.py --version` | | |
| 6. Bridge detection | | |
| 7. Config merge | | |
| 8. Idempotent re-run | | |
| 9. App build | | |
| 10. Flash over `COM<n>` | | |

When all ten pass, delete the `VERIFICATION STATUS` block from `setup-windows.ps1`'s header and
replace it with what was actually run, the way `setup-macos.sh` records its own status.
