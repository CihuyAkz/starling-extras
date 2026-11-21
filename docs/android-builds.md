# Starling Extras Android builds

Starling Extras is an unofficial fork of Starling. The Android workflows in this repository build from this fork's source and publish to this fork's GitHub repository; they do not publish builds to the official Starling project.

## Preview builds

Pushes to `main`, `android`, `starling` or `integration`, and pull requests, build APKs for `arm64-v8a`, `armeabi-v7a`, `x86` and `x86_64`. Download the `starling-extras-apk-<abi>` artifact from this repository's Actions run. Preview builds use the separate `StarlingPreview` app identity and are signed with a debug key (or the optional Preview debug keystore).

## Release builds

Create and push a tag such as `v0.9.0` in `CihuyAkz/starling-extras`. The tag must match `VERSION_NAME` in `packaging/android/build_apk.py`. The workflow builds and signs all four ABIs, validates the APK set, and publishes the release to this repository. It then dispatches `native-mods.yml` on the exact same tag so prebuilt native-mod libraries are built from the same Starling Extras source version.

GitHub Actions artifact names use the `starling-extras-` prefix. APK release asset filenames deliberately remain `starling-<version>-<abi>.apk`, because the Android in-app updater parses that established filename format.

## Repository secrets

Create a GitHub Environment named `release` in this fork and add these secrets:

- `STARLING_EXTRAS_KEYSTORE_BASE64` — base64-encoded release keystore.
- `STARLING_EXTRAS_KEY_ALIAS` — release key alias.
- `STARLING_EXTRAS_STORE_PASSWORD` — keystore password.
- `STARLING_EXTRAS_KEY_PASSWORD` — key password.

Optionally add `STARLING_EXTRAS_DEBUG_KEYSTORE_BASE64` as a repository or environment secret for a persistent Preview signing key. Keep all signing credentials private.

A manual `release_key` workflow dispatch builds with the release profile and signing key but does not publish a GitHub release.

## App identity and update compatibility

The Android application ID remains `dev.horizon.android`, and the version code is incremented from the supplied Extras 0.8.0 build to 32 for 0.9.0. Keeping the application ID allows Android to recognize an update, but installation over an existing app still requires the same signing certificate. The fork's updater and native-build browser query `CihuyAkz/starling-extras` only.

The separate remote native-mod compile service configured by `packaging/android/build-server.txt` is not deployed by these GitHub Actions workflows; these workflows publish APK releases and prebuilt native-mod release assets.
