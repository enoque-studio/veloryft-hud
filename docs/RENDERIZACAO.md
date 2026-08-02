# Arquitetura de renderização

## Objetivo

O cluster de condução — RPM, marcha, velocidade, freio e acelerador — é o
caminho visual crítico. Ele deve receber a primeira oportunidade de apresentação
em cada ciclo, mesmo quando o jogo pressiona o compositor do Windows. Pneus,
força G e elevação são informativos e podem atualizar em cadências menores sem
prejudicar a pilotagem.

## Por que não WebView2

O HUD já rasteriza com Direct2D nativo. Colocar apenas a barra de aceleração em
um WebView2 adicionaria Chromium, uma nova swap chain e outra superfície para o
DWM compor. Isso aumenta memória e contenção de GPU e não remove o gargalo final:
com 100% de uso, toda superfície ainda depende de tempo de composição do DWM.

## Pipeline

1. A thread UDP publica somente o snapshot mais recente por `ArcSwap`.
2. `DisplaySmoother` interpola os controles usando tempo real, sem depender da
   quantidade de pacotes recebidos.
3. O cluster é processado primeiro e não sofre limitação de cadência.
4. Cada painel possui sua própria chave visual. Uma animação no cluster não
   invalida mais os painéis auxiliares.
5. O tempo síncrono de `UpdateLayeredWindow` alimenta uma média móvel do custo
   de apresentação.
6. Sob pressão, somente os painéis auxiliares degradam progressivamente:

| Estado | Cluster | Pneus / força G | Elevação |
|---|---:|---:|---:|
| Normal | até 60 fps | até 30 fps | até 10 fps |
| Elevado | até 60 fps | até 20 fps | até 4 fps |
| Severo | até 60 fps | até 10 fps | até 2 fps |

A histerese impede alternância rápida entre os estados. Como frames adiados não
atualizam sua chave visual, o painel apresenta o estado mais recente quando seu
próximo orçamento fica disponível; não existe fila de frames antigos.

## Limite físico

O governador remove trabalho concorrente e protege o cluster, mas não cria tempo
de GPU. Se o jogo mantiver a GPU permanentemente em 100%, o DWM ainda pode
atrasar `UpdateLayeredWindow`. Para garantia operacional, o jogo deve ter limite
de FPS que preserve aproximadamente 5–10% de margem para o compositor.

## Evolução

Se medições em máquinas reais ainda mostrarem bloqueio relevante, a próxima fase
é substituir DIB + `UpdateLayeredWindow` por swap chain de composição e
DirectComposition, mantendo click-through via hit-test. Essa migração deve ser
guiada por métricas de latência de apresentação, não pela introdução de WebView2.

## Barra crítica isolada (0.1.0.2)

A barra de acelerador foi removida do bitmap de 680 × 240 do cluster e passou
para uma janela layered própria, transparente e alinhada sobre o card. Essa
janela é controlada por uma thread exclusiva, lê diretamente o snapshot mais
recente e mantém apenas o estado atual — nunca uma fila de frames.

O `ID2D1DCRenderTarget` da barra usa `D2D1_RENDER_TARGET_TYPE_SOFTWARE`. Assim,
o preenchimento da barra não compete com o jogo na fila de rasterização da GPU;
somente a composição final do pequeno bitmap continua dependendo do DWM. Durante
o modo de edição ela é ocultada e sua posição acompanha a geometria do cluster.
