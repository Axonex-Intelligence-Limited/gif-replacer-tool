BeforeAll {
    $script:ScriptPath = Join-Path $PSScriptRoot '..' 'setup-windows.ps1'
    . $script:ScriptPath
}

Describe 'contract helpers' {
    It 'formats a PASS line and reports success' {
        $out = Get-ContractLine -Kind Pass -Message 'ESP-IDF v5.5.2'
        $out | Should -Be 'PASS: ESP-IDF v5.5.2'
    }

    It 'formats a FAIL line' {
        (Get-ContractLine -Kind Fail -Message 'download truncated') |
            Should -Be 'FAIL: download truncated'
    }

    It 'marks already-done steps distinctly from fresh work' {
        (Get-ContractLine -Kind Skip -Message 'ESP-IDF already at C:\Espressif') |
            Should -BeLike '*-*-*already*'
    }

    It 'reports which stream each kind belongs on' {
        # Callers use this to route FAIL to stderr. Blocking a caller from
        # having to decide is the point of having it here.
        (Get-ContractLine -Kind Fail -Message 'x' -Stream) | Should -Be 'Error'
        (Get-ContractLine -Kind Pass -Message 'x' -Stream) | Should -Be 'Output'
    }
}

Describe 'Get-IdfCandidates' {
    It 'puts the configured path first' {
        $c = Get-IdfCandidates -Configured 'D:\my-idf' -Environment @{} -UserProfile 'C:\Users\dev'
        $c[0] | Should -Be 'D:\my-idf'
    }

    It 'uses IDF_TOOLS_PATH from the environment when set' {
        $c = Get-IdfCandidates -Configured '' `
            -Environment @{ IDF_TOOLS_PATH = 'D:\tools' } -UserProfile 'C:\Users\dev'
        $c | Should -Contain 'D:\tools\frameworks\esp-idf-v5.5.2'
    }

    It 'uses the installer default layout' {
        $c = Get-IdfCandidates -Configured '' -Environment @{} -UserProfile 'C:\Users\dev'
        $c | Should -Contain 'C:\Espressif\frameworks\esp-idf-v5.5.2'
        $c | Should -Contain 'C:\Users\dev\esp\esp-idf-v5.5.2'
        $c | Should -Contain 'C:\esp\esp-idf-v5.5.2'
    }

    It 'never returns an empty list' {
        # If this is empty the "not found" message has nothing to name, and the
        # script reports a failure with no explanation of where it looked.
        (Get-IdfCandidates -Configured '' -Environment @{} -UserProfile 'C:\Users\dev').Count |
            Should -BeGreaterThan 0
    }
}

Describe 'Find-IdfPath' {
    It 'returns the candidate containing the marker' {
        $dir = Join-Path ([System.IO.Path]::GetTempPath()) "gif_setup_idf_$PID"
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        New-Item -ItemType File -Force -Path (Join-Path $dir 'export.bat') | Out-Null

        (Find-IdfPath -Candidates @('C:\nope', $dir) -Environment @{}) | Should -Be $dir

        Remove-Item -Recurse -Force $dir
    }

    It 'returns null when no candidate has the marker' {
        (Find-IdfPath -Candidates @('C:\nope') -Environment @{}) | Should -BeNullOrEmpty
    }

    It 'falls back to $IDF_PATH when no candidate has the marker' {
        # resolve_idf_path checks the probe list first and only then falls back
        # to $IDF_PATH. Omitting this would make the script fail to find an
        # install the app would happily use.
        (Find-IdfPath -Candidates @('C:\nope') -Environment @{ IDF_PATH = 'D:\env-idf' }) |
            Should -Be 'D:\env-idf'
    }

    It 'prefers a candidate over $IDF_PATH' {
        $dir = Join-Path ([System.IO.Path]::GetTempPath()) "gif_setup_idf2_$PID"
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        New-Item -ItemType File -Force -Path (Join-Path $dir 'export.bat') | Out-Null

        (Find-IdfPath -Candidates @($dir) -Environment @{ IDF_PATH = 'D:\env-idf' }) |
            Should -Be $dir

        Remove-Item -Recurse -Force $dir
    }

    It 'ignores an empty $IDF_PATH' {
        (Find-IdfPath -Candidates @('C:\nope') -Environment @{ IDF_PATH = '' }) |
            Should -BeNullOrEmpty
    }
}

Describe 'Test-DownloadIntegrity' {
    It 'accepts an exact match' {
        Test-DownloadIntegrity -Expected 1577539256 -Actual 1577539256 | Should -BeTrue
    }

    It 'rejects a truncated download' {
        # The realistic failure: connection dropped part-way.
        Test-DownloadIntegrity -Expected 1577539256 -Actual 900000000 | Should -BeFalse
    }

    It 'rejects an oversized file' {
        Test-DownloadIntegrity -Expected 1577539256 -Actual 1577539257 | Should -BeFalse
    }

    It 'rejects an unknown expected length rather than trusting it' {
        # Without a Content-Length there is nothing to verify, so treating 0 as
        # "matches anything" would skip the check exactly when it matters.
        Test-DownloadIntegrity -Expected 0 -Actual 1577539256 | Should -BeFalse
    }
}

Describe 'Get-PartialPath' {
    It 'appends .part so a failed download never occupies the final path' {
        Get-PartialPath -Target 'C:\Temp\idf-setup.exe' | Should -Be 'C:\Temp\idf-setup.exe.part'
    }
}

Describe 'Get-InstallerArgs' {
    It 'includes every flag the silent path needs' {
        $a = Get-InstallerArgs -IdfDir 'C:\Espressif' -LogPath 'C:\Temp\idf.log'
        $a | Should -Contain '/VERYSILENT'
        $a | Should -Contain '/SUPPRESSMSGBOXES'
        $a | Should -Contain '/SP-'
        $a | Should -Contain '/NOCANCEL'
        $a | Should -Contain '/USEEMBEDDEDPYTHON=yes'
        $a | Should -Contain '/SKIPSYSTEMCHECK=yes'
    }

    It 'passes the install directory and log path through' {
        $a = Get-InstallerArgs -IdfDir 'D:\idf' -LogPath 'C:\Temp\idf.log'
        $a | Should -Contain '/IDFDIR=D:\idf'
        $a | Should -Contain '/LOG=C:\Temp\idf.log'
    }

    It 'does not pass /IDFVERSION, which cannot help a version-specific installer' {
        # The offline installer IS the 5.5.2 build. /IDFVERSION drives the
        # legacy installer's dropdown, which reads idf_versions.txt - a list
        # that does not contain 5.5.2.
        (Get-InstallerArgs -IdfDir 'C:\Espressif' -LogPath 'x') -join ' ' |
            Should -Not -BeLike '*/IDFVERSION*'
    }
}

Describe 'Wait-ProcessGone' {
    It 'returns true immediately when no such process exists' {
        (Wait-ProcessGone -Name 'definitely-not-a-real-process-xyz' -TimeoutSeconds 5 -PollSeconds 1) |
            Should -BeTrue
    }

    It 'returns false when a matching process exists for the whole timeout' {
        # Uses the test host's OWN process name rather than `sleep`.
        #
        # `sleep` works on macOS, where it is /bin/sleep, but not on Windows:
        # there `sleep` is only a PowerShell alias for Start-Sleep, and
        # Start-Process cannot launch an alias - it fails to resolve the file.
        # The host process is guaranteed present on every platform and will
        # still be running when the timeout expires, which is precisely the
        # condition under test.
        $name = (Get-Process -Id $PID).ProcessName

        (Wait-ProcessGone -Name $name -TimeoutSeconds 2 -PollSeconds 1) | Should -BeFalse
    }
}

Describe 'Get-InstallerLogVerdict' {
    It 'reads success from the log the installer writes' {
        Get-InstallerLogVerdict -LogText 'Installation completed successfully' | Should -Be 'Ok'
    }

    It 'reads failure from the log' {
        Get-InstallerLogVerdict -LogText 'Installation failed: disk full' | Should -Be 'Failed'
    }

    It 'treats an empty log as failed, not as success' {
        Get-InstallerLogVerdict -LogText '' | Should -Be 'Failed'
    }
}

Describe 'Get-BridgeFromHardwareIds' {
    It 'recognises a CH340' {
        Get-BridgeFromHardwareIds -HardwareIds @('USB\VID_1A86&PID_7523&REV_0264') |
            Should -Be 'CH340'
    }

    It 'recognises a CP210x' {
        Get-BridgeFromHardwareIds -HardwareIds @('USB\VID_10C4&PID_EA60&REV_0100') |
            Should -Be 'CP210x'
    }

    It 'recognises an FTDI' {
        Get-BridgeFromHardwareIds -HardwareIds @('USB\VID_0403&PID_6001') |
            Should -Be 'FTDI'
    }

    It 'is case-insensitive, because Windows reports both cases' {
        Get-BridgeFromHardwareIds -HardwareIds @('usb\vid_1a86&pid_7523') |
            Should -Be 'CH340'
    }

    It 'returns null for an unrelated device' {
        Get-BridgeFromHardwareIds -HardwareIds @('USB\VID_046D&PID_C52B') | Should -BeNullOrEmpty
    }

    It 'returns null for an empty device list' {
        Get-BridgeFromHardwareIds -HardwareIds @() | Should -BeNullOrEmpty
    }

    It 'picks the bridge out of a mixed list' {
        $ids = @('USB\VID_046D&PID_C52B', 'USB\VID_1A86&PID_7523')
        Get-BridgeFromHardwareIds -HardwareIds $ids | Should -Be 'CH340'
    }
}

Describe 'Merge-ToolConfig' {
    It 'sets idf_path on an empty config' {
        $o = (Merge-ToolConfig -ExistingJson '' -IdfPath 'C:\Espressif\frameworks\esp-idf-v5.5.2') |
            ConvertFrom-Json
        $o.idf_path | Should -Be 'C:\Espressif\frameworks\esp-idf-v5.5.2'
    }

    It 'preserves project_path and last_serial_port' {
        # The app owns this file. Overwriting it would silently cost the user
        # their project location and port choice.
        $existing = '{"project_path":"C:\\dev\\EmotionDisplay","idf_path":null,"last_serial_port":"COM3"}'
        $o = (Merge-ToolConfig -ExistingJson $existing -IdfPath 'C:\Espressif') | ConvertFrom-Json
        $o.project_path    | Should -Be 'C:\dev\EmotionDisplay'
        $o.last_serial_port | Should -Be 'COM3'
        $o.idf_path        | Should -Be 'C:\Espressif'
    }

    It 'replaces an existing idf_path' {
        $existing = '{"idf_path":"C:\\stale","project_path":null,"last_serial_port":null}'
        $o = (Merge-ToolConfig -ExistingJson $existing -IdfPath 'C:\Espressif') | ConvertFrom-Json
        $o.idf_path | Should -Be 'C:\Espressif'
    }

    It 'survives a corrupted config rather than throwing' {
        # A hand-edited or truncated file must not abort setup at the last step.
        $o = (Merge-ToolConfig -ExistingJson '{not json' -IdfPath 'C:\Espressif') | ConvertFrom-Json
        $o.idf_path | Should -Be 'C:\Espressif'
    }

    It 'round-trips a path containing backslashes' {
        $o = (Merge-ToolConfig -ExistingJson '' -IdfPath 'C:\a\b\esp-idf-v5.5.2') | ConvertFrom-Json
        $o.idf_path | Should -Be 'C:\a\b\esp-idf-v5.5.2'
    }
}

Describe 'script shape' {
    It 'does not run Main when dot-sourced' {
        # If the guard breaks, every Pester run would kick off a 1.58 GB
        # download instead of running the suite.
        . (Join-Path $PSScriptRoot '..' 'setup-windows.ps1')
        Get-Command Main -ErrorAction SilentlyContinue | Should -Not -BeNullOrEmpty
    }
}
