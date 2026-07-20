param(
    [switch]$SkipChecks
)

$ErrorActionPreference = "Stop"
$Root = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
Set-Location $Root

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][scriptblock]$Action
    )
    Write-Host "`n==> $Label" -ForegroundColor Cyan
    & $Action
    if ($LASTEXITCODE -ne 0) {
        throw "$Label failed with exit code $LASTEXITCODE"
    }
}

function Get-Sha256Hex {
    param(
        [Parameter(Mandatory = $true)][string]$Path
    )

    $Stream = [System.IO.File]::OpenRead($Path)
    $Algorithm = [System.Security.Cryptography.SHA256]::Create()
    try {
        $Bytes = $Algorithm.ComputeHash($Stream)
        return ([System.BitConverter]::ToString($Bytes) -replace "-", "").ToLowerInvariant()
    }
    finally {
        $Algorithm.Dispose()
        $Stream.Dispose()
    }
}

$Package = Get-Content (Join-Path $Root "package.json") -Raw | ConvertFrom-Json
$Version = [string]$Package.version
$Tauri = Get-Content (Join-Path $Root "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json
$Latest = Get-Content (Join-Path $Root "resources\latest.json") -Raw -Encoding utf8 | ConvertFrom-Json
$Cargo = Get-Content (Join-Path $Root "src-tauri\Cargo.toml") -Raw
$CargoVersion = [regex]::Match($Cargo, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value

if ($Version -ne [string]$Tauri.version -or $Version -ne $CargoVersion -or $Version -ne [string]$Latest.version) {
    throw "Version mismatch: package.json=$Version, tauri.conf.json=$($Tauri.version), Cargo.toml=$CargoVersion, latest.json=$($Latest.version)"
}

if (-not $SkipChecks) {
    Invoke-Checked "Release metadata" { & npm.cmd run check:release-metadata }
    Invoke-Checked "Rust format" { & cargo fmt --manifest-path src-tauri\Cargo.toml -- --check }
    Invoke-Checked "Frontend lint" { & npm.cmd run lint }
    Invoke-Checked "Frontend tests" { & npm.cmd run test }
    Invoke-Checked "Rust tests" { & cargo test --manifest-path src-tauri\Cargo.toml }
    Invoke-Checked "Rust clippy" { & cargo clippy --manifest-path src-tauri\Cargo.toml --all-targets -- -D warnings }
}

Invoke-Checked "Windows release build" { & npm.cmd run tauri -- build }

$ReleaseExe = Join-Path $Root "src-tauri\target\release\stacker.exe"
$NsisSource = Join-Path $Root "src-tauri\target\release\bundle\nsis\Stacker_${Version}_x64-setup.exe"
if (-not (Test-Path $ReleaseExe)) { throw "Release executable not found: $ReleaseExe" }
if (-not (Test-Path $NsisSource)) { throw "NSIS installer not found: $NsisSource" }

$Output = Join-Path $Root "release\v$Version"
$PortableStage = Join-Path $Output "portable"
if (Test-Path $Output) { Remove-Item $Output -Recurse -Force }
New-Item $PortableStage -ItemType Directory -Force | Out-Null

$InstallerName = "Stacker-$Version-setup-windows-x64.exe"
$PortableName = "Stacker-$Version-portable-windows-x64.zip"
$InstallerPath = Join-Path $Output $InstallerName
$PortablePath = Join-Path $Output $PortableName

Copy-Item $NsisSource $InstallerPath
Copy-Item $ReleaseExe (Join-Path $PortableStage "Stacker.exe")
Copy-Item (Join-Path $Root "LICENSE") (Join-Path $PortableStage "LICENSE")
Copy-Item (Join-Path $Root "resources\PORTABLE_README.txt") (Join-Path $PortableStage "README.txt")
Compress-Archive -Path (Join-Path $PortableStage "*") -DestinationPath $PortablePath -CompressionLevel Optimal
Remove-Item $PortableStage -Recurse -Force

$ChecksumPath = Join-Path $Output "SHA256SUMS.txt"
$Checksums = @($InstallerPath, $PortablePath) | ForEach-Object {
    "$(Get-Sha256Hex $_) *$([System.IO.Path]::GetFileName($_))"
}
$Checksums | Set-Content $ChecksumPath -Encoding ascii

Write-Host "`nRelease artifacts:" -ForegroundColor Green
Get-ChildItem $Output | Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize
