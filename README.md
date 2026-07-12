# Veloryft — HUD de telemetria nativo para Forza Horizon 6

O **Veloryft** é um overlay de telemetria para Windows, escrito em **Rust**, que
captura os pacotes UDP do Forza Horizon 6 e desenha um HUD in-game fluido com
**Direct2D nativo + janela layered**. O clique atravessa o overlay e chega ao
jogo, enquanto os painéis exibem RPM, marcha, velocidade, pedais, temperatura dos
pneus, força G e perfil de elevação da volta.

> **Nota sobre nomes:** `veloryft-hud` é o nome deste repositório. O produto
> voltado ao usuário chama-se apenas **Veloryft**.

Para começar a usar ou desenvolver, leia o [guia de início rápido](docs/INICIO.md).
A documentação completa está em [`docs/`](docs/INDEX.md).

## Destaques

- **Caminho de dados em Rust**, zero-copy, sem HTTP/SSE/JSON intermediário.
- **Transporte event-driven** a partir do UDP do jogo.
- **Overlay nativo**: composição GPU com alpha por pixel, sem o judder de WebView2.
- **Click-through real**: o mouse passa pelo HUD e interage com o jogo.
- **Licenciamento server-authoritative** com Ed25519, HMAC, TLS pinning, DPAPI e TPM.
- **Build standalone**: CRT estático, sem depender do Visual C++ Redistributable.

## Por que o overlay nativo

A janela transparente fullscreen do WebView2 passa pelo *redirection bitmap* e o
DWM mescla a camada alpha a cada frame → judder que nenhum ajuste de CSS/rAF
resolve.

O overlay nativo usa uma **janela pequena `WS_EX_LAYERED | WS_EX_TRANSPARENT`**
(faixa do HUD, não tela cheia): o Windows garante o click-through entre processos
(o clique vai pro jogo) e o resto da tela fica livre. Direct2D desenha num DIB
premultiplicado enviado ao DWM via `UpdateLayeredWindow` — composição GPU, alpha
por pixel, sem judder. Direct2D mantém o estilo (gradiente, cantos, anti-alias).

Por que não DirectComposition: dá render liso, mas `WS_EX_LAYERED` (necessário p/
click-through cross-process) é incompatível com o alpha-por-pixel do DComp, e
`WS_EX_TRANSPARENT`/`HTTRANSPARENT` sozinhos não atravessam para outro processo.

## Arquitetura

```
Forza (Data Out UDP :20066)
        │
        ▼
src-tauri/src/forza/         (núcleo de telemetria)
  udp.rs        thread: recv_from → parse → snapshot (callback opcional)
  packet.rs     detecção Sled/Dash/FH6 + leitura por offset (porta de packet.py)
  payload.rs    telemetria derivada
  display.rs    suavização, visibilidade (consome revlimit)
  revlimit.rs   redline learning: corte real + ponto de troca por carro,
                aprendido a todo gás e persistido em %APPDATA%
  elevation.rs  perfil de elevação da volta (loop fechado + bolinha)
  state.rs      snapshot lock-free (arc-swap)
        │
        ▼
src-tauri/src/render.rs   janela WS_EX_LAYERED|WS_EX_TRANSPARENT + Direct2D em DIB
                          + UpdateLayeredWindow (click-through); dirty-skip + sleep
                          adaptativo (menos carga na GPU)
```

O binário in-game é `main.rs` → `render.rs` (overlay Direct2D).

## Licenca

O overlay valida uma licenca antes de abrir em builds de distribuicao. O
provisionamento seguro do release, a rotacao de chaves/pins e o fluxo de
ativacao estao em [docs/LICENCA.md](docs/LICENCA.md).

## Executar

Pré-requisitos: Rust (toolchain MSVC), Windows 10/11. Veja detalhes em
[docs/INICIO.md](docs/INICIO.md).

```sh
# HUD in-game — overlay Direct2D nativo:
cargo run --manifest-path src-tauri/Cargo.toml
#   sair: Ctrl+Alt+Q
#   reposicionar painéis: Ctrl+Alt+E
```

No Forza Horizon 6: **Configurações → HUD/Telemetria → Data Out = Ligado**, IP
`127.0.0.1`, porta `20066` (override com a env `FORZA_UDP_PORT`).

Sem o jogo, o HUD fica oculto. Ao dirigir, aparece; some 100% em menu, pausa ou
cinemática. O modo de edição (**Ctrl+Alt+E**) força a exibição para reposicionar
painéis e ajustar cores, opacidade e visibilidade — tudo persistido em
`%APPDATA%/forza-hud-native/`.

## Limite de giro por carro (redline learning)

O aviso de troca não usa limiar fixo: `forza/revlimit.rs` aprende, por carro,
onde o **limitador corta** (platô de RPM com acelerador cravado — e o giro
precisa ter *subido* até o platô: overrev de reduzida forte entra por cima e
não conta) e a **curva de potência** + a queda de RPM dos upshifts — o ponto de troca recomendado é o
maior RPM em que segurar a marcha ainda rende mais potência do que a próxima
renderia. Na barra de RPM: tick branco = troca recomendada; banda/tick
vermelhos = zona até o corte (aparecem quando o limitador já foi observado).
O flash do cluster (`near`) dispara nesse ponto aprendido.

A calibração persiste em `%APPDATA%/forza-hud-native/rev-limits.json` por
`car_ordinal` — trocar de carro e voltar mantém o aprendizado; upgrade/swap de
motor (muda `engine_max_rpm`) recalibra do zero. Enquanto não há dados, vale o
default de 92% do fundo de escala.

## Status

- [x] Núcleo UDP/parse + overlay nativo Direct2D (cluster RPM/marcha/velocidade/
      pedais, pneus, força-G do piloto, perfil de elevação da volta; suavização em
      Rust).
- [ ] Atalhos globais completos, multi-monitor, gravação de sessão, build portable.
- [ ] Telas fora do jogo (configuração e login/licença).

## Lançamento inicial — mapeamento de pista desligado

O **HUD in-game** (RPM, marcha, velocidade, pedais, força-G, perfil de elevação da
volta) está no escopo do lançamento. A **telemetria de mapeamento de pista** —
gravador `WorldMapRecorder`, live API em `:9877`, autosave e upload para a
plataforma — está **desligada por enquanto** (`forza::TRACK_MAPPING_ENABLED = false`
em `src-tauri/src/forza/mod.rs`).

Motivo: o produto ainda está tomando forma; priorizamos entregar o overlay estável
antes de expor viewer 3D, back e persistência de pistas. Para reativar, mude a
constante para `true` e recompile.

## Documentação

A documentação completa do projeto está organizada em [`docs/`](docs/INDEX.md):

| Documento | Conteúdo |
|-----------|----------|
| [INICIO.md](docs/INICIO.md) | Requisitos, instalação e primeiro run. |
| [ARQUITETURA.md](docs/ARQUITETURA.md) | Arquitetura da plataforma Veloryft. |
| [ESTRUTURA.md](docs/ESTRUTURA.md) | Organização de pastas e módulos. |
| [TECNOLOGIAS.md](docs/TECNOLOGIAS.md) | Stack tecnológico e crates. |
| [INTERFACE.md](docs/INTERFACE.md) | Guia dos painéis, atalhos e modo de edição. |
| [DESENVOLVIMENTO.md](docs/DESENVOLVIMENTO.md) | Guia do desenvolvedor. |
| [DISTRIBUICAO.md](docs/DISTRIBUICAO.md) | Build e instalador Windows. |
| [LICENCA.md](docs/LICENCA.md) | Provisionamento e fluxo de licenças. |
| [SEGURANCA.md](docs/SEGURANCA.md) | Modelo de segurança e boas práticas. |
| [REFERENCIAS.md](docs/REFERENCIAS.md) | Links e documentações úteis. |

## Notas

- `.cargo/config.toml` desliga a checagem de revogação de certificado do schannel
  (`CRYPT_E_NO_REVOCATION_CHECK`) que trava o download de crates em algumas redes.
- A crate `windows` está fixada em `0.61` para um grafo único de `windows_core`.
- A porta UDP padrão (20066) é a mesma do backend Python original.
