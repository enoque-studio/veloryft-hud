<#
  build-installer.ps1 — gera o instalador do Veloryft de ponta a ponta.

  Passos:
    1. (opcional) regenera os assets de marca a partir de docs/
    2. compila o binário release (LTO + CRT estático)
    3. compila o instalador com o Inno Setup (ISCC)

  Uso:
    pwsh installer/build-installer.ps1              # build completo
    pwsh installer/build-installer.ps1 -SkipBuild   # só recompila o instalador
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

if ($Assets) {
  Write-Host "==> Regenerando assets de marca..." -ForegroundColor Cyan
  python "$PSScriptRoot\assets\make_assets.py"
}

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
