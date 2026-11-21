# Starling Extras 0.9.0 source port

This tree is based on the supplied Starling 0.9.0 source archive and the supplied Starling Extras 0.8.0-1 source archive.

## Carried forward

- Kept the Starling Extras Android application ID `dev.horizon.android` so the new build can update the existing Extras installation.
- Kept the app label `Starling Extras` and advanced Android `versionCode` from 31 to 32; `versionName` is 0.9.0.
- Updated the Java package namespace, Android manifest component names, resource package and JNI exports to match the retained application ID.
- Preserved the companion Horizon Android package lookup as `dev.horizon.android.dev`.
- Migrated the 21 supplied Extras native-mod source bundles with Starling 0.9.0's `native_mod_adapt.py`. The existing 0.9 `wrench` bundle was retained rather than replaced by the older Extras copy.
- Kept the upstream 0.9 `wrench` prebuilt set and added the same 20-mod Extras build set used by the supplied 0.8 Extras source. `kotost_roulette` source is retained in `native-mods/` but is not part of that prebuilt set, matching the supplied source configuration.

## Build and verification limits

The native-mod sources and manifests were migrated, and Python/TOML/JSON/source-path checks were run on the resulting tree. A full Android APK and native `.so` were not compiled in this environment: the Rust toolchain (`cargo` and `rustc`), cargo-ndk and Android SDK/NDK build dependencies are not installed here. The 0.9 native-mod workflow performs its full source materialization, adapter pass, mixin application and native compile in a properly provisioned checkout.

The 0.8 overlay also changed a number of core files whose module layout and APIs were substantially reorganized in 0.9. Its old patch does not apply cleanly to the 0.9 tree, so those core changes were not force-applied. This port carries the native Extras mod bundles and Android app identity, but it is not a claim that every 0.8 core/UI patch has been independently reimplemented or runtime-tested on 0.9.
