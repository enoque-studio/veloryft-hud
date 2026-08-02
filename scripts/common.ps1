<#
  common.ps1 — helpers shared by the packaging scripts.

  Dot-source it; it defines functions and sets no global state:

      . "$PSScriptRoot\common.ps1"

  Every function throws on failure so that callers running with
  $ErrorActionPreference = 'Stop' exit non-zero without extra checks.
#>

Set-StrictMode -Version Latest

<#
.SYNOPSIS
  Absolute path to the repository root (the parent of scripts/).
#>
function Get-RepositoryRoot {
  $root = Split-Path $PSScriptRoot -Parent
  if (-not (Test-Path (Join-Path $root 'Cargo.toml'))) {
    throw "Cargo.toml not found in '$root'; scripts/ must sit directly under the repository root."
  }
  return $root
}

<#
.SYNOPSIS
  Throws unless an executable is on PATH.
.PARAMETER Name
  Command to look for.
.PARAMETER InstallHint
  Shown to the user when the command is missing.
#>
function Assert-Command {
  param(
    [Parameter(Mandatory)][string]$Name,
    [Parameter(Mandatory)][string]$InstallHint
  )
  if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
    throw "'$Name' was not found on PATH. $InstallHint"
  }
}

<#
.SYNOPSIS
  The package version from Cargo.toml, the single source of truth.
.OUTPUTS
  PSCustomObject with:
    SemVer   the Cargo version, e.g. '0.2.0-beta.1'
    Windows  the four-component resource version, e.g. '0.2.0.1'
#>
function Get-PackageVersion {
  param([string]$RepositoryRoot = (Get-RepositoryRoot))

  Assert-Command -Name 'cargo' -InstallHint 'Install the Rust toolchain from https://rustup.rs.'

  $manifest = Join-Path $RepositoryRoot 'Cargo.toml'
  $metadataJson = & cargo metadata --no-deps --format-version 1 --manifest-path $manifest
  if ($LASTEXITCODE -ne 0) { throw "cargo metadata failed ($LASTEXITCODE)" }

  $metadata = $metadataJson | ConvertFrom-Json
  $package = $metadata.packages | Where-Object { $_.name -eq 'veloryft-hud' }
  if (-not $package) { throw "package 'veloryft-hud' not found in cargo metadata" }

  return [PSCustomObject]@{
    SemVer  = $package.version
    Windows = ConvertTo-WindowsVersion $package.version
  }
}

<#
.SYNOPSIS
  Converts a SemVer string into the four-component Windows resource version.
.DESCRIPTION
  Mirrors windows_resource_version() in src/version.rs: the fourth component is
  the prerelease ordinal, or 0 for a final release. Keep both in step.
#>
function ConvertTo-WindowsVersion {
  param([Parameter(Mandatory)][string]$SemVer)

  $withoutBuildMetadata = ($SemVer -split '\+')[0]
  $parts = $withoutBuildMetadata -split '-', 2
  $core = $parts[0]

  if ($core -notmatch '^\d+\.\d+\.\d+$') {
    throw "package version '$SemVer' is not major.minor.patch[-prerelease]"
  }

  $build = 0
  if ($parts.Count -gt 1) {
    $numeric = ($parts[1] -split '\.') | Where-Object { $_ -match '^\d+$' } | Select-Object -Last 1
    if ($numeric) { $build = [int]$numeric }
  }

  return "$core.$build"
}

<#
.SYNOPSIS
  Path to the release binary, throwing a useful message when it is missing.
#>
function Get-ReleaseBinary {
  param([string]$RepositoryRoot = (Get-RepositoryRoot))

  $exe = Join-Path $RepositoryRoot 'target\release\veloryft.exe'
  if (-not (Test-Path $exe)) {
    throw "release binary not found at '$exe'. Run the script without -SkipBuild, or run: cargo build --release"
  }
  return $exe
}

<#
.SYNOPSIS
  Stops any running Veloryft process so its executable can be replaced.
#>
function Stop-RunningVeloryft {
  # veloryft = current binary name, Veloryft = installed name,
  # forza-hud-native = the pre-0.2 binary name, still possible on dev machines.
  Get-Process -Name 'veloryft', 'Veloryft', 'forza-hud-native' -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue
}
