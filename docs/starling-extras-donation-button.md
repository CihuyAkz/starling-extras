# Starling Extras 0.9.0 — Donation Button Update

The 0.8.0-1 source contains a **Donate** button on the main title screen. It is positioned at the lower-left edge, opens a confirmation screen, and only launches the external browser after the player selects **Open in Browser**. The original destination in the 0.8.0-1 source is `https://sociabuzz.com/cihuyakz/tribe`.

The 0.9.0 feature-synced source did not contain that title-screen button or its confirmation flow. This update restores the feature in the 0.9.0 fork.

## Changes

- Restored the **Donate** button at the 0.8.0-1 position: x = 4 px, y = screen height − 34 px, width = 100 px.
- Added a dedicated `donation_confirm` screen mode with narration support.
- Added **Open in Browser** and **Cancel** actions. The link is opened only after explicit confirmation.
- Added Escape handling to return to the title screen without opening the link.
- Kept button narration accurate; the Donate button is not announced as the Mods control.
- Added regression tests for button placement, confirmation, Cancel, Escape, focus state, and the exact Sociabuzz URI.

## Validation

Static source checks passed for the new mode, stable mode ID, button geometry, confirmation UI, click routing, Escape handling, runtime-agent screen mapping, and the exact URL. The source archive integrity is checked separately after packaging.

The Rust toolchain (`cargo`, `rustc`, and `rustfmt`) is not installed in the editing environment. The regression tests have been added but were not executed here, and no APK has been built. Run the Starling Extras Android workflow on the fork to compile and package the updated application.
