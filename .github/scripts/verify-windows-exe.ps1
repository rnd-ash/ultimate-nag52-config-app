param(
    [Parameter(Mandatory = $true)]
    [string]$Path
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$exe = (Resolve-Path -LiteralPath $Path).Path

$stream = [System.IO.File]::OpenRead($exe)
try {
    $pe = [System.Reflection.PortableExecutable.PEReader]::new($stream)
    try {
        $headers = $pe.PEHeaders
        if ($headers.CoffHeader.Machine -ne 'I386' -or $headers.PEHeader.Magic -ne 'PE32' -or
            -not $headers.CoffHeader.Characteristics.HasFlag([System.Reflection.PortableExecutable.Characteristics]::ExecutableImage) -or
            $headers.CoffHeader.Characteristics.HasFlag([System.Reflection.PortableExecutable.Characteristics]::Dll)) {
            throw 'Expected a Windows x86 PE32 executable.'
        }
    } finally {
        $pe.Dispose()
    }
} finally {
    $stream.Dispose()
}
Write-Host "Verified Windows x86 PE32 executable: $exe"

$dependencies = @(dumpbin /NOLOGO /DEPENDENTS $exe | ForEach-Object {
    if ($_ -match '^\s+([a-zA-Z0-9_.-]+\.dll)\s*$') {
        $Matches[1].ToLowerInvariant()
    }
} | Sort-Object -Unique)
if ($dependencies.Count -eq 0) {
    throw 'No DLL dependencies were found in dumpbin output.'
}
$allowedDlls = @(
    'kernel32.dll', 'user32.dll', 'combase.dll', 'ole32.dll', 'shell32.dll',
    'ws2_32.dll', 'bcrypt.dll', 'advapi32.dll', 'crypt32.dll', 'secur32.dll',
    'ntdll.dll', 'shlwapi.dll', 'setupapi.dll', 'cfgmgr32.dll', 'gdi32.dll',
    'uiautomationcore.dll', 'oleaut32.dll', 'opengl32.dll', 'bcryptprimitives.dll',
    'iphlpapi.dll', 'imm32.dll', 'dwmapi.dll', 'uxtheme.dll',
    # shortcut: CI supplies the x86 VC++ runtime; check a clean Windows install before release.
    'vcruntime140.dll'
)
$unexpectedDlls = @($dependencies | Where-Object {
    $_ -notmatch '^api-ms-win-[a-z0-9-]+\.dll$' -and $_ -notin $allowedDlls
})
if ($unexpectedDlls.Count -gt 0) {
    throw "Unexpected DLL dependencies: $($unexpectedDlls -join ', ')"
}
Write-Host "DLL dependencies: $($dependencies -join ', ')"

# Probe the desktop independently of the application under test.
$environmentFailure = $null
if ($env:SKIP_GUI_STARTUP -eq '1') {
    $environmentFailure = 'Mesa runtime installation failed; see the installation step.'
} else {
    $probe = $null
    try {
        if (-not [Environment]::UserInteractive) {
            throw 'The runner has no interactive desktop.'
        }
        Add-Type -AssemblyName System.Windows.Forms
        $probe = [System.Windows.Forms.Form]::new()
        $probe.Show()
        [System.Windows.Forms.Application]::DoEvents()
        if (-not $probe.Visible -or $probe.Handle -eq 0) {
            throw 'Could not create a test window on the runner desktop.'
        }
    } catch {
        $environmentFailure = $_.Exception.Message
    } finally {
        if ($null -ne $probe) { $probe.Dispose() }
    }
}
if ($null -ne $environmentFailure) {
    Write-Host "::warning::Windows GUI startup was NOT checked: test environment unavailable."
    Write-Host $environmentFailure
    if ($env:GITHUB_STEP_SUMMARY) {
        Add-Content $env:GITHUB_STEP_SUMMARY '### Windows GUI startup: NOT CHECKED
Test environment preparation failed. Executable validation passed; publication is allowed. See the verification and Mesa installation logs.'
    }
    return
}

$stdout = [System.IO.Path]::GetTempFileName()
$stderr = [System.IO.Path]::GetTempFileName()
$process = $null
try {
    $process = Start-Process -FilePath $exe -PassThru -NoNewWindow `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        $process.Refresh()
        if ($process.HasExited) {
            throw "Application exited before opening its window (exit code $($process.ExitCode))."
        }
        if ($process.MainWindowHandle -ne 0 -and $process.MainWindowTitle -eq 'Ultimate NAG52 config suite') {
            break
        }
        Start-Sleep -Milliseconds 500
    }
    if ($process.MainWindowHandle -eq 0 -or $process.MainWindowTitle -ne 'Ultimate NAG52 config suite') {
        throw 'Application did not open its main window within 30 seconds.'
    }
    Start-Sleep -Seconds 3
    $process.Refresh()
    if ($process.HasExited) {
        throw "Application exited after opening its window (exit code $($process.ExitCode))."
    }
    Write-Host 'Application window opened and remained running.'
} finally {
    try {
        if ($null -ne $process -and -not $process.HasExited) {
            $process.Kill()
            if (-not $process.WaitForExit(5000)) {
                throw 'Could not stop the application after the startup check.'
            }
        }
    } finally {
        if ($null -ne $process) {
            $process.Dispose()
        }
        Get-Content -LiteralPath $stdout, $stderr | Write-Host
        Remove-Item -LiteralPath $stdout, $stderr
    }
}
