<#
.SYNOPSIS
  Writes SHA256SUMS for the release artifacts in dist/.

.DESCRIPTION
  Produces the standard `sha256sum` format — "<hash>  <name>", two spaces — so
  users can verify with either PowerShell or coreutils:

      sha256sum --check SHA256SUMS
      (Get-FileHash Veloryft-Setup-0.2.0-beta.1.exe).Hash

  Only file names are recorded, never absolute paths, so the file is identical
  wherever it was produced.

.PARAMETER Path
  Directory holding the artifacts. Defaults to dist/.

.INPUTS
  None.

.OUTPUTS
  <Path>\SHA256SUMS

.EXAMPLE
  pwsh scripts/generate_checksums.ps1
#>
[CmdletBinding()]
param(
  [string]$Path
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\common.ps1"

$repository = Get-RepositoryRoot
if (-not $Path) { $Path = Join-Path $repository 'dist' }

if (-not (Test-Path $Path)) {
  throw "artifact directory '$Path' does not exist. Build the installer or the portable package first."
}

$artifacts = Get-ChildItem -Path $Path -File |
  Where-Object { $_.Extension -in '.exe', '.zip' } |
  Sort-Object Name

if ($artifacts.Count -eq 0) {
  throw "no .exe or .zip artifacts found in '$Path'. Nothing to checksum."
}

$lines = foreach ($artifact in $artifacts) {
  $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $artifact.FullName).Hash.ToLowerInvariant()
  Write-Host ("  {0}  {1}" -f $hash, $artifact.Name)
  "$hash  $($artifact.Name)"
}

$checksums = Join-Path $Path 'SHA256SUMS'
# LF endings and no BOM: `sha256sum --check` rejects CRLF and a BOM.
$content = ($lines -join "`n") + "`n"
[System.IO.File]::WriteAllText($checksums, $content, [System.Text.UTF8Encoding]::new($false))

Write-Host ''
Write-Host "OK: checksums -> $checksums" -ForegroundColor Green
