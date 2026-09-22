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
        $c = Get-IdfCandidates -Configured 'D:\my-idf' -Environment @{} -Profile 'C:\Users\dev'
        $c[0] | Should -Be 'D:\my-idf'
    }

    It 'uses IDF_TOOLS_PATH from the environment when set' {
        $c = Get-IdfCandidates -Configured '' `
            -Environment @{ IDF_TOOLS_PATH = 'D:\tools' } -Profile 'C:\Users\dev'
        $c | Should -Contain 'D:\tools\frameworks\esp-idf-v5.5.2'
    }

    It 'uses the installer default layout' {
        $c = Get-IdfCandidates -Configured '' -Environment @{} -Profile 'C:\Users\dev'
        $c | Should -Contain 'C:\Espressif\frameworks\esp-idf-v5.5.2'
        $c | Should -Contain 'C:\Users\dev\esp\esp-idf-v5.5.2'
        $c | Should -Contain 'C:\esp\esp-idf-v5.5.2'
    }

    It 'never returns an empty list' {
        # If this is empty the "not found" message has nothing to name, and the
        # script reports a failure with no explanation of where it looked.
        (Get-IdfCandidates -Configured '' -Environment @{} -Profile 'C:\Users\dev').Count |
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
