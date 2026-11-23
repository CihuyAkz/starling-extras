# Android signing and release channels

## Create signing keys once

Run this from a checkout with Python, a JDK (`keytool`), GitHub CLI (`gh`), and permission to manage Actions secrets and environments in the repository:

```bash
gh auth login
python3 packaging/android/setup_signing.py --repo CihuyAkz/starling-extras --mode both
```

When no signing key is configured, the script generates persistent Preview and Public Android signing keys, then stores them in the GitHub Environments `preview` and `release`. The generated private keys and passwords are sent directly to GitHub Secrets; they are not written into the repository or uploaded as workflow artifacts. Run it again safely: complete signing keys are detected and left unchanged. It also keeps existing repository-level or legacy signing secrets when it finds them, so setup does not silently rotate a key already in use.

For only one channel, use `--mode preview` or `--mode public`. Do not delete or replace a signing key after an APK from that channel has been distributed. Android requires updates to keep the same application ID and signing certificate.

Existing `STARLING_KEYSTORE_BASE64` release secrets and `STARLING_DEBUG_KEYSTORE_BASE64` Preview secrets are recognized as legacy signing keys. They are kept in use so an already-installed app does not unexpectedly lose its update path. The Android workflow also tries to restore the original `horizon-android-signing-<repository_id>` Actions cache at `.local/android/debug.keystore` before using Environment Secrets. That cache is branch-scoped: run from the original `android` branch to restore its key when the cache still exists. If neither the legacy cache nor a legacy signing secret is available, the new Environment key is used; APKs signed with a different certificate cannot update an older installation signed by the missing key.

Local builds reuse `.local/android/debug.keystore` when it exists, matching the original Starling Extras signer. Otherwise they create a reusable keystore under `target/android` automatically: Preview uses `target/android/preview.keystore`, and a regular local build uses `target/android/debug.keystore`. These local debug keys are not a substitute for the GitHub Public signing key.

## Choose Preview or Public in Actions

Open **Actions → android → Run workflow** and select `release_mode`:

- `preview`: builds `StarExtras: Preview` (`io.github.cihuyakz.starling.extras.dev`).
- `public`: builds `Starling Extras` (`io.github.cihuyakz.starling.extras`).
- `auto`: reads the repository Actions variable `STARLING_RELEASE_MODE`; without that variable, `v*` tags use Public and other runs use Preview.

Set a repository Actions variable named `STARLING_RELEASE_MODE` to `preview` or `public` at **Settings → Secrets and variables → Actions → Variables → New repository variable** to choose the default. The Run workflow input can override it for a single run.

On a branch/manual build, enable `publish_release` to publish the selected channel. Preview uses one `preview` pre-release and replaces its APK assets on each publication. Public uses `v<VERSION_NAME>` and updates the corresponding public release. A pushed `v*` tag publishes automatically using the resolved mode.

## Replacing a Preview APK with the same version

`packaging/android/build_apk.py` currently sets `VERSION_NAME = "0.9.0"` and `VERSION_CODE = 14`. Preview keeps its application ID and signing key across runs. Publishing another Preview build with version `0.9.0` replaces the `starling-preview-0.9.0-*.apk` release assets. Android accepts an update when the application ID and signing certificate match and the version code is equal to or higher than the installed build.

Public and Preview use different application IDs, so they remain independently installable. New Environment signing keys are separate by default; when the original Extras signing cache or a pre-existing repository-level signing key is reused for upgrade compatibility, both channels may share a certificate without sharing app data or package identity.
