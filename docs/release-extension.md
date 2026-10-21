# Clovy browser extension: identity and local packaging

There is no store publication in this repository: the extension is loaded
unpacked from `extension/dist`. Its identity follows
[ADR-0068](adr/0068-clovy-takes-its-own-identity-with-a-one-time-migration.md).

`Clovy-extension.zip` is the canonical artifact name.

## Identity

- The extension ID is `cmfpkfaibipkocbjcpgamaednndpopcp`. It derives from the
  `key` in `extension/public/manifest.json`, the public half of the owner's
  own RSA key pair; the private half never enters the repository. The ID is
  pinned in `EXTENSION_ID` (`src-tauri/src/extension_host.rs`) and checked by
  `scripts/validate-extension-id.mjs`. If the key pair is ever replaced, update
  the three together.
- The native messaging host is `dev.guilhermexp.clovy.extension`.
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
