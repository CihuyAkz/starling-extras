# Additional Android native mod bundles

The following supplied HNMOD source bundles are included in the Starling Extras Android native-mod catalogue:

| Mod | Version | Category | Horizon source pin |
| --- | --- | --- | --- |
| Cinematic Interactions | 0.12.2 | Client-Side | `fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca` |
| First-person Model | 1.0.0 | Client-Side | `2ebf00a0281c592f79191e76fce0e7d522c6f425` |
| Eating Animation | 0.2.0 | Client-Side | `6d99e14ff0c70ae60bc56b67829b526409fa068e` |
| Veinminer | 0.3.2 | Server-Side | `d91cab68ded29bd2b004f7050c468fb98c41a00b` |
| LiteMiner | 1.0.0 | Server-Side | `f319701e8c9ab657ff562250b3ba07b2d654e3bc` |
| AppleSkin | 0.1.3 | Client-Side | `02c8607ee8ebd216f2f535c1eb7b62b572830df2` |
| RightClickHarvest | 0.1.1 | Server-Side | `4aa6854a886183440256f4f4a4a9d1ed853227a1` |
| Beautiful Day Counter (CustomHUD) | 1.7.0 | Client-Side | `c9d474dd9116e84c243f8992d75c11314df56f06` |
| Carry Block | 0.1.2 | Server-Side | `142388e1c136e622652ec7505af15d8ad195e041` |

Each source archive is unpacked under `native-mods/<id>/`, with its `horizon-native.json`, mixin manifest, Rust helpers, and supplied assets retained. The Android asset bundler packages these folders as `.hnmod` archives. The nine IDs are added to the Android build manifests and `native-mods/prebuilt.txt`. World Map and Minimap are intentionally excluded from the Android bundle.

At native-build time, `packaging/android/native_build.sh` copies the selected bundles, pins the copied manifests to the Starling source revision, and runs `native_mod_adapt.py` before applying the mixins. This is the project's intended route for older Horizon HNMOD bundles; they are not copied into the game as runtime-loaded Rust source. The packages are made available to the Android native-mod build flow, not automatically compiled into the default APK library.

The supplied archives have different Horizon source pins. Those original pins are retained in each bundle's metadata for provenance; the native build flow retargets working copies to the Starling revision. The build requires the matching Rust, Cargo, and Android toolchains. LiteMiner is listed as Server-Side because its expanded block breaking is handled by the game that owns the world; its hotkey, HUD, shape selection, highlights and configuration still use client hooks.

## License notes

- First-person Model includes its MIT license and attribution notice.
- Eating Animation includes its MIT license.
- Cinematic Interactions includes a notice that identifies the port as MIT and attributes the adapted hammer model under CC BY 4.0, but its notice refers to a `LICENSE` file that is absent from the supplied archive.
- Veinminer contains no license file or license declaration in the supplied archive. Keep it local and obtain clarification before redistributing it publicly.
- LiteMiner includes the MIT license for the upstream Liteminer project and a separate MIT license and attribution notice for its Horizon port.
- AppleSkin 0.1.3 and RightClickHarvest 0.1.1 contain no license file or license declaration in their supplied HNMOD archives. Keep them local and obtain licensing clarification before redistributing them publicly. RightHarvest is listed as Server-Side because its crop interaction runs in `Game::use_hand` on the game that owns the world; AppleSkin is Client-Side because it draws HUD overlays and client settings.
- Beautiful Day Counter (CustomHUD) 1.7.0 and Carry Block 0.1.2 contain no license file or license declaration in their supplied HNMOD archives. Keep them local and obtain licensing clarification before redistributing them publicly. CustomHUD is Client-Side and explicitly gated to singleplayer; Carry Block is Server-Side because pickup and placement act on the game that owns the world.

## Verification status

The archive structure, IDs, versions, source pins, manifest/helper references, Android bundle packaging, and generic microphone-permission detection are checked separately. Veinminer was also present in the successful Dev arm64 native build on 2026-11-12 (GitHub Actions run `43212568931`); this confirms it compiled in that build set. Installation and in-game behavior are not certified by source-only checks.
