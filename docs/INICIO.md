# Guia de Início Rápido — Veloryft

Este guia mostra como executar o Veloryft HUD pela primeira vez, do clone até o overlay aparecendo no jogo.

> **Tempo estimado:** 10 a 20 minutos, dependendo da velocidade de download das dependências Rust.

## O que você precisa

- Windows 10 ou 11 (64 bits).
- Rust instalado com toolchain **MSVC** (`x86_64-pc-windows-msvc`).
- Visual Studio Build Tools ou Visual Studio Community com carga de trabalho **"Desenvolvimento para desktop com C++"** (fornece `rc.exe` e o Windows SDK).
- (Opcional) Inno Setup 6, se quiser gerar o instalador.
- (Opcional) O jogo Forza Horizon 6, se quiser ver telemetria real.

## 1. Clone o repositório

```powershell
git clone <url-do-repo> veloryft-hud
cd veloryft-hud
```

## 2. Verifique a dependência `veloryft-core`

O [Cargo.toml](../src-tauri/Cargo.toml) aponta para `veloryft-core` como dependência de path:

```toml
veloryft-core = { path = "../../veloryft-back/crates/core" }
```

Portanto, a estrutura esperada é:

```
workspace-pai/
├── veloryft-hud/        <- você está aqui
└── veloryft-back/
    └── crates/
        └── core/
```

Se os repositórios não estiverem lado a lado, o build falhará. Ajuste o path no `Cargo.toml` se sua organização for diferente.

## 3. Compile e execute em modo de desenvolvimento

Em builds de **debug**, o sistema de licenças aceita uma configuração vazia e entra em modo de desenvolvimento. Portanto, não é necessário configurar variáveis de licença para rodar localmente.

```powershell
cargo run --manifest-path src-tauri/Cargo.toml
```

Na primeira execução, o Cargo baixará e compilará todas as dependências. Isso pode levar vários minutos.

Se tudo correr bem, o aplicativo iniciará sem janela visível — isso é esperado, pois o overlay só aparece quando há telemetria válida ou quando você entra no modo de edição.

## 4. Teste o overlay sem o jogo (modo edição)

Pressione **Ctrl+Alt+E** para forçar a exibição do HUD. Você verá:

- Uma malha de grade sobre a tela.
- Os quatro painéis do HUD (cluster, pneus, G-force, elevação).
- Uma barra de ferramentas flutuante no topo para personalizar cores, opacidade e visibilidade.

Pressione **Ctrl+Alt+E** novamente para sair do modo de edição. Pressione **Ctrl+Alt+Q** para fechar o aplicativo.

> **Nota:** sem telemetria, os painéis mostrarão valores zerados ou em branco. Isso é normal.

## 5. Configure o Forza Horizon 6

Para receber telemetria real, ative o **Data Out** no jogo:

1. Abra o Forza Horizon 6.
2. Vá em **Configurações → HUD e Telemetria** (ou equivalente).
3. Ative **Data Out**.
4. Configure:
   - **IP:** `127.0.0.1`
   - **Porta:** `20066`

O Veloryft HUD escuta em `0.0.0.0:20066` por padrão. Você pode sobrescrever a porta com a variável de ambiente `FORZA_UDP_PORT`.

## 6. Dirija

Ao entrar em uma corrida ou free roam com movimento real, o HUD aparecerá automaticamente. Ele se esconde em:

- Menus e pausas.
- Cinemáticas.
- Free roam parado.

## 7. Personalize o HUD

No modo de edição (**Ctrl+Alt+E**):

- **Arraste** os painéis para reposicioná-los.
- **Clique e arraste nas bordas** para redimensionar.
- **Roda do mouse** sobre a barra de cores muda o tom de destaque.
- **Botão direito** alterna o alvo do destaque (marcha ou card).
- **Slider VIDRO** ajusta a opacidade do fundo.
- **Slider TOM** ajusta a matiz do vidro (ou deixa neutro).
- **Botões de painel** mostram/ocultam cada card.

As preferências são salvas automaticamente em `%APPDATA%/forza-hud-native/`.

## 8. Pare o aplicativo

- **Ctrl+Alt+Q** fecha o Veloryft.
- Também é possível clicar com o botão direito no ícone da bandeja do sistema e escolher **Sair**.

## Próximos passos

- Para entender a arquitetura: [ARQUITETURA.md](ARQUITETURA.md).
- Para desenvolver e testar: [DESENVOLVIMENTO.md](DESENVOLVIMENTO.md).
- Para gerar o instalador: [DISTRIBUICAO.md](DISTRIBUICAO.md).
- Para configurar licenças em release: [LICENCA.md](LICENCA.md).

## Solução de problemas comuns

### `cargo run` falha com "rc.exe not found"

Instale o Windows SDK através do Visual Studio Installer, marcando a carga de trabalho **"Desenvolvimento para desktop com C++"**.

### O overlay não aparece no jogo

1. Confirme que o Data Out está ligado no Forza e apontando para `127.0.0.1:20066`.
2. Verifique se o firewall do Windows não está bloqueando o processo `forza-hud-native.exe`.
3. Teste primeiro no modo de edição (**Ctrl+Alt+E**) para garantir que o aplicativo está rodando.

### O modo de edição não abre

Certifique-se de que o foco está na área de trabalho ou no jogo em modo janela/borderless. Atalhos globais podem não funcionar se outro aplicativo estiver capturando input em modo exclusivo.

### O executável fecha imediatamente em release

Builds release exigem as variáveis de licença. Consulte [LICENCA.md](LICENCA.md). Para desenvolvimento, use sempre `cargo run` sem o perfil release.
