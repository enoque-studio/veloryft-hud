# Referências — Veloryft

Este documento reúne links, documentações e materiais de referência úteis para quem desenvolve ou mantém o Veloryft HUD.

## Documentação oficial do projeto

| Documento | Descrição |
|-----------|-----------|
| [INDEX.md](INDEX.md) | Índice central da documentação. |
| [INICIO.md](INICIO.md) | Guia de início rápido. |
| [ARQUITETURA.md](ARQUITETURA.md) | Arquitetura da plataforma Veloryft e papel deste repo. |
| [ESTRUTURA.md](ESTRUTURA.md) | Organização de pastas e responsabilidades. |
| [TECNOLOGIAS.md](TECNOLOGIAS.md) | Stack tecnológico completo. |
| [INTERFACE.md](INTERFACE.md) | Guia da interface do HUD. |
| [DESENVOLVIMENTO.md](DESENVOLVIMENTO.md) | Guia do desenvolvedor. |
| [DISTRIBUICAO.md](DISTRIBUICAO.md) | Build e distribuição do instalador. |
| [LICENCA.md](LICENCA.md) | Provisionamento e fluxo de licenças. |
| [SEGURANCA.md](SEGURANCA.md) | Considerações de segurança. |

## Crates Rust

| Crate | Documentação | Uso no projeto |
|-------|--------------|----------------|
| `windows` | https://docs.rs/windows/0.61/windows/ | Bindings Win32 para Direct2D, janelas, input, shell, registry etc. |
| `windows-numerics` | https://docs.rs/windows-numerics/ | Tipos vetoriais e matriciais do Direct2D. |
| `arc-swap` | https://docs.rs/arc-swap/ | Snapshot lock-free de telemetria. |
| `tokio` | https://docs.rs/tokio/ | Runtime assíncrono para requisições HTTPS. |
| `reqwest` | https://docs.rs/reqwest/ | Cliente HTTP. |
| `rustls` | https://docs.rs/rustls/ | Implementação TLS em Rust. |
| `ed25519-dalek` | https://docs.rs/ed25519-dalek/ | Verificação de assinaturas Ed25519. |
| `sha2` | https://docs.rs/sha2/ | Hash SHA-256. |
| `hmac` | https://docs.rs/hmac/ | HMAC-SHA256. |
| `serde` / `serde_json` | https://docs.rs/serde/ | Serialização. |
| `thiserror` | https://docs.rs/thiserror/ | Enums de erro. |
| `uuid` | https://docs.rs/uuid/ | Identificadores. |
| `wmi` | https://docs.rs/wmi/ | Coleta de informações de hardware. |
| `winresource` | https://docs.rs/winresource/ | Embed de ícone e version info. |
| `tiny_http` | https://docs.rs/tiny_http/ | Servidor HTTP local (mapeamento de pista). |

## APIs e tecnologias Windows

| Tecnologia | Documentação | Uso no projeto |
|------------|--------------|----------------|
| Direct2D | https://learn.microsoft.com/en-us/windows/win32/direct2d/direct2d-portal | Renderização do overlay. |
| DirectWrite | https://learn.microsoft.com/en-us/windows/win32/directwrite/direct-write-portal | Texto no overlay. |
| Layered Windows | https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features#layered-windows | Janela transparente com alpha por pixel. |
| `UpdateLayeredWindow` | https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-updatelayeredwindow | Composição do DIB premultiplicado. |
| HiDPI | https://learn.microsoft.com/en-us/windows/win32/hidpi/high-dpi-desktop-application-development-on-windows | Suporte a múltiplos DPI. |
| DPAPI | https://learn.microsoft.com/en-us/windows/win32/seccng/cng-portal | Criptografia do estado de licença. |
| CNG | https://learn.microsoft.com/en-us/windows/win32/seccng/cng-portal | Chave ECDSA P-256 no TPM. |
| WMI | https://learn.microsoft.com/en-us/windows/win32/wmisdk/wmi-start-page | Fingerprint de hardware. |

## Protocolo de telemetria do Forza

| Recurso | Link | Descrição |
|---------|------|-----------|
| Forza Motorsport Data Out | https://forums.forza.net/t/data-out-feature/704316 | Discussão oficial do recurso Data Out. |
| Forza Horizon 5 / FM Data Out Format | https://github.com/ForzaMods/Forza-Mods-AIO/blob/main/ForzaData/ForzaData.md | Especificação comunitária dos offsets dos pacotes. |
| Sled vs Dash vs FH6 | Comunidade Forza (GitHub, fóruns) | Diferenças de tamanho e campos entre os formatos. |

> **Nota:** os layouts de pacote do Forza são documentados pela comunidade. Sempre valide alterações no parser com capturas reais do jogo.

## Ferramentas de build e distribuição

| Ferramenta | Link | Uso |
|------------|------|-----|
| Rust | https://www.rust-lang.org/ | Linguagem principal. |
| Cargo | https://doc.rust-lang.org/cargo/ | Build e dependências. |
| Visual Studio Build Tools | https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022 | `rc.exe` e Windows SDK. |
| Inno Setup | https://jrsoftware.org/isinfo.php | Instalador Windows. |
| Pillow | https://pillow.readthedocs.io/ | Geração de assets de marca. |

## Segurança e criptografia

| Tópico | Recurso |
|--------|---------|
| Ed25519 | https://ed25519.cr.yp.to/ |
| RFC 8032 — Ed25519 | https://datatracker.ietf.org/doc/html/rfc8032 |
| HMAC | https://datatracker.ietf.org/doc/html/rfc2104 |
| TLS Certificate Pinning | https://owasp.org/www-community/controls/Certificate_and_Public_Key_Pinning |
| DPAPI | https://learn.microsoft.com/en-us/windows/win32/seccng/cng-portal |
| JWT | https://jwt.io/introduction (apenas como referência de formato; tokens do Veloryft usam claims próprias) |

## Repositórios relacionados

| Repositório | Papel |
|-------------|-------|
| `veloryft-back` | API monolítica em Rust/Axum; emite licenças e recebe telemetria. |
| `veloryft-front` | Plataforma web em SolidJS + Vite. |
| `veloryft-db` | Schema PostgreSQL + TimescaleDB. |

## Comunidade e suporte

- Issues e pull requests devem ser abertos no repositório `veloryft-hud`.
- Dúvidas sobre segurança ou licenciamento devem ser encaminhadas à equipe de operações antes de release.

---

> **Última atualização:** 2026-07-12
