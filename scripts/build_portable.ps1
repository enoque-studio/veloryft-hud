<#
.SYNOPSIS
  Builds the Veloryft portable package (no installer).

.DESCRIPTION
  Veloryft needs no files beside its executable — the car catalog is embedded in
  the binary — so the same .exe runs from any folder. This script only stages
  the executable together with the quick-start guide and the license terms, then
  zips it.

.PARAMETER SkipBuild
  Package the already-built target\release\veloryft.exe instead of rebuilding.

.PARAMETER Features
  Extra Cargo features to enable, for example -Features track_mapping.

.INPUTS
  None. Reads Cargo.toml and installer/*.txt.

.OUTPUTS
  dist\Veloryft-Portable-<version>.zip

.EXAMPLE
  pwsh scripts/build_portable.ps1 -SkipBuild
#>
[CmdletBinding()]
param(
  [switch]$SkipBuild,
  [string[]]$Features = @()
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\common.ps1"

$repository = Get-RepositoryRoot
Push-Location $repository
try {
  $version = Get-PackageVersion -RepositoryRoot $repository
  Write-Host "==> Veloryft $($version.SemVer) portable" -ForegroundColor Cyan

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

  $distribution = Join-Path $repository 'dist'
  New-Item -ItemType Directory -Force -Path $distribution | Out-Null

  $stage = Join-Path $distribution 'portable-stage'
  if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
  New-Item -ItemType Directory -Path $stage | Out-Null

  # Installed and portable builds ship the same file name, so a user moving
  # between them keeps the same shortcut target and the same tray identity.
  Copy-Item $exe (Join-Path $stage 'Veloryft.exe')
  Copy-Item (Join-Path $repository 'installer\readme.txt') (Join-Path $stage 'Read Me.txt')
  Copy-Item (Join-Path $repository 'installer\eula.txt') (Join-Path $stage 'License Agreement.txt')

  $zip = Join-Path $distribution "Veloryft-Portable-$($version.SemVer).zip"
  if (Test-Path $zip) { Remove-Item $zip -Force }
  Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
  Remove-Item $stage -Recurse -Force

  Write-Host ''
  Write-Host "OK: portable zip -> $zip" -ForegroundColor Green
  Write-Host ('     size: {0:N1} MB' -f ((Get-Item $zip).Length / 1MB))
}
finally {
  Pop-Location
}
