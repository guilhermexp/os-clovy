# Spec Delta

## ADDED Requirements

### Requirement: Tela bloqueada no Windows
Quadros cujo app é a tela de bloqueio ou de login do Windows (`LockApp`, `LogonUI`) SHALL ser tratados como os de tela bloqueada do macOS: nunca úteis, mesmo com entrada do usuário.

#### Scenario: Windows bloqueado com o Clovy aberto
- **WHEN** o usuário bloqueia o Windows por 20 minutos com a captura ligada
- **THEN** o intervalo não conta como tempo focado
- Test: unit — construtor com quadros do app `LockApp`
