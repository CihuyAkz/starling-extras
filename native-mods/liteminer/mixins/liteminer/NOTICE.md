# Credits

LiteMiner for Horizon is a port of **Liteminer** by **Kaf** (iamkaf),
https://github.com/iamkaf/liteminer, MIT License, Copyright (c) 2024 Kaf. The port was made
from the Liteminer source at commit `247a550e31a289348872023700cf5baa472f454c`.

Taken from Liteminer, rewritten in Rust for Horizon's own break and use paths:

- the five shapes and their walkers (`ShapelessWalker`, `TunnelWalker`, `StaircaseUpWalker`,
  `StaircaseDownWalker`, `ThreeByThreeWalker`) and `withoutInstantBreakExpansion`;
- `BlockFamily` (ore families, deepslate variants, flowers and tall plants, grown crops);
- `VeinmineChecks`, `TagHelper` (the `liteminer:` and `ftbultimine:` tags), `FoodExhaustion`;
- `OnBlockBreak` and `OnBlockInteraction`; the break speed modifier
  (`getScaledBreakSpeedModifier`);
- the HUD text, the scroll to change shape, the highlight's merged outside edges and its two
  passes, the commands, and both configuration files with their defaults, ranges, names
  and tooltips.

Not carried over: Liteminer's handshake and doctor (Horizon's mod channel stands in for its
packets), its public API events, and Farmer's Delight's Nourishment check (no such effect
in Horizon).

Port by bartekchomik26-dotcom, MIT (see `LICENSE`).
