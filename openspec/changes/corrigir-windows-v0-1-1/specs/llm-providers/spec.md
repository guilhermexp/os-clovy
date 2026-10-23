## MODIFIED Requirements

### Requirement: Execução isolada de CLIs em geração pontual
Chamadas de geração pontual a CLIs SHALL rodar sem persistir sessão no CLI, sem carregar arquivos de contexto/skills, numa pasta de trabalho temporária vazia, com tempo limite, e MUST marcar o prompt com o marcador de autoria do Clovy para que a ingestão de sessões de agentes as ignore. Cada CLI MUST rodar no modo de menor privilégio que oferece: `claude` e `pi` sem ferramentas; `codex` em sandbox só leitura, sem rede e sem pedidos de aprovação; `agy` no modo mais restrito que o CLI aceita. `codex` e `agy` SHALL ser aceitos para notas, limpeza de ditado e atividade (decisão do dono); `cursor-agent` e `copilot` continuam recusados nesses usos, sem redirecionamento silencioso, e disponíveis como motor do chat. Chamadas em segundo plano dos recursos de atividade MUST ser serializadas (uma por vez).

#### Scenario: Codex em notas
- **WHEN** o usuário escolhe `codex` para notas e gera um resumo
- **THEN** o Clovy roda o `codex` em sandbox só leitura, sem rede, numa pasta temporária vazia, sem sessão persistida, e usa a resposta
- Test: unit — montagem da invocação do `codex` para geração pontual (flags de sandbox, aprovação, rede, pasta e efemeridade) e seleção por uso aceita

#### Scenario: Antigravity em atividade
- **WHEN** o usuário escolhe `agy` para atividade
- **THEN** a escolha é aceita e a invocação usa o modo mais restrito do `agy`
- Test: unit — seleção por uso e montagem da invocação do `agy`

#### Scenario: CLI ainda recusado
- **WHEN** o usuário tenta escolher `cursor-agent` para notas
- **THEN** a escolha é recusada com a explicação de que as ferramentas desse CLI não podem ser desligadas, e nenhum processo é iniciado
- Test: unit — seleção por uso e invocação recusadas com CLI falso instalado

#### Scenario: Marcador de autoria presente
- **WHEN** o Clovy executa qualquer chamada de geração pontual por CLI
- **THEN** o prompt enviado contém o marcador de autoria do Clovy
- Test: unit — montagem do comando/prompt por provedor

#### Scenario: Tempo limite
- **WHEN** um CLI não responde dentro do tempo limite configurado
- **THEN** o processo e todos os seus filhos são encerrados e a chamada falha com erro de tempo esgotado, sem processo órfão, também no Windows
- Test: integration — CLI falso que dorme além do limite; no Windows, CLI falso `.cmd` que dispara um filho

## ADDED Requirements

### Requirement: Processos de CLI sem console no Windows
No Windows, todo processo filho de linha de comando que o Clovy inicia (sondagem de versão, `gh`, chat, geração pontual, sidecar do agente, servidores MCP, ferramentas de shell e limpeza de processos antigos) MUST rodar sem abrir janela de console. A sondagem de CLIs instalados SHALL ser compartilhada entre as telas que a pedem ao mesmo tempo, em vez de repetida por cada uma.

#### Scenario: Abrir o app no Windows
- **WHEN** o app de release abre no Windows com `codex`, `claude` e `gh` instalados
- **THEN** nenhuma janela de terminal aparece na tela
- Test: unit — todo ponto de criação de processo passa pelo ajuste de criação sem console (teste que falha se um `Command` novo não passar por ele); manual — abertura do app de release no Windows de prova

#### Scenario: Sondagem simultânea
- **WHEN** três telas pedem o catálogo de CLIs ao mesmo tempo
- **THEN** cada CLI é sondado uma vez só
- Test: unit — contador de sondagens com chamadas concorrentes

### Requirement: Encerramento da árvore de processos no Windows
No Windows, cancelar uma execução de chat por CLI ou estourar o tempo limite MUST encerrar o processo iniciado e todos os seus descendentes (por exemplo `cmd.exe` → `node` → `codex.exe`).

#### Scenario: Cancelar Codex instalado pelo npm
- **WHEN** o usuário cancela uma execução com motor `codex` resolvido como `codex.cmd`
- **THEN** nenhum `node` nem `codex.exe` daquela execução continua rodando
- Test: integration (Windows) — CLI falso `.cmd` que inicia um filho de longa duração; verificação de que o filho terminou
