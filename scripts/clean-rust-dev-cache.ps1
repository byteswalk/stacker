param(
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$DebugDir = Join-Path $Root "src-tauri\target\debug"

if (-not (Test-Path -LiteralPath $DebugDir)) {
    Write-Host "Rust debug cache does not exist: $DebugDir"
    exit 0
}

$processes = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue)
$processById = @{}
foreach ($process in $processes) {
    $processById[[int]$process.ProcessId] = $process
}

function Test-BelongsToWorkspace {
    param([object]$Process)

    $current = $Process
    for ($depth = 0; $current -and $depth -lt 12; $depth++) {
        if ($current.CommandLine -and
            $current.CommandLine.IndexOf($Root, [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
            return $true
        }
        $parentId = [int]$current.ParentProcessId
        if ($parentId -le 0 -or -not $processById.ContainsKey($parentId)) {
            break
        }
        $current = $processById[$parentId]
    }
    return $false
}

$active = $processes |
    Where-Object {
        ($_.Name -eq "stacker.exe" -and $_.ExecutablePath -like "$DebugDir*") -or
        ($_.Name -in @("cargo.exe", "rustc.exe") -and (Test-BelongsToWorkspace $_))
    }
if ($active -and -not $Force) {
    Write-Error "Stacker dev or Rust compilation is still running. Stop dev first, or rerun with -Force."
}

$before = (Get-ChildItem -LiteralPath $DebugDir -File -Recurse -Force -ErrorAction SilentlyContinue |
    Measure-Object Length -Sum).Sum

$cargoDirectories = @(".fingerprint", "build", "deps", "examples", "incremental")
foreach ($name in $cargoDirectories) {
    $path = Join-Path $DebugDir $name
    if (Test-Path -LiteralPath $path) {
        Remove-Item -LiteralPath $path -Recurse -Force
    }
}

Get-ChildItem -LiteralPath $DebugDir -File -Force -ErrorAction SilentlyContinue |
    Where-Object {
        $_.Name -like "stacker*" -or
        $_.Name -like "libstacker*" -or
        $_.Name -in @(".cargo-lock", ".cargo-build-lock", ".cargo-artifact-lock")
    } |
    Remove-Item -Force

$after = (Get-ChildItem -LiteralPath $DebugDir -File -Recurse -Force -ErrorAction SilentlyContinue |
    Measure-Object Length -Sum).Sum
$freed = [Math]::Max([int64]0, [int64]$before - [int64]$after)

Write-Host ("Rust debug cache cleaned. Freed {0:N2} GiB." -f ($freed / 1GB)) -ForegroundColor Green
Write-Host "Preserved non-Cargo directories such as go, gradle, fnm, jdk, maven, and pyenv."
