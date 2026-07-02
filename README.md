# Forza HUD — overlay nativo (Rust)

HUD de telemetria do Forza com **núcleo Rust** (UDP + parsing) e apresentação em
**Direct2D nativo + layered window** — **este é o HUD in-game** (produto de
lançamento). Faixa pequena no rodapé, click-through real (clique vai pro jogo),
barras lisas sem o judder do WebView2.

Comparado ao stack original (Electron + Python/SSE):

- **Caminho de dados em Rust**, zero-copy, sem HTTP/SSE/JSON intermediário.
- **Transporte event-driven** (sem o poll de 4 ms do SSE).
- **Overlay nativo**: composição GPU com alpha por pixel, RAM mínima.

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

## Rodar

Pré-requisitos: Rust (msvc), Windows 11.

```sh
# HUD in-game — overlay Direct2D nativo:
cargo run --manifest-path src-tauri/Cargo.toml
#   sair: Ctrl+Alt+Q
#   reposicionar painéis: Ctrl+Alt+E
#   release: cargo run --release --manifest-path src-tauri/Cargo.toml
```

No Forza: **Configurações → HUD/Telemetria → Data Out = Ligado**, IP `127.0.0.1`,
porta `20066` (override com a env `FORZA_UDP_PORT`).

Sem o jogo, o HUD fica oculto (sem telemetria). Ao dirigir, aparece; some
100% em menu, pausa ou cinemática (`visibility::is_hud_visible`). Modo edição
(**Ctrl+Alt+E**) força visibilidade para reposicionar painéis; a toolbar traz
cor do acento, alvo do shift, visibilidade dos cards e os sliders **VIDRO**
(opacidade) e **TOM** (neutro ou matiz do vidro) — tudo persistido.

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
- [ ] Telas fora do jogo (configuração, login/licença).

## Lançamento inicial — mapeamento de pista desligado

O **HUD in-game** (RPM, marcha, velocidade, pedais, força-G, perfil de elevação da
volta) está no escopo do lançamento. A **telemetria de mapeamento de pista** —
gravador `WorldMapRecorder`, live API em `:9877`, autosave e upload para a
plataforma — está **desligada por enquanto** (`forza::TRACK_MAPPING_ENABLED = false`
em `src-tauri/src/forza/mod.rs`).

Motivo: o produto ainda está tomando forma; priorizamos entregar o overlay estável
antes de expor viewer 3D, back e persistência de pistas. Para reativar, mude a
constante para `true` e recompile.

## Notas

- `.cargo/config.toml` desliga a checagem de revogação de certificado do schannel
  (`CRYPT_E_NO_REVOCATION_CHECK`) que trava o download de crates em algumas redes.
- A crate `windows` está fixada em `0.61` para um grafo único de `windows_core`.
- A porta UDP padrão (20066) é a mesma do backend Python original.
