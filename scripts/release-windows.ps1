param(
    [switch]$SkipChecks,
    # The build's number, e.g. r74: shown next to the version in the app (`v0.3.4 (r74)`) and
    # used for the output folder (release\v0.3.4-r74), so builds of one version tell apart.
    [string]$Revision = ""
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
    Invoke-Checked "Internationalization coverage" { & npm.cmd run check:i18n }
    Invoke-Checked "Rust format" { & cargo fmt --manifest-path src-tauri\Cargo.toml -- --check }
    Invoke-Checked "Frontend lint" { & npm.cmd run lint }
    Invoke-Checked "Frontend tests" { & npm.cmd run test }
    Invoke-Checked "Rust tests" { & cargo test --manifest-path src-tauri\Cargo.toml }
    Invoke-Checked "Rust clippy" { & cargo clippy --manifest-path src-tauri\Cargo.toml --all-targets -- -D warnings }
}

Invoke-Checked "Browser extension build" { & npm.cmd run ext:build }
if ($Revision -and $Revision -notmatch '^r\d+$') { throw "Revision must look like r74, got: $Revision" }
# Vite reads VITE_* variables at build time into the frontend.
$env:VITE_STACKER_REVISION = $Revision
try {
    Invoke-Checked "Windows release build" { & npm.cmd run tauri -- build --config src-tauri/tauri.bundle.conf.json }
} finally {
    Remove-Item Env:VITE_STACKER_REVISION -ErrorAction SilentlyContinue
}

$ReleaseExe = Join-Path $Root "src-tauri\target\release\stacker.exe"
$NsisSource = Join-Path $Root "src-tauri\target\release\bundle\nsis\Stacker_${Version}_x64-setup.exe"
if (-not (Test-Path $ReleaseExe)) { throw "Release executable not found: $ReleaseExe" }
if (-not (Test-Path $NsisSource)) { throw "NSIS installer not found: $NsisSource" }

$BundledExtension = Join-Path $Root "src-tauri\target\release\extension"
if (-not (Test-Path (Join-Path $BundledExtension "manifest.json"))) { throw "Browser extension was not bundled: $BundledExtension" }
if (-not (Test-Path (Join-Path $BundledExtension "chunks"))) { throw "Browser extension subfolders were not bundled: $BundledExtension" }

$Output = Join-Path $Root ("release\v$Version" + $(if ($Revision) { "-$Revision" } else { "" }))
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
Copy-Item (Join-Path $Root "extension\dist") (Join-Path $PortableStage "extension") -Recurse
New-Item (Join-Path $PortableStage "portable.flag") -ItemType File -Force | Out-Null
Compress-Archive -Path (Join-Path $PortableStage "*") -DestinationPath $PortablePath -CompressionLevel Optimal
Remove-Item $PortableStage -Recurse -Force

$ChecksumPath = Join-Path $Output "SHA256SUMS.txt"
$InstallerHash = Get-Sha256Hex $InstallerPath
$PortableHash = Get-Sha256Hex $PortablePath
$Checksums = @(
    "$InstallerHash *$InstallerName",
    "$PortableHash *$PortableName"
)
$Checksums | Set-Content $ChecksumPath -Encoding ascii

# 用发布私钥给安装包签名。私钥只在本机，路径由 STACKER_SIGNING_KEY 指定，
# 默认 %USERPROFILE%\.stacker\release-signing.key。私钥有密码时会交互式询问。
$KeyPath = $env:STACKER_SIGNING_KEY
if (-not $KeyPath) { $KeyPath = Join-Path $env:USERPROFILE ".stacker\release-signing.key" }
if (-not (Test-Path $KeyPath)) {
    throw "Release signing key not found: $KeyPath. Generate one with: cargo run --manifest-path src-tauri/Cargo.toml --example release-key -- keygen `"$KeyPath`""
}
$InstallerSig = & cargo run -q --manifest-path (Join-Path $Root "src-tauri\Cargo.toml") --example release-key -- sign $KeyPath $InstallerPath
if ($LASTEXITCODE -ne 0 -or -not $InstallerSig) { throw "Signing the installer failed" }
$PortableSig = & cargo run -q --manifest-path (Join-Path $Root "src-tauri\Cargo.toml") --example release-key -- sign $KeyPath $PortablePath
if ($LASTEXITCODE -ne 0 -or -not $PortableSig) { throw "Signing the portable archive failed" }
$InstallerSig = ($InstallerSig -join "`n").Trim()
$PortableSig = ($PortableSig -join "`n").Trim()
Set-Content "$InstallerPath.minisig" -Value $InstallerSig -Encoding ascii
Set-Content "$PortablePath.minisig" -Value $PortableSig -Encoding ascii

# 自动更新把校验值和签名写回发布清单：应用下载更新后先比校验值再验签，任一不符就拒绝安装。
# SHA256SUMS.txt 必须与安装包一起上传到 Release，走 Releases 接口的检查会读它。
$LatestPath = Join-Path $Root "resources\latest.json"
$Latest = Get-Content $LatestPath -Raw -Encoding utf8 | ConvertFrom-Json
$Latest.installer_sha256 = $InstallerHash
$Latest.portable_sha256 = $PortableHash
$Latest | Add-Member -NotePropertyName installer_signature -NotePropertyValue $InstallerSig -Force
# Set-Content -Encoding utf8 在 Windows PowerShell 下会写 BOM，JSON 解析器不吃，必须绕开。
$Utf8NoBom = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText($LatestPath, (($Latest | ConvertTo-Json -Depth 10) + "`n"), $Utf8NoBom)
Invoke-Checked "Release metadata check" { & npm.cmd run check:release-metadata }
Write-Host "resources/latest.json updated with the release checksums and signature - commit it with the release." -ForegroundColor Yellow

Write-Host "`nRelease artifacts:" -ForegroundColor Green
Get-ChildItem $Output | Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize
