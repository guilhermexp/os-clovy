# Spec Delta

## Purpose

Oferecer controle de navegador próprio do Clovy, com logins separados persistentes e autoridade temporária isolada por tarefa, preservando segurança e encerramento.

## ADDED Requirements

### Requirement: Navegador próprio e login humano
O Clovy SHALL usar agent-browser integrado por ferramentas próprias, com perfil persistente exclusivo no uso assistido e perfil anônimo descartável em rotinas autorizadas. Não SHALL copiar cookies/perfil pessoal ou auto-conectar ao navegador habitual. Login/senha/OTP exigem takeover humano, durante o qual captura e ações do agente ficam suspensas. Stop encerra controle/processos, não apaga os logins persistidos.

#### Scenario: Login e reabertura
- **WHEN** usuário autentica-se no navegador dedicado, encerra a tarefa e o reabre
- **THEN** login persiste, mas nova tarefa precisa de autoridade própria e não herda abas/refs da anterior
- Test: native Mac/Windows — login sintético e isolamento; box — perfil/lease/lifecycle reais

### Requirement: Transporte privado e identidade estrita
Controle SHALL passar pelo broker Clovy com dono conversa/run, origem, aba/documento/nó e referências qualificadas. Inspeção e ação MUST revalidar identidade exata; aba/nó fechado não pode redirecionar ação a similar. Canal do motor MUST ser privado/autenticado, sem streaming/CDP/comandos públicos desprotegidos. Não SHALL publicar MCP upstream, eval, cookies/auth vault ou sessão escolhida pelo modelo.

#### Scenario: Nó substituído depois da aprovação
- **WHEN** um formulário troca o nó por outro com mesmo nome/role
- **THEN** ação é recusada sem clicar/preencher o substituto
- Test: browser real — fixture local, duas abas e troca de documento/nó

### Requirement: Rede e encerramento supervisionados
O app SHALL preservar políticas de rede e bloqueio privado/DNS pinning aplicáveis às rotinas e grants assistidos, sem substituir proteção por allowlist genérica. Stop/revogação/shutdown MUST encerrar daemon e descendentes desde startup; resultado perdido após mutação MUST permanecer incerto sem replay. Perfil ocupado por outra tarefa MUST ser recusado explicitamente.

#### Scenario: Stop durante startup e entrega incerta
- **WHEN** startup é cancelado ou a resposta se perde depois do dispatch
- **THEN** processos não sobrevivem autorizados e a mutação não é reenviada
- Test: integration — supervisor real, processo filho e fixture de conexão interrompida

### Requirement: Integração completa e modo efetivo
A capacidade autorizada SHALL executar transporte real em produção, com navegador/motor pinados e empacotados para macOS/Windows, erros acionáveis e fluxo nos dois CLIs. Segundo plano é padrão; modo visível é permitido quando background não suporta a ação e antes de despacho incerto. Dependência exclusiva de extensão/pareamento e backend antigo MUST ser retirados após cutover, preservando utilidades compartilhadas.

#### Scenario: Usar sem terminal manual
- **WHEN** usuário habilita o recurso e pede navegação em Claude Code ou Codex
- **THEN** navegador dedicado inicia e ferramentas funcionam sem instalar/rodar CLI global ou anexar Chrome pessoal
- Test: box — smoke com motor real; native — pacote/macOS/Windows e ambos CLIs
