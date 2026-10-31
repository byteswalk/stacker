$ErrorActionPreference = "Stop"
$Root = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$File = Join-Path $Root "resources\install-overrides.json"

# What is signed must be what the raw URLs serve: Git stores this file with LF line ends.
$Utf8NoBom = New-Object System.Text.UTF8Encoding $false
$Text = [System.IO.File]::ReadAllText($File, $Utf8NoBom) -replace "`r`n", "`n"
[System.IO.File]::WriteAllText($File, $Text, $Utf8NoBom)

$KeyPath = $env:STACKER_SIGNING_KEY
if (-not $KeyPath) { $KeyPath = Join-Path $env:USERPROFILE ".stacker\release-signing.key" }
if (-not (Test-Path $KeyPath)) { throw "Release signing key not found: $KeyPath" }

$Manifest = Join-Path $Root "src-tauri\Cargo.toml"
$Signature = & cargo run -q --manifest-path $Manifest --example release-key -- sign $KeyPath $File
if ($LASTEXITCODE -ne 0 -or -not $Signature) { throw "Signing failed" }
$Signed = "$File.minisig"
[System.IO.File]::WriteAllText($Signed, (($Signature -join "`n").Trim() + "`n"), $Utf8NoBom)

& cargo run -q --manifest-path $Manifest --example release-key -- verify $File $Signed
if ($LASTEXITCODE -ne 0) { throw "The new signature does not verify" }
Write-Host "Signed resources/install-overrides.json - commit it with its .minisig." -ForegroundColor Green
