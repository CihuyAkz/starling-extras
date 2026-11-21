# Starling Extras native mod bundle

Starling Extras keeps its own native-mod bundle alongside the client features documented in [Horizon extras](extras.md). The bundle below is the set migrated from the supplied Starling Extras 0.8.0-1 source into the Starling 0.9.0 fork.

| Feature name | Native mod ID |
|---|---|
| Zoom | `zoom` |
| Recipes | `recipes` |
| SmartF3 | `smartf3` |
| Freecam+ | `freecam_plus` |
| Connected Textures | `connected_textures` |
| Armor HUD | `armorhud` |
| Jade | `jade` |
| Basic HUD | `basic_hud` |
| Visuality Horizon | `visuality_horizon` |
| Punchy | `punchy` |
| Freelook | `freelook` |
| Cinematic Interactions | `cinematic_interactions` |
| First Person Model | `firstpersonmodel` |
| Eating Animation | `eatinganimation` |
| VeinMiner | `veinminer` |
| LiteMiner | `liteminer` |
| AppleSkin | `appleskin` |
| RightClickHarvest | `rightharvest` |
| Custom HUD | `customhud` |
| Carry Block | `carryblock` |

## How the bundle is built

`native-mods/prebuilt.txt` defines two separate builds: the inherited `wrench` example and the 20-mod Starling Extras bundle above. The Android release workflow dispatches the native-mod workflow for the same Starling Extras tag. The native-mod workflow builds the selected set from that tag and publishes the library and build metadata to this fork's `native-builds` release.

These mods are **not statically embedded inside the APK file**. The app lists compatible native builds in **Options → Mods**; choosing a build makes the app load that matching game library on the next start. The published native-mod build service currently supports **ARM64 only**. Other APK ABIs can still be produced, but cannot use this prebuilt native bundle at present.

## Keep the list synchronized

Every ID in the default bundle must have a matching `horizon-native.json`, `horizon-mixin.toml`, module source file, and injection helper. Run:

```sh
python3 tools/check_extras_bundle.py
```

The native-mod GitHub Actions workflow runs this check before accepting a build request. It fails if an ID is removed, added, duplicated, renamed, or missing a referenced source/helper file. `wrench` remains a separate build and is not counted as one of the 20 Extras mods.

The separate `native-mods/kotost_roulette` source directory is not part of the default prebuilt set.
