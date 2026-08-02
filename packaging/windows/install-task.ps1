param(
    [Parameter(Mandatory)]
    [ValidateSet('Prepare', 'Install', 'Uninstall', 'Rollback')]
    [string]$Action,
    [string]$Account,
    [string]$LauncherPath,
    [string]$TemplatePath,
    [switch]$ValidateAccount
)

$ErrorActionPreference = 'Stop'
$taskName = 'KioskLauncher'
$stateDirectory = Join-Path $env:ProgramData 'kiosk\.installer-task'
$backupPath = Join-Path $stateDirectory 'KioskLauncher.xml'
$runningPath = Join-Path $stateDirectory 'was-running'
$preparedPath = Join-Path $stateDirectory 'prepared'

function Remove-Task {
    Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
}

function Remove-State {
    if (Test-Path -LiteralPath $stateDirectory) {
        Remove-Item -LiteralPath $backupPath, $runningPath, $preparedPath -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $stateDirectory -Force
    }
}

function Test-LocalGroupContainsUser {
    param(
        [string]$GroupSid,
        [string]$UserSid,
        [hashtable]$Visited
    )

    if ($Visited.ContainsKey($GroupSid)) {
        return $false
    }
    $Visited[$GroupSid] = $true

    foreach ($member in Get-LocalGroupMember -SID $GroupSid -ErrorAction Stop) {
        if ($member.SID.Value -eq $UserSid) {
            return $true
        }
        $nestedGroup = Get-LocalGroup -SID $member.SID -ErrorAction SilentlyContinue
        if ($nestedGroup -and (Test-LocalGroupContainsUser -GroupSid $nestedGroup.SID.Value -UserSid $UserSid -Visited $Visited)) {
            return $true
        }
    }
    return $false
}

if ($Action -eq 'Prepare') {
    $security = New-Object Security.AccessControl.DirectorySecurity
    $security.SetAccessRuleProtection($true, $false)
    foreach ($sidValue in 'S-1-5-18', 'S-1-5-32-544') {
        $sid = New-Object Security.Principal.SecurityIdentifier($sidValue)
        $rule = New-Object Security.AccessControl.FileSystemAccessRule($sid, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
        $security.AddAccessRule($rule)
    }
    if (Test-Path -LiteralPath $stateDirectory) {
        Set-Acl -LiteralPath $stateDirectory -AclObject $security
        Get-ChildItem -LiteralPath $stateDirectory -Force | Remove-Item -Recurse -Force
    } else {
        [IO.Directory]::CreateDirectory($stateDirectory) | Out-Null
        Set-Acl -LiteralPath $stateDirectory -AclObject $security
    }

    if ($ValidateAccount) {
        if (-not $Account) {
            throw 'KIOSK_ACCOUNT is required.'
        }
        if ($Account -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,19}$') {
            throw 'KIOSK_ACCOUNT must be a bare local account name using only letters, digits, dot, underscore, or hyphen.'
        }
        $user = Get-LocalUser -Name $Account -ErrorAction Stop
        if (-not $user.Enabled) {
            throw 'KIOSK_ACCOUNT is disabled.'
        }
        if (Test-LocalGroupContainsUser -GroupSid 'S-1-5-32-544' -UserSid $user.SID.Value -Visited @{}) {
            throw 'KIOSK_ACCOUNT must not be a local administrator.'
        }
    }

    $task = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
    if ($task) {
        [IO.File]::WriteAllText($backupPath, (Export-ScheduledTask -TaskName $taskName), (New-Object Text.UTF8Encoding($false)))
        if ($task.State -eq 'Running') {
            [IO.File]::WriteAllText($runningPath, '')
        }
    }
    [IO.File]::WriteAllText($preparedPath, '')
    if ($task) {
        Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
    }
    exit 0
}

if ($Action -eq 'Rollback') {
    if (-not (Test-Path -LiteralPath $preparedPath -PathType Leaf)) {
        Remove-State
        exit 0
    }
    Remove-Task
    if (Test-Path -LiteralPath $backupPath -PathType Leaf) {
        Register-ScheduledTask -TaskName $taskName -Xml (Get-Content -Raw -LiteralPath $backupPath) -Force | Out-Null
        if (Test-Path -LiteralPath $runningPath -PathType Leaf) {
            Start-ScheduledTask -TaskName $taskName
        }
    }
    Remove-State
    exit 0
}

if ($Action -eq 'Uninstall') {
    Remove-Task
    exit 0
}

if (-not (Test-Path -LiteralPath $LauncherPath -PathType Leaf) -or -not (Test-Path -LiteralPath $TemplatePath -PathType Leaf)) {
    throw 'Scheduled Task launcher or XML template is missing.'
}

$user = Get-LocalUser -Name $Account -ErrorAction Stop
$xml = (Get-Content -Raw -LiteralPath $TemplatePath).Replace('__KIOSK_ACCOUNT_SID__', [Security.SecurityElement]::Escape($user.SID.Value)).Replace('__LAUNCHER_PATH__', [Security.SecurityElement]::Escape($LauncherPath))
Register-ScheduledTask -TaskName $taskName -Xml $xml -Force | Out-Null
