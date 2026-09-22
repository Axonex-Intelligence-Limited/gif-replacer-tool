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
