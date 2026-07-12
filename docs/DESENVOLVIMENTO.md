# Guia do Desenvolvedor — Veloryft

Este documento orienta o dia a dia de quem trabalha no código do Veloryft HUD: como compilar, testar, depurar e contribuir com segurança.

## Ambiente de desenvolvimento

### Pré-requisitos obrigatórios

1. **Rust** com toolchain MSVC:
   ```powershell
   rustup default stable-x86_64-pc-windows-msvc
   ```

2. **Visual Studio Build Tools** ou Visual Studio Community com a carga de trabalho **"Desenvolvimento para desktop com C++"**.

3. **Git** para clonar o repositório e gerenciar `veloryft-core`.

### Pré-requisitos opcionais

- **Inno Setup 6** se for gerar instaladores.
- **Python 3 + Pillow** se for regenerar assets de marca.

## Organização do workspace

Espera-se a seguinte estrutura:

```
workspace-pai/
├── veloryft-hud/        <- este repositório
└── veloryft-back/
    └── crates/
        └── core/          <- dependência de path
```

Se você não tiver `veloryft-back` disponível, o build falhará na dependência `veloryft-core`. Nesse caso, você pode:

- Clonar o repositório `veloryft-back` lado a lado.
- Criar um crate stub local com os tipos mínimos necessários (não recomendado para trabalho sério).

## Compilação

### Debug

```powershell
cargo run --manifest-path src-tauri/Cargo.toml
```

Use este comando para desenvolvimento. Ele é rápido para recompilar e não exige configuração de licença.

### Release

```powershell
$env:VELORYFT_LICENSE_ORIGIN = 'https://license.seu-dominio.com'
$env:VELORYFT_LICENSE_APP_HMAC_SECRET = '<secret>'
$env:VELORYFT_LICENSE_RESPONSE_PUBLIC_KEYS = '<pub1-base64>;<pub2-base64>'
$env:VELORYFT_LICENSE_SPKI_PINS = '<pin1-hex>;<pin2-hex>'

cargo build --release --manifest-path src-tauri/Cargo.toml
```

Release **sempre** exige as quatro variáveis acima. Isso é garantido por `build.rs`.

## Testes

### Todos os testes

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
```

### Testes de um módulo específico

```powershell
cargo test --manifest-path src-tauri/Cargo.toml visibility::
cargo test --manifest-path src-tauri/Cargo.toml tires::
```

### Testes de licença

O módulo `license` possui testes que verificam o contrato criptográfico sem depender de rede. Execute:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml license::
```

## Depuração

### Console do Windows

Em debug, o aplicativo é um console app. Você verá mensagens de erro no terminal. Em release, o subsistema Windows é usado e erros só aparecem em `MessageBoxW` (para licença) ou são silenciados.

### Logs

O projeto não possui um sistema de logging estruturado. Mensagens de erro são enviadas para `stderr` com prefixos como `[overlay]` ou `[forza-udp]`.

Se precisar de mais visibilidade, considere adicionar temporariamente `eprintln!` em pontos de interesse, mas remova antes de commitar.

### Modo edição como ferramenta de debug

Mesmo sem o jogo, você pode testar o render pressionando **Ctrl+Alt+E**. Isso força a exibição dos painéis e permite verificar:

- Inicialização do Direct2D.
- Criação das janelas layered.
- Layout e cores.
- Interação com a toolbar.

## Convenções de código

### Formatação

Use `rustfmt`:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
```

### Análise estática

Use `clippy` para capturar problemas comuns:

```powershell
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

### Estrutura de commits

Sugestão de padrão para mensagens:

```
<escopo>: <descrição curta>

<corpo opcional explicando o porquê>
```

Exemplos de escopos: `render`, `forza`, `license`, `installer`, `docs`.

## Trabalhando com o overlay

### Adicionar um novo painel

1. Defina um novo `Kind` e `PanelKind` em `render.rs` / `display.rs`.
2. Adicione constantes de tamanho padrão e posição padrão.
3. Implemente a função de desenho no loop de renderização.
4. Adicione um botão de toggle na toolbar.
5. Persista estado em `load_theme` / `save_theme` ou funções equivalentes.

### Alterar a lógica de visibilidade

Edite `forza/visibility.rs`. Inclua testes unitários para cobrir novos cenários (menu, pausa, cinemática, free roam).

### Alterar o parser de pacotes

Edite `forza/packet.rs`. Respeite os offsets e tamanhos dos formatos Sled/Dash/FH6. Considere que a comunidade do Forza documenta esses layouts — consulte as referências em [REFERENCIAS.md](REFERENCIAS.md).

### Alterar o sistema de licença

> ⚠️ Qualquer mudança em `license*` deve ser revisada por pelo menos duas pessoas.

1. Mantenha o contrato criptográfico documentado em [LICENCA.md](LICENCA.md).
2. Atualize testes unitários que verificam assinatura, HMAC e validação de token.
3. Garanta que builds debug continuem funcionando sem segredos de licença.

## Build do instalador durante o desenvolvimento

Se você precisar testar o instalador sem configurar licença, pode compilar o binário em debug e ajustar o `veloryft.iss` para apontar para ele. No entanto, o script `build-installer.ps1` exige configuração de licença para release.

```powershell
# Caminho manual para testes
& "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" installer\veloryft.iss
```

## Checklist antes de abrir um pull request

- [ ] `cargo fmt` executado.
- [ ] `cargo clippy -- -D warnings` sem erros.
- [ ] `cargo test` passando.
- [ ] Build debug inicia e modo de edição funciona.
- [ ] Nenhum segredo de licença foi adicionado ao código.
- [ ] Documentação atualizada se comportamento do usuário mudou.

## Recursos úteis

- [TECNOLOGIAS.md](TECNOLOGIAS.md): detalhes das crates.
- [ESTRUTURA.md](ESTRUTURA.md): organização dos arquivos.
- [SEGURANCA.md](SEGURANCA.md): cuidados com licenças e segredos.
- [REFERENCIAS.md](REFERENCIAS.md): links externos.
