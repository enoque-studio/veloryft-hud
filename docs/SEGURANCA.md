# Segurança — Veloryft

Este documento descreve o modelo de segurança do Veloryft HUD: o que é defendido, como é defendido e quais são os limites do design.

> ⚠️ Este documento é de leitura obrigatória para qualquer pessoa que vá tocar no sistema de licenças, gerar builds de release ou administrar os segredos de build.

## Princípios fundamentais

1. **Server-authoritative:** o cliente nunca decide sozinho se tem licença válida. Ele verifica assinaturas e tokens emitidos pelo servidor.
2. **Zero leitura runtime de segredos:** origem do serviço, HMAC secret, chaves públicas e pins SPKI entram no binário em tempo de compilação. Em runtime, nada é lido de arquivo, registro, variável de ambiente ou URL.
3. **Bind de máquina:** o token de licença é criptografado com DPAPI e, quando possível, ancorado a um TPM, dificultando a transferência entre computadores.
4. **Defense in depth:** cada mecanismo tem fallback e múltiplas camadas (assinatura + HMAC + TLS pinning + DPAPI + TPM + high-water marks).

## Ameaças defendidas

| Ameaça | Mecanismo de defesa |
|--------|---------------------|
| Modificação do pacote de resposta de licença | Assinatura Ed25519 do corpo + headers + nonce da requisição. |
| MITM no canal de licença | TLS 1.2+ com pinagem SPKI do certificado final. |
| Automação de requisições | HMAC-SHA256 sobre corpo canonicalizado, com nonce e timestamp. |
| Cópia do estado de licença para outra máquina | DPAPI (máquina/usuário) + TPM signature de high-water mark. |
| Rollback do relógio do sistema | High-water marks de relógio espelhadas em DPAPI e TPM. |
| Token vazado em logs | Tokens, chaves e fingerprints nunca são impressos em stderr/log. |
| Engenharia reversa fácil do binário | Perfil release com strip de símbolos, LTO e panic=abort. |

## Componentes criptográficos

### HMAC-SHA256 (autenticação de requisição)

Cada requisição ao servidor de licença é atestada com HMAC-SHA256 sobre uma string canonicalizada:

```
VELORYFT-REQUEST-V1\n<method>\n<path>\n<timestamp>\n<nonce>\n<sha256(body)>
```

- O HMAC secret do aplicativo (`VELORYFT_LICENSE_APP_HMAC_SECRET`) é embutido no binário.
- O nonce é gerado com `OsRng` e nunca é reutilizado.
- O timestamp deve estar dentro da tolerância de skew do servidor.

### Ed25519 (assinatura de resposta)

O servidor assina o corpo da resposta e metadados. O cliente verifica com até duas chaves públicas embutidas:

- Uma chave **atual**.
- Uma chave **reserva**, para rotação sem quebrar clientes antigos.

A canonicalização da resposta inclui status HTTP, nonce da requisição, timestamp, key ID e hash SHA-256 do corpo.

### TLS + Pinagem SPKI

O cliente `reqwest`/`rustls` verifica o certificado do servidor contra dois pins SPKI:

- Cada pin é o SHA-256 do DER `SubjectPublicKeyInfo` do certificado final.
- Pins são fornecidos em hex minúsculo, separados por `;`.
- Dois pins permitem rotação de certificado sem interrupção.

### DPAPI (persistência local)

O token de licença é criptografado com DPAPI do Windows no escopo do usuário. Isso significa:

- O arquivo de estado não pode ser lido em outra máquina.
- O arquivo não pode ser lido por outro usuário do mesmo Windows (exceto administradores com acesso a chaves mestras).

### TPM (anti-rollback e anti-cópia)

Quando disponível, o cliente usa o CNG para criar uma chave ECDSA P-256 **não-exportável** no TPM. Essa chave assina o high-water mark de relógio, criando um sidecar que:

- Pode ser verificado sem revelar a chave privada.
- Torna mais difícil copiar o estado de licença para outra máquina, pois a chave não sai do TPM original.

## High-water marks

Para evitar rollback de relógio e reutilização de tokens antigos, o cliente mantém:

1. `response_high_water`: maior timestamp de resposta autorizada (mirror DPAPI).
2. `clock_high_water`: maior relógio local observado (mirror DPAPI).
3. `clock_high_water` + assinatura TPM (sidecar).

A tolerância de retrocesso do relógio é de **5 minutos**. Retroceder além disso invalida o acesso offline e exige revalidação online.

## Segredos de build

Os seguintes valores são processados por `build.rs` e embutidos no binário:

| Variável | Descrição | Mínimo em release |
|----------|-----------|-------------------|
| `VELORYFT_LICENSE_ORIGIN` | URL base do serviço de licença. | 1 valor |
| `VELORYFT_LICENSE_APP_HMAC_SECRET` | Segredo HMAC do app. | 1 valor, ≥ 32 bytes |
| `VELORYFT_LICENSE_RESPONSE_PUBLIC_KEYS` | Chaves públicas Ed25519 em base64. | 2 valores (atual + reserva) |
| `VELORYFT_LICENSE_SPKI_PINS` | Pins SHA-256 de SPKI em hex. | 2 valores (atual + reserva) |

### ⚠️ Regras de ouro

- **Nunca** comite esses valores.
- **Nunca** imprima esses valores em logs de CI/CD.
- **Nunca** os passe via linha de comando em um shell que salva histórico.
- Os artefatos de build em `src-tauri/target` contêm esses segredos; mantenha-os fora do controle de versão e limpe-os após o build quando apropriado.

## Rotação de chaves

1. Gere um par Ed25519 atual e um par reserva.
2. Faça um build com **ambas** as chaves públicas e ambos os pins SPKI.
3. Após a maioria dos clientes estarem atualizados, promova a chave reserva para atual no servidor.
4. Gere um novo par reserva e repita o ciclo.

Essa janela de sobreposição garante que clientes antigos ainda aceitem respostas assinadas pela chave que conhecem.

## Limites do modelo

| Cenário | Limitação |
|---------|-----------|
| Snapshot completo de VM ou disco | DPAPI pode ser decifrado no mesmo ambigo; o modelo depende de revalidação online periódica para mitigar. |
| Administrador local | Admin pode extrair chaves DPAPI do usuário; DPAPI protege contra outros usuários, não contra admin do mesmo sistema. |
| Desativação do TPM | O fallback para DPAPI puro continua funcionando, mas perde a camada anti-cópia do TPM. |
| Engenharia reversa do binário | Rust release dificulta, mas não impossibilita. O HMAC secret pode ser extraído de um binário comprometido. |
| Chave de ativação via linha de comando | A chave pode aparecer no histórico de processos; instrua usuários a não compartilharem e evite logs de CLI. |

## Boas práticas para desenvolvedores

1. Sempre execute `cargo run` em debug para desenvolvimento — nesse modo, a ausência de configuração de licença é aceita.
2. Nunca rode `cargo build --release` sem as quatro variáveis de licença; o `build.rs` rejeitará, e isso é intencional.
3. Ao depurar problemas de licença, confie apenas nas mensagens de erro públicas (`LicenseError`); não exponha nonces, tokens ou fingerprints.
4. Mantenha o diretório `src-tauri/target` fora de backups públicos e do controle de versão.

## Responsabilidades

| Papel | Responsabilidade |
|-------|------------------|
| Desenvolvedor | Não comitar segredos; usar builds debug para testes locais. |
| DevOps / Release | Configurar variáveis de ambiente de build de forma segura; limpar artefatos. |
| Admin de licenças | Gerar e rotacionar pares Ed25519; calcular pins SPKI corretamente. |
| Jurídico | Revisar EULA e termos antes de distribuição em escala. |
