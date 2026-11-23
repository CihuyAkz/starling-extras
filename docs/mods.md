# Horizon mods

The general mod documentation lives in the modding book: [introduction](modding/src/introduction.md), [installing and using mods](modding/src/start/installing.md), [making a content mod](modding/src/how-to/content-mods.md) and [publishing](modding/src/how-to/publish.md).

This Starling fork also builds an experimental Java/Fabric bridge into the Android app and desktop game target. Put compatible Fabric JARs in `mods-java/` under the game directory. The bridge uses the Java 25 runtime, imports supported Java block/item content into Starling's native content packages, and adapts a limited set of Fabric gameplay events and screen/HUD drawing operations. It does not make arbitrary Java mods fully compatible: see [Starling Java Bridge](starling-java-bridge.md) for the supported surface and known limits.
