$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
    throw 'This SYSTEM trial requires a disposable GitHub-hosted Windows runner.'
}
$artifacts = & cargo test --workspace --all-targets --locked --no-run --message-format=json
if ($LASTEXITCODE -ne 0) { throw 'Cannot build the Windows test fixture.' }
$binary = @($artifacts | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object {
    $_.reason -eq 'compiler-artifact' -and $_.profile.test -and $_.target.name -eq 'midas_agent_windows'
})
if ($binary.Count -ne 1 -or -not $binary[0].executable) { throw 'Ambiguous Windows test fixture.' }
$releaseAgent = Join-Path $PSScriptRoot '..\..\target\release\midas-agent.exe'
if (-not (Test-Path -LiteralPath $releaseAgent -PathType Leaf)) { throw 'Missing agent release binary.' }
$taskName = 'MIDAS-BrokerTrial-' + [guid]::NewGuid().ToString('N')
$trialRoot = Join-Path $env:ProgramFiles $taskName
if (Test-Path -LiteralPath $trialRoot) { throw 'Trial directory already exists.' }
$resolvedRoot = [IO.Path]::GetFullPath($trialRoot)
$allowedRoot = [IO.Path]::GetFullPath($env:ProgramFiles).TrimEnd('\') + '\MIDAS-BrokerTrial-'
if (-not $resolvedRoot.StartsWith($allowedRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid trial directory.' }
New-Item -ItemType Directory -Path $resolvedRoot | Out-Null
$acl = [Security.AccessControl.DirectorySecurity]::new()
$acl.SetAccessRuleProtection($true, $false)
foreach ($entry in @(@('S-1-5-18', 'FullControl'), @('S-1-5-32-544', 'FullControl'), @('S-1-5-32-545', 'ReadAndExecute'))) {
    $sid = [Security.Principal.SecurityIdentifier]::new($entry[0])
    $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid, $entry[1], 'ContainerInherit,ObjectInherit', 'None', 'Allow'))
}
Set-Acl -LiteralPath $resolvedRoot -AclObject $acl
$registered = $false
$systemHost = $null
try {
    Copy-Item -LiteralPath $binary[0].executable -Destination (Join-Path $resolvedRoot 'trial.exe')
    Copy-Item -LiteralPath $releaseAgent -Destination (Join-Path $resolvedRoot 'midas-agent.exe')
    $fileAcl = [Security.AccessControl.FileSecurity]::new()
    $fileAcl.SetAccessRuleProtection($true, $false)
    foreach ($entry in @(@('S-1-5-18', 'FullControl'), @('S-1-5-32-544', 'FullControl'), @('S-1-5-32-545', 'ReadAndExecute'))) {
        $fileAcl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($entry[0]), $entry[1], 'Allow'))
    }
    Set-Acl -LiteralPath (Join-Path $resolvedRoot 'trial.exe') -AclObject $fileAcl
    Set-Acl -LiteralPath (Join-Path $resolvedRoot 'midas-agent.exe') -AclObject $fileAcl
    $scriptPath = Join-Path $resolvedRoot 'run.ps1'
    @'
$ErrorActionPreference = 'Stop'
$code = 1
$account = $null
$log = Join-Path $PSScriptRoot 'trial.log'
try {
    $until = [DateTime]::UtcNow.AddSeconds(30)
    while (-not (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'host-ready')) -and [DateTime]::UtcNow -lt $until) { Start-Sleep -Milliseconds 100 }
    if (-not (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'host-ready'))) { throw 'CI did not retain the SYSTEM host.' }
    $name = 'MidasR' + [guid]::NewGuid().ToString('N').Substring(0, 10)
    $secret = 'Midas!' + [guid]::NewGuid().ToString('N') + 'A1'
    $account = New-LocalUser -Name $name -Password (ConvertTo-SecureString $secret -AsPlainText -Force)
    $account.SID.Value | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'account-sid.txt')
    Add-LocalGroupMember -SID 'S-1-5-32-545' -Member $account
    $env:MIDAS_SYSTEM_TRIAL = '1'
    $env:GITHUB_ACTIONS = 'true'
    $env:MIDAS_SYSTEM_TRIAL_USER = $name
    $env:MIDAS_SYSTEM_TRIAL_PASSWORD = $secret
    $output = Join-Path $PSScriptRoot 'trial.stdout'
    $errors = Join-Path $PSScriptRoot 'trial.stderr'
    $process = Start-Process -FilePath (Join-Path $PSScriptRoot 'trial.exe') -WindowStyle Hidden -PassThru `
        -ArgumentList @('--exact', 'win::remote_ipc::broker::tests::isolated_system_query_access', '--ignored', '--nocapture') `
        -RedirectStandardOutput $output -RedirectStandardError $errors
    $null = $process.Handle
    $process.Id | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'native-pid.txt')
    if (-not $process.WaitForExit(120000)) { $process.Kill(); $process.WaitForExit(); throw 'Native trial timed out.' }
    $process.WaitForExit()
    $exitCode = $process.ExitCode
    if ($null -eq $exitCode) { throw 'Native process exit code unavailable.' }
    $code = [int]$exitCode
    Get-Content -LiteralPath $output, $errors | Set-Content -LiteralPath $log
    "Test process exit code: $code" | Add-Content -LiteralPath $log
} catch {
    $_.Exception.Message | Add-Content -LiteralPath $log
} finally {
    Remove-Item Env:MIDAS_SYSTEM_TRIAL_PASSWORD -ErrorAction SilentlyContinue
    $secret = $null
    if ($account) {
        try { Remove-LocalUser -SID $account.SID } catch { $code = 1; $_.Exception.Message | Add-Content -LiteralPath $log }
    }
}
$code | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'result.tmp')
Move-Item -LiteralPath (Join-Path $PSScriptRoot 'result.tmp') -Destination (Join-Path $PSScriptRoot 'result.txt')
exit $code
'@ | Set-Content -LiteralPath $scriptPath
    $launcherPath = Join-Path $resolvedRoot 'launch.ps1'
    @'
$ErrorActionPreference = 'Stop'
try {
    $powershell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $script = Join-Path $PSScriptRoot 'run.ps1'
    $startup = New-CimInstance -ClassName Win32_ProcessStartup -ClientOnly -Property @{ ShowWindow = [uint16]0; CreateFlags = [uint32]16777216 }
    $created = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{
        CommandLine = ('"' + $powershell + '" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + $script + '"')
        ProcessStartupInformation = $startup
    }
    if ($created.ReturnValue -ne 0) { throw "SYSTEM host creation failed: $($created.ReturnValue)" }
    $created.ProcessId | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'host-pid.tmp')
    Move-Item -LiteralPath (Join-Path $PSScriptRoot 'host-pid.tmp') -Destination (Join-Path $PSScriptRoot 'host-pid.txt')
} catch {
    $_.Exception.Message | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'trial.log')
    '1' | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'result.tmp')
    Move-Item -LiteralPath (Join-Path $PSScriptRoot 'result.tmp') -Destination (Join-Path $PSScriptRoot 'result.txt')
}
'@ | Set-Content -LiteralPath $launcherPath
    $powershell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $action = New-ScheduledTaskAction -Execute $powershell -Argument ('-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + $launcherPath + '"')
    $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
    $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 3)
    Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Settings $settings | Out-Null
    $registered = $true
    Start-ScheduledTask -TaskName $taskName
    $result = Join-Path $resolvedRoot 'result.txt'
    $until = [DateTime]::UtcNow.AddMinutes(3)
    while (-not (Test-Path -LiteralPath $result) -and [DateTime]::UtcNow -lt $until) {
        $hostRecord = Join-Path $resolvedRoot 'host-pid.txt'
        if (-not $systemHost -and (Test-Path -LiteralPath $hostRecord)) {
            $systemHost = Get-Process -Id ([int](Get-Content -LiteralPath $hostRecord -Raw).Trim())
            $null = $systemHost.SafeHandle
            if ($systemHost.MainModule.FileName -ne $powershell) { throw 'Unexpected SYSTEM host image.' }
            'ready' | Set-Content -LiteralPath (Join-Path $resolvedRoot 'host-ready')
        }
        Start-Sleep -Milliseconds 200
    }
    $log = Join-Path $resolvedRoot 'trial.log'
    if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log }
    if (-not (Test-Path -LiteralPath $result) -or ([string](Get-Content -LiteralPath $result -Raw)).Trim() -ne '0') { throw 'SYSTEM broker access trial failed.' }
    if ((Get-Content -LiteralPath $log -Raw) -notmatch '1 passed; 0 failed; 0 ignored') { throw 'SYSTEM trial did not execute.' }
} finally {
    if ($registered) {
        Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
        Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
    }
    if ($systemHost) {
        if (-not $systemHost.WaitForExit(5000)) {
            $nativeRecord = Join-Path $resolvedRoot 'native-pid.txt'
            if (Test-Path -LiteralPath $nativeRecord) {
                $native = Get-Process -Id ([int](Get-Content -LiteralPath $nativeRecord -Raw).Trim()) -ErrorAction SilentlyContinue
                if ($native) {
                    $null = $native.SafeHandle
                    if ($native.MainModule.FileName -ne (Join-Path $resolvedRoot 'trial.exe')) { throw 'Unexpected native fixture image.' }
                    $native.Kill()
                    if (-not $native.WaitForExit(5000)) { throw 'Native fixture did not stop.' }
                    $native.Dispose()
                }
            }
            $systemHost.Kill()
            if (-not $systemHost.WaitForExit(5000)) { throw 'SYSTEM host did not stop.' }
        }
        $systemHost.Dispose()
    }
    $accountFile = Join-Path $resolvedRoot 'account-sid.txt'
    if (Test-Path -LiteralPath $accountFile) {
        $accountSid = [Security.Principal.SecurityIdentifier]::new((Get-Content -LiteralPath $accountFile -Raw).Trim())
        $remainingAccount = Get-LocalUser -SID $accountSid -ErrorAction SilentlyContinue
        if ($remainingAccount) {
            if ($remainingAccount.Name -notmatch '^MidasR[0-9a-f]{10}$') { throw 'Unexpected fixture account.' }
            Remove-LocalUser -SID $accountSid
        }
    }
    $resolved = (Resolve-Path -LiteralPath $resolvedRoot).Path
    if ($resolved -ne $resolvedRoot -or -not $resolved.StartsWith($allowedRoot, [StringComparison]::OrdinalIgnoreCase) -or
        ((Get-Item -LiteralPath $resolved).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Refusing unexpected cleanup path.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
