# Licenca do HUD

O HUD usa o `veloryft-license` como autoridade de trial e entitlement. Em um
build de distribuicao, a origem HTTPS, o HMAC, as chaves Ed25519 de resposta e
os pins SPKI entram no binario durante a compilacao. Eles nunca sao lidos de
arquivo, registro, variavel de ambiente ou URL depois que o aplicativo inicia.

## Provisionamento do release

O servidor precisa estar publicado por um proxy HTTPS em um hostname fixo. A
porta HTTP interna do container permanece privada. Antes de gerar o HUD,
defina no processo de build:

```powershell
$env:VELORYFT_LICENSE_ORIGIN = 'https://license.seu-dominio.example'
$env:VELORYFT_LICENSE_APP_HMAC_SECRET = '<o mesmo VELORYFT_APP_HMAC_SECRET do servidor>'
$env:VELORYFT_LICENSE_RESPONSE_PUBLIC_KEYS = '<ed25519-publica-atual-base64>;<ed25519-publica-reserva-base64>'
$env:VELORYFT_LICENSE_SPKI_PINS = '<sha256-spki-atual-em-hex-minusculo>;<sha256-spki-reserva-em-hex-minusculo>'
pwsh installer/build-installer.ps1
```

O script e o `build.rs` recusam um build `release` sem todos os valores e sem
um par atual/reserva para chaves e pins. O HMAC faz parte do binario por
defesa adicional contra automacao; ele nao deve ser comitado nem impresso em
logs de CI. Os artefatos em `src-tauri/target` tambem ficam fora do controle de
versao porque carregam esse valor durante o build.

Gere o par Ed25519 atual e o reserva antecipadamente com o utilitario do
servico. A seed correspondente ao reserva fica guardada no secret manager e so
substitui `VELORYFT_SIGNING_KEY` depois que uma versao do HUD contendo as duas
chaves estiver instalada. Calcule cada pin como SHA-256 do DER
`SubjectPublicKeyInfo` do certificado final do proxy, nao como hash do arquivo
PEM inteiro.

## Fluxo do usuario

Sem estado local, o HUD inicia `POST /v1/trial/start`. Nas proximas execucoes
ele usa `POST /v1/validate` com o token anterior. Uma resposta autenticada e
ativa e persistida com DPAPI no escopo do usuario; o token pode ser usado sem
rede somente ate o prazo `exp` assinado e sem rollback relevante do relogio.

O instalador tem um campo opcional de chave. Quando preenchido, a primeira
execucao recebe `--activate VLRY-XXXX-XXXX-XXXX-XXXX` e chama
`/v1/license/activate`; a chave nao entra no estado DPAPI nem em logs do HUD.
Uma ativacao posterior usa o mesmo argumento no executavel. A chave ainda pode
ser visivel temporariamente a processos do mesmo usuario pelo command line,
portanto o fluxo de distribuicao deve evitar historico de shell e telemetria de
argumentos.

O HUD nunca abre URLs de resposta de licenca e nao baixa ou executa updates.
Um atualizador futuro precisa de manifest Ed25519 separado, allowlist de host,
hash/tamanho do artefato e verificacao Authenticode antes de executar qualquer
instalador.