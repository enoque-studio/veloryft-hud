# Documentação Veloryft

Bem-vindo à central de documentação do **Veloryft**, o overlay de telemetria nativo para Forza Horizon 6.

Esta pasta reúne tudo o que você precisa para entender, executar, desenvolver e distribuir o projeto — de leitura obrigatória para novos integrantes da equipe até referências técnicas para mantenedores.

## Para quem está começando

| Documento | O que você encontra |
|-----------|---------------------|
| [INICIO.md](INICIO.md) | Requisitos, instalação e primeiro run do HUD em menos de 10 minutos. |
| [ARQUITETURA.md](ARQUITETURA.md) | Visão de alto nível: onde este repo se encaixa na plataforma Veloryft. |
| [ESTRUTURA.md](ESTRUTURA.md) | Organização de pastas e responsabilidade de cada módulo. |

## Para desenvolvedores

| Documento | O que você encontra |
|-----------|---------------------|
| [TECNOLOGIAS.md](TECNOLOGIAS.md) | Stack completo: linguagens, crates, ferramentas de build e por quê cada uma foi escolhida. |
| [DESENVOLVIMENTO.md](DESENVOLVIMENTO.md) | Como compilar, testar, depurar e trabalhar com o código no dia a dia. |
| [INTERFACE.md](INTERFACE.md) | Guia completo dos painéis do HUD, atalhos e modo de edição. |

## Para release e operações

| Documento | O que você encontra |
|-----------|---------------------|
| [DISTRIBUICAO.md](DISTRIBUICAO.md) | Como gerar o instalador Windows, pré-requisitos, certificados e checklist de release. |
| [LICENCA.md](LICENCA.md) | Provisionamento do serviço de licenças, fluxo de ativação e rotação de chaves. |
| [SEGURANCA.md](SEGURANCA.md) | Ameaças defendidas, limites do modelo e boas práticas para proteger os segredos de build. |

## Referências

| Documento | O que você encontra |
|-----------|---------------------|
| [REFERENCIAS.md](REFERENCIAS.md) | Links úteis: documentação de crates, protocolo UDP do Forza, APIs e ferramentas. |

## Convenções desta documentação

- **Caminhos** são relativos à raiz do repositório, exceto quando indicado o contrário.
- `%APPDATA%` refere-se a `C:\Users\<usuário>\AppData\Roaming` no Windows.
- O nome do produto voltado ao usuário é **Veloryft**; `veloryft-hud` é apenas o nome deste repositório.
- Seções marcadas com `⚠️` exigem atenção especial antes de release ou compartilhamento.

---

> **Dica:** se você acabou de chegar ao projeto, comece por [INICIO.md](INICIO.md) e depois leia [ARQUITETURA.md](ARQUITETURA.md).
