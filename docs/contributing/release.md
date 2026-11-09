# Release

Releases are cut on demand from `main`: run the **Release** workflow from the Actions tab. It has no checks of its own, since `main` only holds commits already checked on `develop`; run from any other branch, it fails. If [semantic-release](https://semantic-release.gitbook.io/) finds a `feat:` or `fix:` commit since the last tag, it tags the next version and creates a draft GitHub release with its notes. Nothing is committed back to `main`: the build stamps the version from the tag, and the notes on each [GitHub release](https://github.com/z3k-md/zephyr/releases) are the changelog. Windows and macOS (Apple silicon only) installers are then built, signed for the updater, and uploaded. A final job writes `latest.json` and publishes the release, and installed copies pick it up from there. Last, the [docs site](/contributing/docs#preview-and-publish) deploys from the tag. With no `feat:` or `fix:` commit to release, the run ends with a warning and changes nothing.

## Repository secrets

| Secret                                                                      | Purpose                                                                                                                                                                                                                                                                               |
| --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `TAURI_PRIVATE_KEY`, `TAURI_KEY_PASSWORD`                                   | Updater signing key. It must match the `pubkey` in [`src-tauri/tauri.conf.json`](https://github.com/z3k-md/zephyr/blob/develop/src-tauri/tauri.conf.json), or installed copies will reject updates.                                                                                   |
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` | Optional. A self-signed certificate for the macOS app, so it keeps the same identity and its permissions across updates. Without them it is ad-hoc signed and asks for permissions again after every update. Either way it isn't notarized, so first launch needs right-click > Open. |

For a local signed build, copy [`.env.example`](https://github.com/z3k-md/zephyr/blob/develop/.env.example) to `.env`, fill in the signing key, and run `bun run build`.

## Windows code signing

Windows builds aren't Authenticode-signed yet, hence the SmartScreen warning; the updater signature above is separate and doesn't affect it. Signing needs a code signing certificate, and then one of:

- A certificate in the build machine's store: set `bundle.windows.certificateThumbprint` (with `digestAlgorithm` and `timestampUrl`) in [`src-tauri/tauri.conf.json`](https://github.com/z3k-md/zephyr/blob/develop/src-tauri/tauri.conf.json), and import the `.pfx` from a secret in the Windows build job before `tauri-action` runs.
- [Azure Trusted Signing](https://learn.microsoft.com/azure/trusted-signing/), which needs no certificate file: set `bundle.windows.signCommand` to a `trusted-signing-cli` command and give the job the Azure credentials as secrets.

Neither is configured, since either one fails the build without its certificate or credentials. A new certificate still builds SmartScreen reputation over time; an EV certificate or Trusted Signing skips most of that.
