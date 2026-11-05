# The Windows loop's exit condition. Prints one PASS, FAIL or INCONCLUSIVE line per check and exits 0
# only when every line is PASS. Runs on Windows PowerShell 5.1.
#
# To add a check, call Invoke-Check (for a command whose exit code decides) or Write-Result (for anything
# else) below the existing ones. Keep one line per check.
$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
Set-Location $workspace
$logs = Join-Path $workspace 'target\verify'
New-Item -ItemType Directory -Force -Path $logs | Out-Null
$script:failed = $false

function Write-Result([string]$Status, [string]$Check, [string]$Detail) {
    $line = '{0,-12} {1}' -f $Status, $Check
    if ($Detail) { $line += "  ($Detail)" }
    Write-Output $line
    if ($Status -ne 'PASS') { $script:failed = $true }
}

function Show-Log([string]$Log, [string]$Pattern = '.') {
    Get-Content $Log | Select-String -Pattern $Pattern | Select-Object -Last 25 | ForEach-Object { "    | $_" }
}

# cmd.exe does the redirection: Windows PowerShell 5.1 wraps a native command's stderr lines in error records.
function Invoke-Logged([string]$Log, [string]$Command) {
    cmd /c "$Command > `"$Log`" 2>&1"
    return $LASTEXITCODE
}

function Invoke-Check([string]$Check, [string]$Command, [scriptblock]$Detail, [string]$FailPattern = '.') {
    $log = Join-Path $logs "$Check.log"
    $exit = Invoke-Logged $log $Command
    if ($exit -eq 0) {
        Write-Result PASS $Check (& $Detail $log)
    } else {
        Write-Result FAIL $Check "exit $exit, $log"
        Show-Log $log $FailPattern
    }
}

Invoke-Check build 'cargo build --workspace --release' { 'release, warnings denied by the workspace lints' } '^(error|warning)'
$built = -not $script:failed

Invoke-Check clippy 'cargo clippy --workspace --all-targets -- -D warnings' { 'all targets, -D warnings' } '^(error|warning)'

Invoke-Check test 'cargo test --workspace -- --show-output' {
    param($log)
    $passed = 0
    Get-Content $log | Select-String 'test result: ok\. (\d+) passed' | ForEach-Object { $passed += [int]$_.Matches[0].Groups[1].Value }
    $checks = Get-Content $log | Select-String '^model check ' | ForEach-Object { $_.Line -replace '^model check ', '' }
    (@("$passed tests passed") + $checks) -join '; '
} '(FAILED|panicked|invariant broken|reached by|error)'

# The end-to-end check drives the release build with real mouse input over a stand-in game window that covers the
# primary monitor for about a minute. It writes the app's pid before its first check, so a run that dies midway
# still gets its app killed here.
$app = Join-Path $workspace 'target\release\btc-ticker.exe'
$e2e = Join-Path $workspace 'target\release\btc-ticker-e2e.exe'
if ($built -and (Test-Path $app) -and (Test-Path $e2e)) {
    $pidFile = Join-Path $logs 'e2e.pid'
    Remove-Item $pidFile -ErrorAction SilentlyContinue
    $log = Join-Path $logs 'e2e.log'
    $exit = Invoke-Logged $log "`"$e2e`" `"$app`" `"$logs`" `"$pidFile`""
    $leftovers = @()
    if (Test-Path $pidFile) {
        $leftovers += Get-Process -Id ([int](Get-Content $pidFile)) -ErrorAction SilentlyContinue | ForEach-Object { $_.Id }
    }
    $standIns = Get-CimInstance Win32_Process -Filter "Name = 'Wow.exe' or Name = 'btc-ticker-e2e.exe'" |
        Where-Object { $_.ExecutablePath -like (Join-Path $env:TEMP 'btcticker-e2e-*') -or $_.ExecutablePath -eq $e2e }
    $leftovers += $standIns | ForEach-Object { $_.ProcessId }
    foreach ($leftover in $leftovers) { taskkill /PID $leftover /T /F | Out-Null }
    if ($leftovers) { Write-Result FAIL e2e-cleanup "pids $($leftovers -join ', ') were still running after e2e, killed them" }
    $results = @(Get-Content $log | Select-String '^(PASS|FAIL|INCONCLUSIVE) ' | ForEach-Object { $_.Line })
    if ($results.Count -eq 0) {
        Write-Result FAIL e2e "no results, exit $exit, $log"
        Show-Log $log
    } else {
        $results
        if ($results | Where-Object { $_ -notmatch '^PASS ' }) {
            $script:failed = $true
        } elseif ($exit -ne 0) {
            Write-Result FAIL e2e "every line passed but the check exited $exit, $log"
        }
    }
} else {
    Write-Result INCONCLUSIVE e2e 'no release build to launch'
}

if ($script:failed) { exit 1 }
exit 0
