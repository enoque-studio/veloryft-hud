# veloryft-hud — papel na arquitetura Veloryft

> Cliente desktop (Windows) que captura a telemetria do **Forza Horizon 6** e
> desenha o HUD in-game. É o **produtor** dos dados que alimentam a plataforma.

## Visão geral — o sistema Veloryft

Veloryft são **dois produtos pagos** distribuídos em **quatro repositórios**:

| Repositório | Papel | Stack |
|---|---|---|
| **veloryft-hud** | Cliente desktop: captura telemetria do Forza e exibe o HUD | Rust + Tauri + Direct2D |
| veloryft-back | API monolítica; **único** tier com acesso ao banco | Rust + axum |
| veloryft-front | Plataforma web: ranking, voltas, pistas, perfis | SolidJS + Vite |
| veloryft-db | Schema e migrations do banco | PostgreSQL + TimescaleDB |

**Fluxo:** Forza → *(UDP :20066)* → **veloryft-hud** → *(HTTPS, lote ao fechar a
volta)* → edge TLS → veloryft-back → veloryft-db. O front consome a API. Pagamentos
via Stripe.

**Princípios:** monólito modular (sem microserviços de início), separação por
responsabilidade, o front nunca toca o banco (back-to-back / DMZ), licença
**server-authoritative** (nunca confiar no cliente).

## Papel deste repositório

1. **Captura de telemetria** — thread Rust faz `recv_from` na porta UDP do Forza,
   parseia o pacote por offset (`forza/packet.rs`) e mantém um snapshot lock-free
   (`forza/state.rs`, arc-swap).
2. **Render do HUD in-game** — overlay nativo **Direct2D** em janela
   `WS_EX_LAYERED | WS_EX_TRANSPARENT` com click-through real (o clique vai pro
   jogo), composto via `UpdateLayeredWindow`. Sem o judder do WebView2.
3. **Buffer + upload** *(a implementar)* — bufferiza a sessão em SQLite e envia a
   volta completa ao `veloryft-back` ao fechar a volta.
4. **Licença** *(a implementar)* — embute a **chave pública Ed25519**, faz binding
   por fingerprint de hardware e heartbeat online; só envia telemetria com token
   de sessão válido.

## Decisão de render: tudo em Direct2D (inclusive a Fase 2)

O foco é **performance sem perder o design**. A UI rica de 8 painéis (Fase 2) será
desenhada em **Direct2D nativo** — a mesma pipeline do overlay atual (DIB
premultiplicado + `UpdateLayeredWindow`), e **não** em WebView2.

Motivo: a janela transparente do WebView2 passa pelo *redirection bitmap* e o DWM
mescla a camada alpha a cada frame, gerando judder que nenhum ajuste de CSS/rAF
resolve. Direct2D dá composição GPU com alpha por pixel, RAM mínima e o controle
visual (gradiente, cantos, anti-alias) para manter a beleza.

O app **Tauri/WebView2** fica restrito a janelas **fora do jogo** (configuração,
login/licença), nunca no caminho de render in-game.

## Fronteiras

- **Faz:** captura, parsing, render, buffer local, upload de voltas, validação de
  licença no cliente (verificação, não emissão).
- **Não faz:** ranking, persistência central, emissão de licença, cobrança — isso é
  do `veloryft-back`.

## Relação com os outros repos

- Compartilha o crate **`veloryft-core`** com o `veloryft-back` (tipos `Telemetry` +
  parsing) — fonte única de verdade, sem divergência de schema.
- Envia voltas para o `veloryft-back`; os dados aparecem no `veloryft-front`.
