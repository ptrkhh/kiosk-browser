param(
    [Parameter(Mandatory)]
    [string]$Path
)

$signature = Get-AuthenticodeSignature -LiteralPath $Path
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch '(^|,\s*)O=Microsoft Corporation(,|$)') {
    throw "WebView2 installer must have a valid Microsoft Authenticode signature: $($signature.Status) $($signature.StatusMessage)"
}
