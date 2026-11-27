param(
    # The build's number, as for release-windows.ps1. A numbered build is a test build: it
    # leaves resources\latest.json alone.
    [string]$Revision = "",
    # The SSH name of the Mac that built the packages with scripts/build-mac.sh.
    [string]$MacHost = "mac",
    # Where the source sits on that Mac.
    [string]$MacDir = "~/work/stacker"
)

# Brings the Mac packages scripts/build-mac.sh made on the Mac to this machine, signs the
# update zips with the release key (which never leaves this machine), adds them to the
# release's SHA256SUMS.txt, and for a real release writes them into resources/latest.json.

$ErrorActionPreference = "Stop"
$Root = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
Set-Location $Root

function Get-Sha256Hex {
    param([Parameter(Mandatory = $true)][string]$Path)
    $Stream = [System.IO.File]::OpenRead($Path)
    $Algorithm = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([System.BitConverter]::ToString($Algorithm.ComputeHash($Stream)) -replace "-", "").ToLowerInvariant()
    }
    finally {
        $Algorithm.Dispose()
        $Stream.Dispose()
    }
}

if ($Revision -and $Revision -notmatch '^r\d+$') { throw "Revision must look like r74, got: $Revision" }
$Version = [string](Get-Content (Join-Path $Root "package.json") -Raw | ConvertFrom-Json).version
$Output = Join-Path $Root ("release\v$Version" + $(if ($Revision) { "-$Revision" } else { "" }))
New-Item $Output -ItemType Directory -Force | Out-Null

Write-Host "`n==> Fetching the Mac packages from $MacHost" -ForegroundColor Cyan
& scp -q "${MacHost}:$MacDir/release/macos/v$Version/Stacker-$Version-*" $Output
if ($LASTEXITCODE -ne 0) { throw "Copying the Mac packages failed; build them first with scripts/build-mac.sh on the Mac." }

$KeyPath = $env:STACKER_SIGNING_KEY
if (-not $KeyPath) { $KeyPath = Join-Path $env:USERPROFILE ".stacker\release-signing.key" }
if (-not (Test-Path $KeyPath)) { throw "Release signing key not found: $KeyPath" }

$ChecksumPath = Join-Path $Output "SHA256SUMS.txt"
$Sums = @()
if (Test-Path $ChecksumPath) { $Sums = @(Get-Content $ChecksumPath | Where-Object { $_ -notmatch 'macos' }) }
$Packages = @{}
foreach ($Chip in @("arm64", "x64")) {
    $Dmg = Join-Path $Output "Stacker-$Version-macos-$Chip.dmg"
    $Zip = Join-Path $Output "Stacker-$Version-update-macos-$Chip.zip"
    if (-not (Test-Path $Zip)) { Write-Host "No $Chip build; skipped." -ForegroundColor Yellow; continue }
    $ZipHash = Get-Sha256Hex $Zip
    $Signature = & cargo run -q --manifest-path (Join-Path $Root "src-tauri\Cargo.toml") --example release-key -- sign $KeyPath $Zip
    if ($LASTEXITCODE -ne 0 -or -not $Signature) { throw "Signing $Zip failed" }
    $Signature = ($Signature -join "`n").Trim()
    Set-Content "$Zip.minisig" -Value $Signature -Encoding ascii
    $Sums += "$ZipHash *$(Split-Path $Zip -Leaf)"
    if (Test-Path $Dmg) { $Sums += "$(Get-Sha256Hex $Dmg) *$(Split-Path $Dmg -Leaf)" }
    $Packages[$Chip] = [ordered]@{
        url = "https://github.com/byteswalk/stacker/releases/download/v$Version/$(Split-Path $Zip -Leaf)"
        sha256 = $ZipHash
        signature = $Signature
    }
}
if ($Packages.Count -eq 0) { throw "No Mac update package was found in $Output" }
$Sums | Set-Content $ChecksumPath -Encoding ascii

if ($Revision) {
    Write-Host "`nTest build $Revision - resources/latest.json left unchanged." -ForegroundColor Yellow
}
else {
    $LatestPath = Join-Path $Root "resources\latest.json"
    $Latest = Get-Content $LatestPath -Raw -Encoding utf8 | ConvertFrom-Json
    if ([string]$Latest.version -ne $Version) { throw "latest.json is for $($Latest.version), not $Version" }
    $Latest | Add-Member -NotePropertyName macos -NotePropertyValue ([pscustomobject]$Packages) -Force
    $Utf8NoBom = New-Object System.Text.UTF8Encoding $false
    [System.IO.File]::WriteAllText($LatestPath, (($Latest | ConvertTo-Json -Depth 10) + "`n"), $Utf8NoBom)
    Write-Host "resources/latest.json now names the Mac packages - commit it with the release." -ForegroundColor Yellow
}

Write-Host "`nMac release artifacts:" -ForegroundColor Green
Get-ChildItem $Output -Filter "*macos*" | Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize
