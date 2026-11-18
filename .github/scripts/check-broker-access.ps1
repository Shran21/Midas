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
try {
    Copy-Item -LiteralPath $binary[0].executable -Destination (Join-Path $resolvedRoot 'trial.exe')
    $scriptPath = Join-Path $resolvedRoot 'run.ps1'
    @'
$ErrorActionPreference = 'Stop'
$code = 1
$account = $null
$log = Join-Path $PSScriptRoot 'trial.log'
try {
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
    $process = Start-Process -FilePath (Join-Path $PSScriptRoot 'trial.exe') -WindowStyle Hidden -Wait -PassThru `
        -ArgumentList @('--exact', 'win::remote_ipc::broker::tests::isolated_system_query_access', '--ignored', '--nocapture') `
        -RedirectStandardOutput $output -RedirectStandardError $errors
    $code = $process.ExitCode
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
$code | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'result.txt')
exit $code
'@ | Set-Content -LiteralPath $scriptPath
    $powershell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $action = New-ScheduledTaskAction -Execute $powershell -Argument ('-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + $scriptPath + '"')
    $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
    $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 3)
    Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Settings $settings | Out-Null
    $registered = $true
    Start-ScheduledTask -TaskName $taskName
    $result = Join-Path $resolvedRoot 'result.txt'
    $until = [DateTime]::UtcNow.AddMinutes(3)
    while (-not (Test-Path -LiteralPath $result) -and [DateTime]::UtcNow -lt $until) { Start-Sleep -Seconds 1 }
    $log = Join-Path $resolvedRoot 'trial.log'
    if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log }
    if (-not (Test-Path -LiteralPath $result) -or (Get-Content -LiteralPath $result -Raw).Trim() -ne '0') { throw 'SYSTEM broker access trial failed.' }
    if ((Get-Content -LiteralPath $log -Raw) -notmatch '1 passed; 0 failed; 0 ignored') { throw 'SYSTEM trial did not execute.' }
} finally {
    if ($registered) {
        Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
        Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
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
