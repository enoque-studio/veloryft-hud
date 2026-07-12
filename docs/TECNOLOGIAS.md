# Stack Tecnológico — Veloryft

Este documento descreve as tecnologias, crates e ferramentas que compõem o Veloryft HUD, com o racional por trás de cada escolha.

## Visão geral

O Veloryft HUD é um aplicativo desktop para Windows, escrito principalmente em **Rust**. Ele captura telemetria do Forza Horizon 6 via UDP e desenha um overlay in-game com **Direct2D** sobre uma janela `WS_EX_LAYERED | WS_EX_TRANSPARENT`.

A escolha do Rust se deve a três fatores:

1. **Previsibilidade de performance** — zero-copy parsing, controle de memória e ausência de garbage collector são essenciais para um overlay de 60 fps.
2. **Ecossistema de segurança criptográfica maduro** — crates como `ed25519-dalek`, `rustls` e `sha2` validam assinaturas, tokens e certificados sem dependências externas.
3. **Binário único e portável** — com CRT estático, o executável roda em Windows 10/11 limpo.

## Linguagens

| Linguagem | Uso |
|-----------|-----|
| **Rust** | Núcleo do aplicativo: captura UDP, parsing, renderização, licenciamento. |
| **PowerShell** | Script de build de ponta a ponta (`installer/build-installer.ps1`). |
| **PascalScript** | Script do instalador Inno Setup (`installer/veloryft.iss`). |
| **Python** | Geração de assets de marca a partir de fontes em `docs/` (`installer/assets/make_assets.py`). |

## Crates Rust principais

### Núcleo e telemetria

| Crate | Versão | Função no projeto |
|-------|--------|-------------------|
| `serde` / `serde_json` | 1.x | Serialização de estado e telemetria derivada. |
| `arc-swap` | 1.x | Snapshot lock-free do último pacote de telemetria, lido pelo render sem bloquear a thread UDP. |

### Rede e segurança

| Crate | Versão | Função no projeto |
|-------|--------|-------------------|
| `tokio` | 1.x | Runtime assíncrono para requisições HTTPS do serviço de licenças. |
| `reqwest` | 0.12 | Cliente HTTP com TLS via `rustls` e raízes `webpki-roots`. |
| `rustls` | 0.23 | Implementação TLS em Rust, com verificação de pinagem SPKI do certificado do servidor de licenças. |
| `webpki-roots` | 0.26 | Raízes de confiança para TLS. |
| `ed25519-dalek` | 2.x | Verificação de assinaturas Ed25519 nas respostas do servidor de licenças. |
| `sha2` / `hmac` | 0.10+ / 0.12 | Atestado HMAC-SHA256 das requisições de licença. |
| `rand` | 0.8 | Geração de nonces criptograficamente seguros. |
| `base64` | 0.22 | Codificação de chaves e assinaturas. |
| `hex` | 0.4 | Representação hexadecimal de hashes e nonces. |
| `uuid` | 1.x | Identificadores de token (`jti`). |
| `x509-parser` | 0.16 | Parsing de certificados para pinagem SPKI. |
| `thiserror` | 2.x | Enumeração tipada de erros de licenciamento. |

### Windows nativo

| Crate | Versão | Função no projeto |
|-------|--------|-------------------|
| `windows` | 0.61 | Bindings Win32 para Direct2D, GDI, janelas layered, HiDPI, input, shell e registry. Fixada em `0.61` para manter um grafo único de `windows_core`. |
| `windows-numerics` | 0.2 | Tipos vetoriais e matriciais usados pelo Direct2D (`Matrix3x2`, `Vector2`). |
| `wmi` | 0.18 | Coleta de informações de hardware para fingerprint de máquina. |
| `winresource` | 0.1 | Embute ícone e version info no executável durante o build. |

### Compartilhamento com o backend

| Crate | Origem | Função no projeto |
|-------|--------|-------------------|
| `veloryft-core` | Path (`../../veloryft-back/crates/core`) | DTOs compartilhados (`Telemetry`, `HeightCell`, `WorldHeightmap`) entre HUD e backend. Fonte única de verdade para o schema de telemetria. |

### Serviços auxiliares

| Crate | Versão | Função no projeto |
|-------|--------|-------------------|
| `tiny_http` | 0.12 | Servidor HTTP local da live API de mapeamento de pista (desligado no lançamento inicial). |

## Ferramentas de build e distribuição

| Ferramenta | Versão / Origem | Função |
|------------|-----------------|--------|
| **Cargo** | Incluído no Rust | Build, testes e gerenciamento de dependências. |
| **Rust toolchain MSVC** | `x86_64-pc-windows-msvc` | Único alvo suportado; produz binários nativos para Windows. |
| **Visual Studio Build Tools** | Windows SDK | Fornece `rc.exe`, necessário para `winresource` embutir ícone e metadados. |
| **Inno Setup 6** | `winget install --id JRSoftware.InnoSetup` | Gera o instalador Windows profissional. |
| **Python 3 + Pillow** | Opcional | Regenera assets de marca (`wizard-large.bmp`, `wizard-small.bmp`, `veloryft.ico`). |

## Configurações de build importantes

### CRT estático

O arquivo [`.cargo/config.toml`](../.cargo/config.toml) força a compilação com a flag `+crt-static`:

```toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

Isso elimina a dependência do **Visual C++ Redistributable** na máquina do usuário final — essencial para um instalador standalone.

### Perfil de release

O [Cargo.toml](../src-tauri/Cargo.toml) define um perfil otimizado para distribuição:

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
strip = "symbols"
panic = "abort"
```

| Flag | Efeito |
|------|--------|
| `opt-level = 3` | Máxima otimização de velocidade. |
| `lto = "fat"` | Link-time optimization completo, reduz tamanho e melhora performance. |
| `codegen-units = 1` | Compilação como uma única unidade, permitindo otimizações agressivas. |
| `strip = "symbols"` | Remove tabela de símbolos, dificultando engenharia reversa. |
| `panic = "abort"` | Em vez de unwind, o processo aborta em panic — reduz binário e simplifica o runtime. |

### Checagem de revogação de certificado

O [`.cargo/config.toml`](../.cargo/config.toml) desliga `check-revoke` porque algumas redes corporativas falham com `CRYPT_E_NO_REVOCATION_CHECK` ao baixar crates. Isso afeta apenas o download de dependências durante o build, não a segurança do aplicativo em runtime.

## Por que não WebView2 / Electron / Tauri?

A versão original do prototipo usava Electron + Python/SSE. O Veloryft HUD migrou para Direct2D nativo pelos seguintes motivos:

| Aspecto | WebView2 / Electron | Direct2D nativo |
|---------|---------------------|-----------------|
| Latência | HTTP/SSE com poll de ~4 ms | UDP direto + callback zero-copy |
| Judder | Redirection bitmap + composição DWM a cada frame | `UpdateLayeredWindow` com DIB premultiplicado |
| Click-through | Difícil de garantir cross-process | `WS_EX_LAYERED \| WS_EX_TRANSPARENT` funciona nativamente |
| Memória | Alto footprint de Chromium | Apenas o executável Rust + recursos gráficos |

DirectComposition foi descartado porque `WS_EX_LAYERED` — necessário para click-through entre processos — é incompatível com o alpha-por-pixel do DComp.

## Integrações externas

| Sistema | Tecnologia | O que trocamos |
|---------|------------|----------------|
| Forza Horizon 6 | UDP "Data Out" | Pacotes Sled / Dash / FH6 na porta `20066`. |
| veloryft-back | HTTPS + JSON | Trial, ativação, validação de licença e (futuro) upload de voltas. |
| veloryft-core | Path dependency | DTOs compartilhados de telemetria. |
| Windows | Win32 + Direct2D | Janela layered, input global, bandeja do sistema, DPAPI, CNG/TPM. |

## Notas de manutenção

- A crate `windows` deve permanecer em `0.61` até que o ecossistema `windows_core` permita atualização sem duplicação de grafos.
- `veloryft-core` é uma dependência de path: os repositórios `veloryft-hud` e `veloryft-back` devem estar lado a lado no mesmo workspace pai.
- Nenhuma chave privada ou HMAC secret deve ser comitada. Eles entram no binário exclusivamente via variáveis de ambiente de build (ver [LICENCA.md](LICENCA.md)).
