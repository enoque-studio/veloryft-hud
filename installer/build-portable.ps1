<#
  build-portable.ps1 — gera a versão portable do Veloryft (sem instalador).

  A app só grava estado em %APPDATA%/%LOCALAPPDATA%/%ProgramData% (ver
  license/state.rs), então o mesmo .exe já roda de qualquer pasta (pendrive,
  Desktop, etc.) sem instalação. Este script só empacota o .exe + docs num zip.

  Uso:
    pwsh installer/build-portable.ps1              # build completo
    pwsh installer/build-portable.ps1 -SkipBuild   # empacota exe já compilado

  Saída: dist/Veloryft-Portable-<versao>-beta.zip
#>
[CmdletBinding()]
param(
  [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
Set-Location $repo

function Require-LicenseBuildConfiguration {
  $required = @(
    'VELORYFT_LICENSE_ORIGIN',
    'VELORYFT_LICENSE_APP_HMAC_SECRET',
    'VELORYFT_LICENSE_RESPONSE_PUBLIC_KEYS',
    'VELORYFT_LICENSE_SPKI_PINS'
  )
  $missing = @(
    $required | Where-Object {
      [string]::IsNullOrWhiteSpace((Get-Item -Path "Env:$_" -ErrorAction SilentlyContinue).Value)
    }
  )
  if ($missing.Count -gt 0) {
    throw "configuração de licença ausente: $($missing -join ', '). Consulte docs/LICENCA.md."
  }
}

Require-LicenseBuildConfiguration

if (-not $SkipBuild) {
  Write-Host "==> Encerrando HUD em execução (se houver)..." -ForegroundColor Cyan
  Get-Process forza-hud-native, Veloryft -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue

  Write-Host "==> Compilando release (LTO)..." -ForegroundColor Cyan
  cargo build --release --manifest-path src-tauri\Cargo.toml
  if ($LASTEXITCODE -ne 0) { throw "cargo build falhou ($LASTEXITCODE)" }
}

$exe = "src-tauri\target\release\forza-hud-native.exe"
if (-not (Test-Path $exe)) { throw "binário não encontrado: $exe (rode sem -SkipBuild)" }

$version = (cargo metadata --no-deps --manifest-path src-tauri\Cargo.toml --format-version 1 |
  ConvertFrom-Json).packages[0].version

$stageDir = "dist\portable-stage"
if (Test-Path $stageDir) { Remove-Item $stageDir -Recurse -Force }
New-Item -ItemType Directory -Path $stageDir | Out-Null

Copy-Item $exe (Join-Path $stageDir "Veloryft.exe")
Copy-Item "installer\README.txt" (Join-Path $stageDir "Read Me.txt")
Copy-Item "installer\EULA.txt" (Join-Path $stageDir "License Agreement.txt")

$zipPath = "dist\Veloryft-Portable-$version-beta.zip"
if (Test-Path $zipPath) { Remove-Item $zipPath -Force }
Compress-Archive -Path (Join-Path $stageDir '*') -DestinationPath $zipPath
Remove-Item $stageDir -Recurse -Force

Write-Host ""
Write-Host "OK: zip portable gerado ->" (Resolve-Path $zipPath) -ForegroundColor Green
Write-Host ("     tamanho: {0:N1} MB" -f ((Get-Item $zipPath).Length / 1MB))
