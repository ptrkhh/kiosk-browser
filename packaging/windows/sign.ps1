[CmdletBinding(DefaultParameterSetName = 'Thumbprint')]
param(
    [Parameter(Mandatory)]
    [ValidateSet('Binaries', 'Installers')]
    [string]$Stage,

    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string[]]$Path,

    [string[]]$NewerThan,

    [Parameter(Mandatory)]
    [uri]$TimestampUrl,

    [Parameter(Mandatory, ParameterSetName = 'Pfx')]
    [string]$PfxPath,

    [Parameter(ParameterSetName = 'Pfx')]
    [ValidateNotNullOrEmpty()]
    [string]$PfxPasswordEnvironmentVariable = 'KIOSK_SIGNING_PFX_PASSWORD',

    [Parameter(Mandatory, ParameterSetName = 'Thumbprint')]
    [string]$Thumbprint,

    [string]$SignToolPath
)

$ErrorActionPreference = 'Stop'

function Resolve-File([string]$Value, [string]$Description) {
    if (-not (Test-Path -LiteralPath $Value -PathType Leaf)) {
        throw "$Description not found: $Value"
    }
    (Resolve-Path -LiteralPath $Value).Path
}

function Test-CodeSigningCertificate($Certificate, [switch]$RequirePrivateKey) {
    $now = Get-Date
    (-not $RequirePrivateKey -or $Certificate.HasPrivateKey) -and
        $Certificate.NotBefore -le $now -and
        $Certificate.NotAfter -gt $now -and
        ($Certificate.EnhancedKeyUsageList | Where-Object { [string]$_.ObjectId -eq '1.3.6.1.5.5.7.3.3' })
}

if ($TimestampUrl.Scheme -ne 'https') {
    throw 'TimestampUrl must use HTTPS.'
}

$targets = @($Path | ForEach-Object { Resolve-File $_ 'Signing target' })
$extensions = @($targets | ForEach-Object { [IO.Path]::GetExtension($_).ToLowerInvariant() })
if ($Stage -eq 'Binaries') {
    $names = @($targets | ForEach-Object { [IO.Path]::GetFileName($_) })
    if ($names.Count -ne 2 -or 'kiosk-main.exe' -notin $names -or 'kiosk-launcher.exe' -notin $names) {
        throw 'Binaries stage requires exactly kiosk-main.exe and kiosk-launcher.exe.'
    }
}
if ($Stage -eq 'Installers' -and ($extensions | Where-Object { $_ -notin '.msi', '.exe' })) {
    throw 'Installers stage accepts only .msi and Burn .exe files.'
}
if ($Stage -eq 'Installers') {
    if (-not $NewerThan) {
        throw 'Installers stage requires -NewerThan dependency paths to reject stale output.'
    }
    $newestDependency = $NewerThan |
        ForEach-Object { Get-Item -LiteralPath (Resolve-File $_ 'Installer dependency') } |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1
    foreach ($target in $targets) {
        if ((Get-Item -LiteralPath $target).LastWriteTimeUtc -lt $newestDependency.LastWriteTimeUtc) {
            throw "Signing target is older than dependency $($newestDependency.FullName): $target"
        }
    }
}

$certificateArgs = @()
$password = $null
$securePassword = $null
$importedCertificates = @()
$newThumbprints = @()
$existingThumbprints = @()
$signingCertificates = @()
$certificate = $null
$pfxData = $null
$pfxLeaf = $null
$pfxLeaves = @()
$collision = @()
$importedCertificate = $null

try {
    if ($PSCmdlet.ParameterSetName -eq 'Pfx') {
        $resolvedPfx = Resolve-File $PfxPath 'PFX certificate'
        $password = [Environment]::GetEnvironmentVariable($PfxPasswordEnvironmentVariable)
        if ([string]::IsNullOrEmpty($password)) {
            throw "PFX password environment variable is absent or empty: $PfxPasswordEnvironmentVariable"
        }
        $securePassword = ConvertTo-SecureString $password -AsPlainText -Force
        $pfxData = Get-PfxData -FilePath $resolvedPfx -Password $securePassword
        $pfxLeaves = @($pfxData.EndEntityCertificates | Where-Object { Test-CodeSigningCertificate $_ })
        if ($pfxLeaves.Count -ne 1) {
            throw "PFX must contain exactly one current code-signing leaf; found $($pfxLeaves.Count)."
        }
        $pfxLeaf = $pfxLeaves[0]
        $existingThumbprints = @(Get-ChildItem Cert:\CurrentUser\My | ForEach-Object { $_.Thumbprint })
        $collision = @(Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Thumbprint -eq $pfxLeaf.Thumbprint })
        if ($collision.Count) {
            if ($collision.Count -ne 1 -or -not (Test-CodeSigningCertificate $collision[0] -RequirePrivateKey)) {
                throw "PFX leaf collides with a CurrentUser certificate lacking a usable private key: $($pfxLeaf.Thumbprint)"
            }
            $signingCertificates = @($collision[0])
        } else {
            $importedCertificates = @(Import-PfxCertificate -FilePath $resolvedPfx -CertStoreLocation Cert:\CurrentUser\My -Password $securePassword -Exportable:$false)
            $newThumbprints = @($importedCertificates |
                Where-Object { $_.Thumbprint -notin $existingThumbprints } |
                Select-Object -ExpandProperty Thumbprint -Unique)
            $signingCertificates = @($importedCertificates |
                Where-Object { $_.Thumbprint -eq $pfxLeaf.Thumbprint -and (Test-CodeSigningCertificate $_ -RequirePrivateKey) })
            if ($signingCertificates.Count -ne 1) {
                throw "Imported PFX leaf lacks exactly one usable private key: $($pfxLeaf.Thumbprint)"
            }
        }
        $certificateArgs = @('/sha1', $signingCertificates[0].Thumbprint)
    } else {
        $normalizedThumbprint = $Thumbprint -replace '\s', ''
        if ($normalizedThumbprint -notmatch '^[0-9A-Fa-f]{40}$') {
            throw 'Thumbprint must contain exactly 40 hexadecimal characters.'
        }
        $certificate = Get-ChildItem Cert:\CurrentUser\My, Cert:\LocalMachine\My |
            Where-Object { $_.Thumbprint -eq $normalizedThumbprint -and (Test-CodeSigningCertificate $_ -RequirePrivateKey) } |
            Select-Object -First 1
        if (-not $certificate) {
            throw "Current code-signing certificate with private key not found: $normalizedThumbprint"
        }
        $certificateArgs = @('/sha1', $normalizedThumbprint)
        if ($certificate.PSParentPath -like '*LocalMachine*') {
            $certificateArgs += '/sm'
        }
    }

    if ($SignToolPath) {
        $signTool = Resolve-File $SignToolPath 'signtool'
    } else {
        $signTool = (Get-Command signtool.exe -ErrorAction SilentlyContinue).Source
        if (-not $signTool -and ${env:ProgramFiles(x86)}) {
            $signTool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -File -ErrorAction SilentlyContinue |
                Sort-Object FullName -Descending |
                Select-Object -ExpandProperty FullName -First 1
        }
        if (-not $signTool) {
            throw 'signtool.exe not found. Install the Windows SDK or pass -SignToolPath.'
        }
    }

    foreach ($target in $targets) {
        & $signTool sign /fd SHA256 /tr $TimestampUrl.AbsoluteUri /td SHA256 @certificateArgs $target
        if ($LASTEXITCODE -ne 0) { throw "signtool sign failed ($LASTEXITCODE): $target" }

        & $signTool verify /pa /all $target
        if ($LASTEXITCODE -ne 0) { throw "signtool verify failed ($LASTEXITCODE): $target" }
    }
} finally {
    try {
        foreach ($importedThumbprint in $newThumbprints) {
            if ($importedThumbprint -notin $existingThumbprints -and (Test-Path -LiteralPath "Cert:\CurrentUser\My\$importedThumbprint")) {
                $importedCertificate = Get-Item -LiteralPath "Cert:\CurrentUser\My\$importedThumbprint"
                if ($importedCertificate.HasPrivateKey) {
                    Remove-Item -LiteralPath $importedCertificate.PSPath -DeleteKey -Force -ErrorAction Stop
                } else {
                    Remove-Item -LiteralPath $importedCertificate.PSPath -Force -ErrorAction Stop
                }
            }
        }
    } finally {
        if ($securePassword) { $securePassword.Dispose() }
        $password = $null
        $securePassword = $null
        $importedCertificates = $null
        $importedCertificate = $null
        $signingCertificates = $null
        $pfxData = $null
        $pfxLeaf = $null
        $pfxLeaves = $null
        $collision = $null
        $existingThumbprints = $null
        $newThumbprints = $null
        $certificateArgs = $null
        $certificate = $null
    }
}
