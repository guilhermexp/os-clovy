# home-conversation Specification

## Purpose
Define o que o Início (a conversa persistente da tela inicial do Clovy) faz com uma mensagem do usuário: qual motor responde, onde a conversa fica guardada, como ela continua entre mensagens e reinícios, e como falhas aparecem.

## Requirements

### Requirement: Dados das conversas antigas do Início são preservados
O aplicativo SHALL preservar os registros existentes das conversas, mensagens e artefatos do Início após a retirada da interface de chat, mantendo os limites de acesso existentes.

#### Scenario: Atualização com conversas existentes
- **WHEN** um perfil com conversas do Início é aberto após atualizar o Clovy
- **THEN** o app inicia em Hoje e as conversas e seus registros persistidos continuam no armazenamento do perfil
