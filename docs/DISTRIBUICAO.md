# Distribuição — Veloryft (Beta)

Como empacotar e distribuir o Veloryft como um instalador Windows profissional.

> **Nome do app:** o produto se chama apenas **"Veloryft"** — sem sufixo "HUD".
> `veloryft-hud` é só o nome deste repositório/projeto, não aparece em nenhum
> lugar visível ao usuário (exe, atalhos, título do instalador etc.).

## Visão geral

O instalador é gerado com **Inno Setup 6**. Tudo que ele precisa mora em
`installer/`:

```
installer/
  veloryft.iss            script do instalador (Inno Setup)
  build-installer.ps1     build de ponta a ponta (exe + instalador)
  EULA.txt                Termos de Uso (página de aceite obrigatória, inglês)
  README.txt              guia pós-instalação (habilitar telemetria + atalhos, inglês)
  assets/
    make_assets.py        gera os assets abaixo a partir de docs/
    veloryft.ico          ícone do app (embutido no exe via build.rs)
    wizard-large.bmp      banner lateral do wizard (+ @2x)
    wizard-small.bmp      banner de topo do wizard (+ @2x)
```

O binário sai como **`Veloryft.exe`** (renomeado no instalador; o alvo do cargo
continua `forza-hud-native.exe`). Metadados do exe (ícone, versão, empresa) são
embutidos em tempo de compilação por `src-tauri/build.rs` (crate `winresource`).

## Pré-requisitos

- **Rust (MSVC)** — mesma toolchain usada para desenvolver.
- **Windows SDK** — fornece o `rc.exe` que o `winresource` usa para embutir o
  ícone/version-info (já vem com o Build Tools do Visual Studio / MSVC).
- **Inno Setup 6**:
  ```powershell
  winget install --id JRSoftware.InnoSetup --source winget
  ```
- **Python 3 + Pillow** — só para (re)gerar os assets de marca:
  ```powershell
  pip install pillow
  ```

## Build de ponta a ponta

```powershell
pwsh installer/build-installer.ps1
```

Isso compila o release (`cargo build --release`) e depois o instalador (ISCC).
Saída em `dist/Veloryft-Setup-0.1.0-beta.exe`.

Variações:

```powershell
pwsh installer/build-installer.ps1 -Assets     # regenera as artes antes
pwsh installer/build-installer.ps1 -SkipBuild  # só recompila o instalador
```

Passos manuais equivalentes:

```powershell
cargo build --release --manifest-path src-tauri\Cargo.toml
& "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" installer\veloryft.iss
```

## O que o instalador faz

- Wizard **moderno**, **100% em inglês** (lançamento mundial), com a marca
  Veloryft.
- **Página de Termos de Uso** com aceite obrigatório (`EULA.txt`).
- Instalação **per-user sem UAC** (`PrivilegesRequired=lowest`); o usuário pode
  optar por instalar para todos no diálogo de privilégios.
- Requer **Windows 10 ou 11, 64 bits** (`MinVersion=10.0`).
- Cria atalhos no **Menu Iniciar** (app, guia, termos, desinstalar) e,
  opcionalmente, na **área de trabalho**.
- **Fecha o app** automaticamente antes de instalar/atualizar/desinstalar (o exe
  fica travado enquanto o overlay está aberto).
- Oferece **abrir o app** e **ler o guia** ao concluir.
- **Desinstalador** completo (com entrada em Aplicativos Instalados).

Os dados do usuário (`%APPDATA%\forza-hud-native`) **não** são removidos na
desinstalação, de propósito — preserva calibração e layout entre reinstalações.

## Ícone na bandeja do sistema

Ao ser executado, o Veloryft cria um ícone na bandeja do Windows (fica entre os
"ícones ocultos" até o usuário fixá-lo — comportamento padrão do Windows para
apps novos). Clique direito abre um menu (Mostrar/Ocultar, Sair); clique duplo
alterna mostrar/ocultar. Implementado em `render.rs` via `Shell_NotifyIconW`
numa janela mensagem-only dedicada (`HWND_MESSAGE`), sem interferir nas janelas
layered do overlay.

## Binário standalone

O `.cargo/config.toml` compila com **CRT estático**
(`target-feature=+crt-static`): o exe roda em Windows 10/11 limpos **sem** exigir
o "Visual C++ Redistributable". Nada além do instalador é necessário.

## Assinatura de código (Authenticode) e SmartScreen

> **Recomendado antes de distribuir publicamente.** Não impede cópia, mas é o que
> separa um app confiável de um "editor desconhecido".

Sem assinatura, o **SmartScreen** do Windows exibe um aviso de "editor
desconhecido" na primeira execução, o que assusta usuários e derruba a taxa de
instalação. Opções:

1. **Certificado OV** (Organization Validation) — mais barato, mas o SmartScreen
   só para de avisar depois de acumular reputação (muitos downloads).
2. **Certificado EV** (Extended Validation) — reputação praticamente imediata no
   SmartScreen; exige token de hardware. Mais caro.

Fluxo de assinatura (assinar **o exe e o instalador**), com `signtool` do SDK:

```powershell
$cert = "caminho\certificado.pfx"
$ts   = "http://timestamp.digicert.com"

# 1) assinar o binário ANTES de empacotar
signtool sign /fd SHA256 /f $cert /p <senha> /tr $ts /td SHA256 `
  src-tauri\target\release\forza-hud-native.exe

# 2) empacotar
& "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" installer\veloryft.iss

# 3) assinar o instalador gerado
signtool sign /fd SHA256 /f $cert /p <senha> /tr $ts /td SHA256 `
  dist\Veloryft-Setup-0.1.0-beta.exe
```

Inno Setup também suporta assinatura automática via `SignTool` (diretiva
`SignTool=` + `signtool` configurado no IDE) — útil para assinar o exe e o
desinstalador embutido de uma vez.

## A preencher antes do lançamento público

- [ ] **URL oficial** — hoje há placeholders `https://veloryft.com` no `.iss` e
      nos textos. Trocar pelo domínio/loja reais.
- [ ] **Canal de contato/suporte** — citado no EULA (cláusula 12) e no `.iss`.
- [ ] **Certificado de assinatura** — ver seção acima.
- [ ] **Revisão jurídica do EULA** — o texto é um ponto de partida sólido, mas
      convém uma revisão por advogado antes de distribuir em escala.
- [ ] **AppId (GUID)** em `veloryft.iss` — **não** alterar entre versões; é a
      identidade usada para atualização e desinstalação.

## Checklist de release

1. Ajustar `MyAppVersion` no `veloryft.iss` **e** `version` no
   `src-tauri/Cargo.toml` (manter em sincronia).
2. `pwsh installer/build-installer.ps1 -Assets`.
3. Testar em uma VM limpa de Windows 10 e 11: instalar, abrir com o Forza, aceitar
   os termos, desinstalar.
4. (Público) Assinar exe + instalador.
5. Publicar o `.exe` de `dist/`.
