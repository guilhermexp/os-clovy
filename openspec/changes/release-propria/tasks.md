# Tasks

## 1. Release não migra (D9)

- [ ] 1.1 RED: teste `the_release_build_never_migrates` falha com a chamada atual; commit RED.
- [ ] 1.2 GREEN: chamada de `identity_migration` sob `cfg(all(target_os = "macos", debug_assertions))` e origem de produção removida sem código morto; `cargo test identity_migration` e `node scripts/check-org-references.mjs` passam.

## 2. Chave e configuração do updater (D4, D5)

- [ ] 2.1 Gerar o par com `pnpm tauri signer generate` e senha aleatória; privada e senha em `~/.config/clovy/updater.key{,.password}` com modo 0600; conferir com `stat` e que nada entrou no repositório (`git status`).
- [ ] 2.2 Gravar `TAURI_SIGNING_PRIVATE_KEY` e `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` com `gh secret set -R guilhermexp/os-clovy` lendo do arquivo; conferir com `gh secret list` (só nomes).
- [ ] 2.3 `plugins.updater` em `tauri.conf.json` (chave pública e endpoint `releases/latest/download/latest.json`), `src-tauri/tauri.release.conf.json` com `createUpdaterArtifacts: true`, dependências `tauri-plugin-updater` e `tauri-plugin-process` (Cargo e package.json), capability e registro no `lib.rs`; `pnpm tauri:build` local sem a chave privada continua passando (`make verify`).

## 3. Atualização no app (D2, D3)

- [ ] 3.1 RED: restaurar de `f7f068d6^` os testes `update-decision.test.ts`, `updater.test.ts` e os de `shutdown.rs`, ajustados para canal único e build dev sem verificação (`updates_are_disabled_in_dev_builds`); commit RED.
- [ ] 3.2 GREEN: restaurar `updates.rs`, `src/lib/updater.ts`, `update-decision.ts`, `use-app-update-actions*.ts`, `update-ui.tsx`, o adiamento do reinício em `shutdown.rs` e a ação em Ajustes → Geral, sem `ReleaseChannel`, `release-settings.json` nem endpoint rc; testes de 3.1 passam.
- [ ] 3.3 Interface do aviso e de Ajustes → Geral revisada com `/impeccable` (pt-BR e inglês nas mensagens i18n); capturas no app de prova em `.tmp/verify/release-propria/`.
- [ ] 3.4 ADR nova do updater próprio e adendo datado na ADR-0001; `docs/index.md` atualizado.

## 4. Build do Mac (D6)

- [ ] 4.1 RED: teste do script de release do Mac decidindo entre ad hoc, identidade e notarização pelas variáveis presentes (modo de checagem, sem `codesign`); commit RED.
- [ ] 4.2 GREEN: `scripts/build-macos-release.sh` (a partir de `build-signed-dmg.sh`, que sai junto com `tauri:build:signed-dmg`), com `CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_STRIP=none` e `tauri.release.conf.json` quando houver chave do updater; teste de 4.1 passa.
- [ ] 4.3 Build local ad hoc com a chave do updater: gera `Clovy_universal.dmg`, `Clovy.app.tar.gz` e `.sig`; `verify-macos-app-name.mjs`, `lipo -archs` (arm64 e x86_64) e `codesign --verify --deep --strict` passam; saídas em `.tmp/verify/release-propria/mac-local/`.
- [ ] 4.4 `docs/release-macos.md`: receita nova, variáveis opcionais da Apple, quarentena no primeiro download e permissões pedidas de novo a cada atualização com assinatura ad hoc.

## 5. Build do Windows (D7)

- [ ] 5.1 RED: teste do verificador exigindo o `.sig` quando a chave do updater existe; commit RED.
- [ ] 5.2 GREEN: `build-windows.ps1` passa `tauri.release.conf.json` e copia o `.sig` quando há chave; `verify-windows-artifacts.ps1` exige o `.sig` nesse caso e continua exigindo `NotSigned` no Authenticode; `docs/release-windows.md` atualizado.

## 6. Workflow e manifesto (D8, D10)

- [ ] 6.1 RED: `src/test/release-version-check.test.mjs` (tag igual, diferente, release existente) e teste do gerador do `latest.json` (três plataformas, URLs do release, conteúdo dos `.sig`); commit RED.
- [ ] 6.2 GREEN: `scripts/check-release-version.mjs` e `scripts/release-manifest.mjs`; testes de 6.1 passam.
- [ ] 6.3 `.github/workflows/release.yml` conforme D8 (ações por SHA, permissões mínimas, `concurrency`, `dry_run` padrão `true`); YAML válido e `actionlint` sem erro.
- [ ] 6.4 Ensaio: `gh workflow run release.yml -R guilhermexp/os-clovy --ref feat/release-propria -f tag=v0.0.40 -f dry_run=true`; os três jobs passam, os seis arquivos saem como artefatos do run e cada `.sig` confere com a chave pública; link do run e conferência em `.tmp/verify/release-propria/dry-run.md`.

## 7. Limpeza (D11)

- [ ] 7.1 `stable-release-provenance.mjs` e seus testes ajustados aos nomes do Clovy ou apagados se nada mais os usa; receitas com `createUpdaterArtifacts:false` e o texto "no updater signing key" corrigidos em `AGENTS.md`, `docs/development.md` e `docs/release-macos.md`; `make verify` com exit 0.

## 8. Prova

- [ ] 8.1 `make verify` com exit 0 e saída em `.tmp/verify/release-propria/make-verify.txt`.
- [ ] 8.2 Windows na máquina de prova (`ssh cert`, só no perfil `cert-`, sem credencial do GitHub): instalar o `setup.exe` do ensaio, abrir, conferir versão e que nenhuma migração rodou; desinstalar e apagar o perfil criado; registro em `.tmp/verify/release-propria/windows.md`.

## Workflow follow-up

- Depois do merge da `identidade-clovy` na `main`: rebase desta branch, revisão, aceite e merge.
- Na `main`: `pnpm version:bump 0.1.0`, commit `release: v0.1.0`, tag `v0.1.0` → release real; conferir `releases/latest/download/latest.json` sem autenticação.
- Prova de atualização: bump `0.1.1` e tag; no Windows de prova o `0.1.0` instalado oferece e instala o `0.1.1`; no Mac, o dono instala o `.dmg` `0.1.0` (Abrir assim mesmo) e aceita a atualização.
- Arquivar (`openspec archive release-propria -y`, depois da `identidade-clovy`), `/sync`, `/cleanup`.
