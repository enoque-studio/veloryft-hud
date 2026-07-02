# veloryft-hud — papel na arquitetura Veloryft

> Cliente desktop (Windows) que captura a telemetria do **Forza Horizon 6** e
> desenha o HUD in-game. É o **produtor** dos dados que alimentam a plataforma.

## Visão geral — o sistema Veloryft

Veloryft são **dois produtos pagos** distribuídos em **quatro repositórios**:

| Repositório | Papel | Stack |
|---|---|---|
| **veloryft-hud** | Cliente desktop: captura telemetria do Forza e exibe o HUD | Rust + Direct2D |
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

## Decisão de render: Direct2D nativo (HUD in-game)

O HUD in-game é o **binário padrão** (`main.rs` → `render.rs`): overlay Direct2D
com `WS_EX_LAYERED | WS_EX_TRANSPARENT` e `UpdateLayeredWindow`. **Não** usa
WebView2/Tauri.

Telas **fora do jogo** (configuração, login/licença) ainda não existem; quando
forem necessárias, entram como camada própria — fora do caminho de render in-game.

## Lançamento inicial — mapeamento de pista desligado

O escopo imediato é o **HUD in-game** (overlay Direct2D). A telemetria de
**mapeamento de pista** — `WorldMapRecorder`, live API local (`:9877`), autosave
do heightmap e upload para o `veloryft-back` — está **desligada** até a
plataforma estar pronta para a galera.

Controle: `TRACK_MAPPING_ENABLED` em `src-tauri/src/forza/mod.rs` (hoje `false`).
Com desligado, o overlay não sobe a live API nem grava células de pista; a captura
UDP e os painéis do HUD continuam normais.

## Fronteiras

- **Faz:** captura, parsing, render, buffer local, upload de voltas, validação de
  licença no cliente (verificação, não emissão).
- **Não faz:** ranking, persistência central, emissão de licença, cobrança — isso é
  do `veloryft-back`.

## Relação com os outros repos

- Compartilha o crate **`veloryft-core`** com o `veloryft-back` (tipos `Telemetry` +
  parsing) — fonte única de verdade, sem divergência de schema.
- Envia voltas para o `veloryft-back`; os dados aparecem no `veloryft-front`.
