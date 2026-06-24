# Forza HUD — protótipo nativo (Rust)

Reimplementação do HUD de telemetria do Forza com **núcleo Rust** (UDP + parsing)
compartilhado, e **duas camadas de apresentação**:

1. **`overlay` — nativo Direct2D + layered window** (recomendado). Faixa pequena
   no rodapé, click-through real (clique vai pro jogo), barras lisas sem o judder
   do WebView2.
2. **app Tauri/WebView2** — reaproveita a UI Solid; serve para janelas **fora do
   jogo** (configuração, login/licença). **Não** é o caminho da UI rica in-game —
   ver Fase 2.

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
src-tauri/src/forza/         (núcleo compartilhado)
  udp.rs      thread: recv_from → parse → snapshot (callback opcional)
  packet.rs   detecção Sled/Dash/FH6 + leitura por offset (porta de packet.py)
  payload.rs  telemetria derivada (subconjunto da Fase 1)
  display.rs  suavização, redline learning, visibilidade — fonte única (overlay + Tauri)
  state.rs    snapshot lock-free (arc-swap)
        │
        ├─► src/render.rs  (bin `overlay`)  janela WS_EX_LAYERED|WS_EX_TRANSPARENT +
        │                  Direct2D em DIB + UpdateLayeredWindow (click-through);
        │                  dirty-skip + sleep adaptativo (menos carga na GPU)
        │
        └─► src/lib.rs (app Tauri)  thread display → evento `display` → src/App.tsx
```

Ambas as camadas de HUD leem o mesmo `DisplaySmoother` em Rust — o front só aplica
valores já suavizados, sem duplicar lógica.

## Rodar

Pré-requisitos: Rust (msvc), Node, WebView2 (Windows 11).

```sh
# Overlay nativo (recomendado — barras lisas):
cargo run --bin overlay --manifest-path src-tauri/Cargo.toml
#   sair: Ctrl+Alt+Q

# App Tauri/WebView2 (UI Solid):
npm install
npm run tauri dev
```

No Forza: **Configurações → HUD/Telemetria → Data Out = Ligado**, IP `127.0.0.1`,
porta `20066` (override com a env `FORZA_UDP_PORT`).

Sem o jogo, o overlay mostra a marcha "–" e barras vazias. Para um teste rápido
sem o jogo, envie um datagrama de ≥311 bytes para a porta 20066.

## Status

- [x] **Fase 1** — núcleo UDP/parse + duas camadas: app Tauri e **overlay nativo
      Direct2D** (barra de RPM, marcha, velocidade, pedais; suavização em Rust).
- [ ] **Fase 2** — `payload.rs` completo (paridade com `payload.py`) + 8 painéis
      ricos **em Direct2D nativo** (mesma pipeline do overlay: DIB premultiplicado +
      `UpdateLayeredWindow`). Foco em performance sem perder o design; WebView2 fica
      fora do caminho de render in-game.
- [ ] **Fase 3** — atalhos globais completos, multi-monitor, modo edição,
      gravação de sessão, build portable.

## Notas

- `.cargo/config.toml` desliga a checagem de revogação de certificado do schannel
  (`CRYPT_E_NO_REVOCATION_CHECK`) que trava o download de crates em algumas redes.
- A crate `windows` está fixada em `0.61` (mesma do Tauri) para evitar duas
  versões de `windows_core` no grafo.
- A porta UDP padrão (20066) é a mesma do backend Python original.
