# Estrutura do Projeto — Veloryft

Este documento descreve a organização de pastas e a responsabilidade de cada módulo e arquivo do Veloryft HUD.

## Árvore de alto nível

```
veloryft-hud/
├── README.md                       # Visão geral e porta de entrada do repositório
├── .cargo/
│   └── config.toml                 # Configurações do Cargo: CRT estático e check-revoke desligado
├── data/
│   └── car-ordinals.json           # Mapeamento nome do carro → ordinal_id do Forza
├── docs/                           # Documentação do produto e do projeto
│   ├── INDEX.md                    # Índice central da documentação
│   ├── INICIO.md                   # Guia de início rápido
│   ├── ARQUITETURA.md              # Posição deste repo na plataforma Veloryft
│   ├── ESTRUTURA.md                # Este arquivo
│   ├── TECNOLOGIAS.md              # Stack tecnológico completo
│   ├── INTERFACE.md                # Guia da interface do HUD
│   ├── DESENVOLVIMENTO.md          # Guia do desenvolvedor
│   ├── DISTRIBUICAO.md             # Build e distribuição do instalador
│   ├── LICENCA.md                  # Provisionamento e fluxo de licenças
│   ├── SEGURANCA.md                # Considerações de segurança
│   └── REFERENCIAS.md              # Links e referências externas
├── installer/                      # Assets e scripts de distribuição
│   ├── build-installer.ps1         # Build de ponta a ponta: cargo + Inno Setup
│   ├── veloryft.iss                # Script do instalador Inno Setup 6
│   ├── EULA.txt                    # Termos de uso (inglês)
│   ├── README.txt                  # Guia pós-instalação (inglês)
│   └── assets/                     # Ícone e imagens do wizard
│       ├── veloryft.ico
│       ├── make_assets.py
│       └── wizard-*.bmp
└── src-tauri/                      # Código-fonte Rust do aplicativo
    ├── Cargo.toml                  # Manifest do crate e dependências
    ├── build.rs                    # Embed de ícone, version info e config de licença
    └── src/
        ├── main.rs                 # Entry point: autorização → overlay
        ├── lib.rs                  # Raiz dos módulos
        ├── render.rs               # Overlay Direct2D: painéis, hotkeys, edição, bandeja
        ├── license.rs              # Tipos e funções criptográficas públicas de licença
        ├── license/                # Implementação do cliente de licença
        │   ├── client.rs           # Fluxo de autorização na inicialização
        │   ├── config.rs           # Configuração embutida de origem, chaves e pins
        │   ├── fingerprint.rs      # Coleta de fingerprint de hardware
        │   ├── state.rs            # Persistência DPAPI + high-water marks
        │   └── tpm.rs              # Integração com CNG/TPM
        └── forza/                  # Núcleo de telemetria
            ├── mod.rs              # Constantes e exports do módulo
            ├── udp.rs              # Thread de recepção UDP
            ├── packet.rs           # Parsing dos pacotes Sled/Dash/FH6
            ├── payload.rs          # Construção do objeto Telemetry
            ├── state.rs            # Snapshot compartilhado lock-free
            ├── display.rs          # Suavização e DisplayFrame
            ├── revlimit.rs         # Aprendizado de limite de giro
            ├── elevation.rs        # Perfil de elevação da volta
            ├── tires.rs            # Acompanhamento de temperatura dos pneus
            ├── visibility.rs       # Regras de visibilidade do HUD
            ├── catalog.rs          # Lookup de nomes de carros
            ├── worldmap.rs         # Gravador de mapa (desligado)
            └── live_api.rs         # Servidor HTTP local (desligado)
```

## Responsabilidade por pasta

### `src-tauri/src/`

Contém o aplicativo Rust propriamente dito. O nome `src-tauri` é herança histórica de uma fase em que o projeto considerava usar Tauri; hoje o executável é um binário Win32 nativo, mas a estrutura de pasta foi mantida.

#### `main.rs`

Ponto de entrada do binário `forza-hud-native`.

- Chama `license::authorize_startup()`.
- Em caso de sucesso, inicia `render::run()`.
- Em caso de falha de licença, exibe uma `MessageBoxW` com mensagem amigável e encerra.
- Em builds não-Windows, encerra com mensagem de erro.

#### `lib.rs`

Raiz do crate. Declara os módulos públicos:

- `pub mod forza;`
- `pub mod license;`
- `#[cfg(windows)] pub mod render;`

#### `render.rs`

Maior arquivo do projeto (~1700 linhas). Responsável por:

- Criar janelas `WS_EX_LAYERED | WS_EX_TRANSPARENT` para cada painel.
- Inicializar fábricas Direct2D, DirectWrite e DXGI.
- Loop de renderização a ~60 fps com dirty-skip e sleep adaptativo.
- Priorização do cluster e degradação adaptativa dos painéis auxiliares conforme
  a latência de apresentação; detalhes em [`RENDERIZACAO.md`](RENDERIZACAO.md).
- Hotkeys globais: `Ctrl+Alt+Q`, `Ctrl+Alt+E`.
- Modo de edição: malha de grade, drag-and-drop, resize, toolbar de customização.
- Persistência de posição, tamanho, tema e visibilidade dos painéis.
- Ícone na bandeja do sistema com menu de contexto.
- Leitura do snapshot de telemetria via `ArcSwap`.

### `src-tauri/src/forza/`

Núcleo de captura e processamento de telemetria.

#### `mod.rs`

Exporta os submódulos e define a constante `TRACK_MAPPING_ENABLED`, que controla se a telemetria de mapeamento de pista está ativa. No lançamento inicial vale `false`.

#### `udp.rs`

Thread dedicada de recepção UDP. Faz `bind` na porta configurada, lê datagramas em loop, chama `packet::parse`, `payload::build` e armazena o resultado no `Shared` snapshot. Aceita um callback opcional — usado pelo gravador de pista quando ligado.

#### `packet.rs`

Parser zero-copy dos pacotes "Data Out" do Forza. Suporta três formatos:

- **Sled** (232 bytes)
- **Dash** (311 bytes)
- **FH6** (323 bytes)

Fornece acessores tipados por offset (ex.: `engine_rpm()`, `speed()`, `gear()`).

#### `payload.rs`

Converte um `Packet` em um `Telemetry`, estrutura derivada usada pelo restante do aplicativo. Inclui:

- Conversão de m/s para km/h.
- Normalização de pedais e volante.
- Cálculo de força G.
- Lookup de nome do carro via `catalog`.

#### `state.rs`

Snapshot compartilhado e lock-free baseado em `ArcSwap`. A thread UDP escreve um novo `Arc<Telemetry>` a cada pacote; o render lê o ponteiro atual sem bloqueios.

#### `display.rs`

Suavização exponencial dos valores brutos para exibição. Produz um `DisplayFrame` consumido pelo `render.rs`. Coordena os submódulos:

- `revlimit.rs` — limite de giro.
- `elevation.rs` — perfil de elevação.
- `tires.rs` — temperatura dos pneus.
- `visibility.rs` — quando mostrar/esconder o HUD.

#### `revlimit.rs`

Aprendizado por carro do corte do limitador e do ponto ideal de troca de marcha. Persiste em `%APPDATA%/forza-hud-native/rev-limits.json`.

#### `elevation.rs`

Mapeia o perfil de elevação da pista na primeira volta e reutiliza o loop fechado nas voltas seguintes.

#### `tires.rs`

Acompanha a temperatura mínima e máxima de cada pneu durante a sessão, com código de cor por faixa.

#### `visibility.rs`

Define quando o HUD deve estar visível. Esconde em menus, pausas, cinemáticas e free roam parado. Inclui testes unitários.

#### `catalog.rs`

Carrega `data/car-ordinals.json` no startup e resolve `car_ordinal` para nome legível do veículo.

#### `worldmap.rs` e `live_api.rs`

Funcionalidades de mapeamento de pista. Desligadas no lançamento inicial (`TRACK_MAPPING_ENABLED = false`).

### `src-tauri/src/license/`

Implementação do cliente de licenças Veloryft.

#### `license.rs`

Módulo raiz. Define tipos públicos:

- `LicenseError`
- `RequestAttestation`
- `SignedResponseHeaders`
- `TrustedResponseKey`
- `EntitlementClaims`

Também implementa as funções criptográficas centrais: `attest_request`, `verify_response`, `verify_token`.

#### `license/client.rs`

Orquestra o fluxo de autorização na inicialização:

- Coleta fingerprint.
- Carrega estado anterior.
- Decide entre trial, ativação com chave ou validação.
- Faz requisições HTTPS assíncronas com Tokio.

#### `license/config.rs`

Lê a configuração embutida em tempo de compilação (origin, HMAC, chaves Ed25519, pins SPKI) e monta o cliente HTTP com TLS pinado.

#### `license/fingerprint.rs`

Coleta informações de hardware via WMI (UUID, CPU, baseboard) e produz um identificador vinculado à máquina.

#### `license/state.rs`

Persiste o token de licença usando DPAPI do Windows, com mirrors de high-water mark e sidecar TPM para defesa contra rollback e cópia de estado.

#### `license/tpm.rs`

Integração com o CNG (Cryptography API: Next Generation) para criar uma chave ECDSA P-256 não-exportável e assinar o high-water mark quando o TPM está disponível.

### `installer/`

Contém tudo necessário para gerar o instalador Windows.

#### `build-installer.ps1`

Script de build de ponta a ponta:

1. Regenera assets (opcional, `-Assets`).
2. Valida variáveis de licença.
3. Compila o release.
4. Invoca o Inno Setup.

#### `veloryft.iss`

Script do instalador. Define wizard moderno, EULA obrigatória, atalhos, página de ativação opcional e fechamento automático do app.

#### `EULA.txt` e `README.txt`

Textos exibidos ao usuário durante e após a instalação. Ambos em inglês.

#### `assets/`

- `veloryft.ico`: ícone do executável.
- `wizard-large*.bmp` / `wizard-small*.bmp`: artes do wizard em variações light/dark e @2x.
- `make_assets.py`: script Python+Pillow para regenerar as artes.

### `data/`

`car-ordinals.json` mapeia nomes de carros para seus `car_ordinal` no Forza. Usado para exibir o nome do veículo no HUD quando disponível.

### `.cargo/`

`config.toml` configura:

- `check-revoke = false` para evitar falhas de download de crates em redes restritas.
- `target-feature=+crt-static` para binário standalone sem VC++ Redist.

## Convenções de código

- Cada módulo começa com um docstring explicando sua responsabilidade.
- Constantes de layout e tema ficam no topo dos arquivos de renderização.
- Testes unitários ficam em `#[cfg(test)]` no mesmo arquivo quando são curtos e diretos (ex.: `visibility.rs`, `tires.rs`).
- Segredos de build nunca são hardcoded; entram via variáveis de ambiente processadas por `build.rs`.
