# Clovy browser extension: store identity and local packaging

The Chrome Web Store release pipeline (RC submission, stable promotion, keyless
GitHub OIDC publishing) belonged to the original organization's desktop
release flow and was removed by the `desvincular-repo-ci` change. There is no
automated store publication in this repository. What remains is the store
identity the code is pinned to and the local build and checks.

`Clovy-extension.zip` is the canonical artifact name. The existing store item
identity remains unchanged for installed-extension continuity under
[ADR-0055](adr/0055-clovy-technical-identity-migrates-through-a-compatibility-bridge.md).

## Store identity

- The store item ID is `jfpogffllplkfoooiaibjkojkngbdnik`. Its public key is
  mirrored into `extension/public/manifest.json` `key` and the ID into
  `EXTENSION_ID` in `src-tauri/src/extension_host.rs`, so load-unpacked builds
  share the store item's ID. If the item is ever recreated, adopt the new key
  and ID the same way.
- The Chrome Web Store rejects any uploaded manifest that contains a `key`
  field ("key field is not allowed in manifest"); strip it from
  `extension/dist/manifest.json` before zipping a package for the store.
- The app hides every Browser use surface behind the `BROWSER_USE_ENABLED`
  feature flag (`src/lib/feature-flags.ts` and
  `src-tauri/src/feature_flags.rs`) until a published store item exists.

## Local package verification

```sh
pnpm --filter clovy-extension typecheck
pnpm --filter clovy-extension test
pnpm extension:build
```

For interactive behavior, load `extension/dist` unpacked and follow the Browser
use QA instructions.
