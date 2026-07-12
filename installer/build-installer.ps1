<#
  build-installer.ps1 — gera o instalador do Veloryft de ponta a ponta.

  Passos:
    1. (opcional) regenera os assets de marca a partir de docs/
    2. compila o binário release (LTO + CRT estático)
    3. compila o instalador com o Inno Setup (ISCC)

  Uso:
    pwsh installer/build-installer.ps1              # build completo (requer config de licenca)
    pwsh installer/build-installer.ps1 -SkipBuild   # empacota exe existente (ainda valida config de licenca)
    pwsh installer/build-installer.ps1 -Assets      # regenera artes antes

  Saída: dist/Veloryft-Setup-<versao>-beta.exe
#>
[CmdletBinding()]
param(
  [switch]$SkipBuild,   # pula o cargo build (usa o exe já compilado)
  [switch]$Assets       # regenera os assets de marca (requer Python + Pillow)
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
Set-Location $repo

function Find-ISCC {
  $cmd = Get-Command ISCC.exe -ErrorAction SilentlyContinue
  if ($cmd) { return $cmd.Source }
  $cands = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
  )
  foreach ($c in $cands) { if (Test-Path $c) { return $c } }
  throw "ISCC.exe (Inno Setup 6) não encontrado. Instale com: winget install --id JRSoftware.InnoSetup --source winget"
}

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

  $responseKeys = @($env:VELORYFT_LICENSE_RESPONSE_PUBLIC_KEYS -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
  $spkiPins = @($env:VELORYFT_LICENSE_SPKI_PINS -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
  if ($responseKeys.Count -lt 2 -or $spkiPins.Count -lt 2) {
    throw 'a release requer pelo menos duas chaves publicas Ed25519 e dois pins SPKI (atual e reserva).'
  }
}

if ($Assets) {
  Write-Host "==> Regenerando assets de marca..." -ForegroundColor Cyan
  python "$PSScriptRoot\assets\make_assets.py"
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

$iscc = Find-ISCC
Write-Host "==> Compilando instalador com $iscc" -ForegroundColor Cyan
& $iscc "installer\veloryft.iss"
if ($LASTEXITCODE -ne 0) { throw "ISCC falhou ($LASTEXITCODE)" }

$out = Get-ChildItem "dist\*.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
Write-Host ""
Write-Host "OK: instalador gerado ->" $out.FullName -ForegroundColor Green
Write-Host ("     tamanho: {0:N1} MB" -f ($out.Length / 1MB))
