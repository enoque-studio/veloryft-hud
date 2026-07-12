# Interface do HUD — Veloryft

Este documento descreve a experiência do usuário com o overlay in-game do Veloryft: painéis, atalhos, modo de edição e personalização.

## Visão geral

O Veloryft exibe até **quatro painéis independentes** sobrepostos ao jogo. Cada painel é uma janela layered própria, com click-through real: o clique do mouse passa através do HUD e chega ao jogo, exceto no modo de edição.

## Painéis

### 1. Cluster (RPM / Marcha / Velocidade)

**Tamanho padrão:** 680 × 240 px  
**Posição padrão:** centralizado na parte inferior da tela

Elementos visuais:

- **Barra de RPM** horizontal na parte superior.
  - Fundo escuro translúcido.
  - Preenchimento que acompanha a rotação do motor.
  - **Tick branco**: ponto de troca recomendado, aprendido por carro.
  - **Tick vermelho / banda vermelha**: zona até o corte do limitador, quando já observado.
- **Marcha** em destaque no centro-esquerdo.
  - Pisca com a cor de destaque quando o RPM atinge o ponto de troca.
- **Velocidade** em km/h.
- **Pedais**: barras de acelerador (verde) e freio (vermelho).
- **Força G**: leitura lateral e longitudinal.
- **Potência instantânea** em cv (quando disponível no pacote Dash/FH6).

### 2. Pneus

**Tamanho padrão:** 256 × 272 px  
**Posição padrão:** canto inferior esquerdo

Elementos visuais:

- Representação 2D das quatro rodas (FL, FR, RL, RR).
- Temperatura atual de cada pneu.
- Indicadores de mínimo/máximo da sessão.
- Cores por faixa:
  - Azul: frio (< 150 °F)
  - Verde: ideal (150–205 °F)
  - Âmbar: quente (205–245 °F)
  - Vermelho: muito quente (> 245 °F)

### 3. G-Force

**Tamanho padrão:** 200 × 250 px  
**Posição padrão:** canto inferior direito

Elementos visuais:

- Gráfico polar de aceleração lateral (eixo X) e longitudinal (eixo Y).
- Traço dinâmico que registra os picos recentes.
- Fade suave nas amostras antigas.

### 4. Elevação

**Tamanho padrão:** 300 × 200 px  
**Posição padrão:** canto superior direito

Elementos visuais:

- Perfil de elevação do circuito.
- Na primeira volta, o traçado é mapeado automaticamente.
- Quando o loop é fechado, o perfil é reutilizado nas voltas seguintes.
- Bolinha indicadora de posição atual.
- Escala de elevação mínima e máxima.

## Atalhos globais

| Atalho | Ação |
|--------|------|
| **Ctrl + Alt + E** | Entra ou sai do modo de edição. |
| **Ctrl + Alt + Q** | Fecha o Veloryft. |

> **Nota:** os atalhos são registrados como hotkeys globais do Windows. Eles funcionam mesmo quando o jogo está em foco.

## Modo de edição

Ao pressionar **Ctrl + Alt + E**, o HUD entra em modo de edição. Nesse modo:

- Uma **malha de grade** aparece sobre a tela para auxiliar o alinhamento.
- Os painéis ficam **arrastáveis**: clique no centro e arraste.
- As bordas e cantos dos painéis permitem **redimensionar**.
- Uma **barra de ferramentas** flutuante aparece, oferecendo:
  - Paleta de cores de destaque.
  - Alternância do alvo do destaque (marcha ou card).
  - Botões de visibilidade de cada painel.
  - Slider **VIDRO** para ajustar a opacidade do fundo.
  - Slider **TOM** para ajustar a matiz do vidro (ou deixá-lo neutro).

### Ajuste fino de posição

Os painéis se encaixam suavemente na grade quando estão próximos o suficiente de uma linha (comportamento de "ímã suave"). Isso mantém o layout limpo sem forçar um posicionamento totalmente quantizado.

### Personalização de cor

- **Roda do mouse** sobre a paleta de cores altera o matiz de destaque.
- A cor de destaque é usada no aviso de troca de marcha e/ou no card, conforme selecionado.

### Opacidade e tom do vidro

- **Slider VIDRO:** controla o alpha do fundo dos cards. Valores baixos deixam o jogo mais visível; valores altos aumentam o contraste do HUD.
- **Slider TOM:** à esquerda, deixa o vidro em tom neutro escuro; à direita, aplica uma matiz colorida sutil.

### Visibilidade dos painéis

Os botões da toolbar permitem mostrar ou ocultar cada painel individualmente. Painéis ocultos não consomem recursos de renderização.

## Ícone da bandeja do sistema

Quando em execução, o Veloryft cria um ícone na bandeja do Windows (área de ícones ocultos por padrão).

| Ação | Resultado |
|------|-----------|
| Clique duplo | Alterna mostrar/ocultar o overlay. |
| Clique direito | Abre menu com opções **Mostrar/Ocultar** e **Sair**. |

## Comportamento de visibilidade in-game

O HUD aparece automaticamente apenas quando o jogador está de fato dirigindo. Ele se esconde em:

- Menus principais.
- Telas de pausa.
- Cinemáticas e câmeras scriptadas.
- Free roam completamente parado.

Essa lógica é implementada em `forza/visibility.rs` e usa `is_race_on` do pacote do Forza combinado com idade do pacote e input do jogador.

## Persistência de preferências

Todas as personalizações feitas no modo de edição são salvas automaticamente em:

```
%APPDATA%\forza-hud-native\
```

Isso inclui:

- Posição e tamanho de cada painel.
- Matiz de destaque e alvo do destaque.
- Opacidade e tom do vidro.
- Visibilidade de cada painel.

## Limitações conhecidas

- O click-through só funciona corretamente fora do modo de edição.
- Painéis podem ser posicionados fora da área visível se o monitor for trocado ou a resolução alterada; nesse caso, use o modo de edição para recolocá-los.
- Atalhos globais podem conflitar com outros aplicativos que registram os mesmos hotkeys.
