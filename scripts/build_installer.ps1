<#
.SYNOPSIS
  Builds the Veloryft Windows installer end to end.

.DESCRIPTION
  Steps:
    1. build the release binary (LTO, static CRT)
    2. compile the installer with Inno Setup (ISCC)

  The version comes from Cargo.toml — see docs/distribution/versioning.md.

.PARAMETER SkipBuild
  Package the already-built target\release\veloryft.exe instead of rebuilding.

.PARAMETER Features
  Extra Cargo features to enable, for example -Features track_mapping.

.INPUTS
  None. Reads Cargo.toml, installer/veloryft.iss, and installer/assets.

.OUTPUTS
  dist\Veloryft-Setup-<version>.exe

.EXAMPLE
  pwsh scripts/build_installer.ps1

.EXAMPLE
  pwsh scripts/build_installer.ps1 -SkipBuild
#>
[CmdletBinding()]
param(
  [switch]$SkipBuild,
  [string[]]$Features = @()
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\common.ps1"

<#
.SYNOPSIS
  Locates ISCC.exe (Inno Setup 6) on PATH or in its usual install locations.
#>
function Find-InnoSetupCompiler {
  $onPath = Get-Command ISCC.exe -ErrorAction SilentlyContinue
  if ($onPath) { return $onPath.Source }

  $candidates = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
  )
  foreach ($candidate in $candidates) {
    if (Test-Path $candidate) { return $candidate }
  }

  throw "ISCC.exe (Inno Setup 6) not found. Install it with: winget install --id JRSoftware.InnoSetup"
}

$repository = Get-RepositoryRoot
Push-Location $repository
try {
  $version = Get-PackageVersion -RepositoryRoot $repository
  Write-Host "==> Veloryft $($version.SemVer) (Windows resource $($version.Windows))" -ForegroundColor Cyan

  if (-not $SkipBuild) {
    Write-Host '==> Stopping any running Veloryft' -ForegroundColor Cyan
    Stop-RunningVeloryft

    Write-Host '==> Building release binary' -ForegroundColor Cyan
    $cargoArguments = @('build', '--release', '--locked')
    if ($Features.Count -gt 0) { $cargoArguments += @('--features', ($Features -join ',')) }
    & cargo @cargoArguments
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
  }

  $exe = Get-ReleaseBinary -RepositoryRoot $repository
  Write-Host "==> Packaging $exe" -ForegroundColor Cyan

  $iscc = Find-InnoSetupCompiler
  & $iscc "/DMyAppVersion=$($version.SemVer)" "/DMyAppVersionInfo=$($version.Windows)" 'installer\veloryft.iss'
  if ($LASTEXITCODE -ne 0) { throw "ISCC failed ($LASTEXITCODE)" }

  $output = Join-Path $repository "dist\Veloryft-Setup-$($version.SemVer).exe"
  if (-not (Test-Path $output)) { throw "expected installer not found at '$output'" }

  Write-Host ''
  Write-Host "OK: installer -> $output" -ForegroundColor Green
  Write-Host ('     size: {0:N1} MB' -f ((Get-Item $output).Length / 1MB))
}
finally {
  Pop-Location
}
