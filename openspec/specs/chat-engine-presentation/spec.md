# chat-engine-presentation Specification

## Purpose
Define o que a interface mostra sobre o motor do chat e como runtimes e provedores são identificados visualmente.

## Requirements

### Requirement: Ícone de marca dos runtimes e provedores
Todo lugar da interface que identifica um runtime de agente ou provedor (Configurações de provedores, servidor MCP, fontes de agentes de código, faixa de agentes de código, importação de projetos do Claude Code, cartões que citam o runtime, seletores de modelo) SHALL mostrar o ícone de marca dele em SVG, junto do nome. Marcas sem SVG de licença aberta MUST cair num monograma. Os SVGs MUST vir de fonte com licença registrada.

#### Scenario: Seletor de motor
- **WHEN** o usuário abre o seletor de motor nas configurações de provedores
- **THEN** Claude Code, Codex e os demais runtimes aparecem cada um com o ícone da sua marca

#### Scenario: Marca sem SVG
- **WHEN** um runtime não tem SVG disponível
- **THEN** aparece o monograma de duas letras, sem quebrar o alinhamento

Test: unit — componente de logo resolve cada id de CLI (`claude`, `codex`, `copilot`, `cursor-agent`, `agy`, `pi`) e provedor para o SVG ou monograma esperado.
