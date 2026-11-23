# Starling Java Bridge

This integration brings Horizon Bridge into Starling as an optional Java-mod runtime and a limited Fabric-to-Starling gameplay adapter. Starling remains the target application and owns the authoritative Rust simulation. Horizon Bridge supplies the JVM/Fabric facade and translates supported calls; it does not replace Starling with a second Minecraft client.

The Android runtime uses Starling's OpenJDK 25 installation and prepared Minecraft client artifacts. The Android JVM loader preloads the runtime libraries with global symbol visibility before resolving `JNI_CreateJavaVM`. It prioritizes `libjli`, the core Java libraries, AWT and font libraries, then best-effort loads the remaining shared objects under the runtime's `lib/` tree. This follows the dynamic-library preparation pattern in Pojav Launcher's [`JREUtils.java`](https://github.com/PojavLauncherTeam/PojavLauncher/blob/v3_openjdk/app_pojavlauncher/src/main/java/net/kdt/pojavlaunch/utils/JREUtils.java); the JNI entry point remains Horizon Bridge's `JNI_CreateJavaVM`, not Pojav's `JLI_Launch` wrapper. When platform sources must be compiled on Android, `horizon-prepare` invokes Starling's packaged `libstarling_javac.so` launcher through the runtime's `libjli.so`; it does not assume the JRE contains `bin/javac`.

## Runtime startup

The `java-mods` feature is enabled by the Android app and the desktop game target. When `mods-java/` contains JARs, Starling starts the JVM and Fabric bootstrap on a dedicated thread during startup. The bridge requires the prepared Minecraft metadata and Java 25 runtime. Fabric API is supplied when the installed mod set does not already contain it. Renderer replacements that conflict with Starling's renderer, and mods that depend on those replacements, are excluded from Fabric's load set and reported in the log. The JVM shutdown path runs after Starling's event loop returns.

On Android, `.jar` files opened or shared to Starling are accepted only when their root `fabric.mod.json` contains a non-empty mod id and version, then copied into `mods-java/` without replacing an existing file. The folder is also exposed in the Starling game folder in Android Files. Restart Starling after adding a JAR; the bridge discovers its mod set during startup. Install only JARs from sources you trust. Forge and NeoForge JARs are not compatible with this Fabric-only startup path.

The Modrinth raw-JAR fallback only installs a downloadable Fabric build matching Starling's pinned Minecraft version. Forge and NeoForge JARs are not compatible with this Fabric-only startup path and are not installed into `mods-java/`.

## Implemented Java GUI and HUD bridge

Horizon Bridge captures Fabric screen, HUD and inventory-overlay output as ordered draw operations. Starling forwards mouse movement, click/drag, wheel, keyboard, character and screen-size changes back to the Java screen callbacks. Captured Java mod menus keep their server-side menu logic in the Java mirror while Starling displays their screen frames and routes input back to that menu. Menu-close and client-only-screen transitions are tracked separately so closing a menu releases input capture without accidentally closing a screen layered over it. The operations are appended to Starling's normal `HudBatch`, so the same native renderer owns clipping, text, texture sampling, item pictures and frame submission.

The current draw-operation adapter handles fills, vertical gradients, text components (including common formatting styles and language keys), GUI atlas sprites and partial-sprite UV regions, resource-pack texture blits, captured dynamic RGBA images, item stacks, item count/damage decorations, item tooltips, text tooltips, nested scissor rectangles and the recorded 2D pose matrix. Java HUD operations are inserted after Starling's base HUD; Java screens and inventory overlays are layered after native notifications and debug overlays. Dynamic Java images are cached by texture name and pixel contents, with bounded image memory. Mod GUI textures are resolved from Starling's resource sources.

The 2D GUI/HUD adapter is separate from the Java entity and GPU paths. Java entity renderer exports are consumed by Starling's world renderer: registered model-part trees, per-part poses, exported mesh geometry (including per-vertex packed light where the Java renderer reports it), held/equipped item models, first-/third-person hand layers, and burning state for Starling's native flame overlay are converted into Starling vertices and item draws. Entity snapshots also feed name labels, shadow placement, crosshair picking, and local attack/use callbacks back into the Java mirror. The Java mirror still owns the entity object, AI, ticking and authoritative behavior; this is not a general Rust implementation of arbitrary Java entity classes, and render features that are not exported by the bridge are unavailable.

The JVM bridge also contains a `renderpearl` GPU-device backend in `horizon-jvm/src/gpu` and `HorizonGpu.java`. It implements the exposed GPU resource, buffer, texture, encoder and render-pass operations through `wgpu`, with the Java-side frontend validating/compiling shader stages through the bundled shaderc/SPIR-V path. This supports mods using that RenderPearl device API (including render-to-texture use cases); it is not a general replacement for Minecraft's complete world renderer, nor does it imply Iris/OptiFine shader-pack compatibility, arbitrary Java render layers, direct OpenGL/Vulkan context ownership, or renderer-replacement compatibility.

## Java content registries

Before Starling freezes its content registries, the bridge starts Fabric and asks the Java-side `HorizonContent` oracle for the actual registered block/item facts, tags and loaded recipes. `horizon-port-pack` uses those facts and the prepared Minecraft templates to carry each Java mod into a generated Starling content package. The generated folders are added to Starling's normal content-mod selection and loaded before the world/resource tables are built.

The bridge also imports the Java creative-tab export. It merges Java tab entries into Starling's prepared creative inventory, preserving Starling-native tabs and items. Java `BlockColors` registrations are exported as biome-source names or constant colors and applied to Starling's terrain tint table before `MeshTables` are created.

This is a data conversion into Starling's native registries, not a live Java registry shared with Rust. The importer supports blocks, items, tags, recipes and the carried assets/data understood by `horizon-port-pack`; custom block behavior is represented by its supported native content definition/template. It does not execute a Java custom block/item class as Rust gameplay code. The server bridge also exports Java mob-effect ids and extra level stems. Starling maps a Java effect id only when it can resolve a corresponding native effect definition, and imports extra-dimension ids, build shape and the serialized dimension-type fields it maps when the Java data registry is available. This does not yet constitute general runtime registry synchronization: arbitrary registry types, unmapped dimension-type properties, preset-specific late changes, biome/structure/feature registries and all world-generation codecs are not imported as a fully native registry graph.

## Java particles and Fabric payload forwarding

Java particle types whose namespaced id has an imported particle sprite set can now be spawned by Starling from the bridge's `horizon:java_particles` payload. Starling interprets the packet's position, spread, speed, count and limiter fields and creates native generic billboard particles using the imported sprite set; animated sprite sets advance with particle age. This is a visual compatibility path, not execution of the Java particle provider, Java particle options, custom tick logic, collision callbacks or arbitrary particle render layers.

Clientbound packets emitted through the Java bridge are decoded at the packet boundary. The `horizon:java_particles` channel is consumed by Starling for the local player, while packet data addressed to a connected LAN guest is forwarded as the original clientbound packet when the Java payload has a registered codec. Unknown LAN-guest serverbound custom payloads are passed into the Java mirror only when Fabric has both a matching payload codec and a registered per-player or global server receiver; Starling's own custom channels are handled first. Horizon's in-JVM `HorizonLoop` continues to run registered Fabric client/server payload handlers within the Java mirror. When a Java recording mod requests packet capture, Starling taps the in-memory local player's outbound-to-client packet stream and passes the raw packet bodies back into the JVM client connection; the pending tap queue is bounded to 64 MiB and drops an oversized batch with a warning. This does not make an arbitrary mod protocol understood by Starling itself, and payloads without matching Fabric codecs/receivers remain unsupported.

## Implemented Starling gameplay bridge

The Java platform receives a per-tick snapshot of the local player and connected LAN guests in the active dimension. The snapshot includes position, rotation, game mode, health, food, saturation, exhaustion, permissions, sneak state, selected hotbar slot, movement input, and changed inventory contents.

The Fabric server mirror receives player join, leave, death, respawn and chat events. A successful player block break is delivered as `PlayerBlockBreakEvents.AFTER`. Before a player break is committed, Starling invokes Fabric's `PlayerBlockBreakEvents.BEFORE`; a `false` return cancels the Starling break and also dispatches Fabric's `CANCELED` event. A successful vanilla block-item placement is forwarded to the Java mirror as the vanilla `GameEvent.BLOCK_PLACE`, so its nearby game-event listeners can react. Those callbacks run while Java can read the active Starling world snapshot.

The read-only world view exposes loaded block states and properties, light, block-entity SNBT, chunk section/column data, chunk revisions, surface height and biome palettes for the active dimension. Game rules, difficulty, game time and seed are synchronized to the Java mirror.

Actions returned from Java and applied by Starling currently include:

- player system messages and action-bar messages, including LAN guests;
- server broadcasts;
- supported Starling game-rule changes;
- Overworld world-spawn changes;
- block-state writes in the active dimension, when the block state exists in Starling's registry;
- food and saturation changes for the local player and LAN guests;
- inventory slot changes for the local player, using the 41-slot vanilla inventory layout;
- dropped vanilla items and experience orbs in the active dimension, preserving their position and motion direction;
- vanilla entity spawns from Java NBT when Starling has a corresponding entity implementation;
- block-entity data writes for block-entity types Starling already implements, decoded from full vanilla SNBT and followed by Starling's block-entity update path;
- same-dimension player teleports for the local player and connected LAN guests, including rotation and Starling's entity-section/position bookkeeping;
- player game-mode and ability changes for the local player and connected LAN guests, including flight flags and finite movement speeds;
- level events in the active dimension, including block-destroy state translation and source-player exclusion;
- Java-originated damage to the local player or a connected LAN guest, with registered damage types and direct-source display names;
- Java-originated damage to Starling mobs, including invulnerability-frame reset and attacker attribution only when the Java source is a mirrored Starling mob;
- selected Java `ServerPlayer.openMenu` calls for LAN guests, routed into Starling's native crafting, stonecutter, grindstone, loom, cartography, smithing and ender-chest menus;
- Java custom entity snapshots rendered in Starling's world scene, including exported model parts, poses, meshes, equipment and hand layers, plus entity names and shadows;
- local Java custom-entity attack and interaction callbacks, routed to the mirrored Java `Player.attack` / `Player.interactOn` path after Starling's crosshair-distance and block-occlusion checks;
- Java custom-menu content for LAN guests when the menu id is registered in Starling's menu registry: slot items and carried stack are shown in a native container window, and slot clicks/close events are forwarded to the Java menu;
- client packet capture for Java recording mods after the JVM requests the tap.

These are concrete integration points, not a claim that arbitrary Fabric mods are compatible. Unsupported actions are reported instead of being silently treated as successful.

## Compatibility boundary

Starling is a Rust game implementation, not the Minecraft Java client/server runtime. Fabric can load a mod JAR only if its dependencies and Minecraft version match, but a loaded mod may still call APIs whose behavior Starling has not implemented. The content porter can make some Java blocks/items visible and usable as native content; it cannot reproduce arbitrary Java methods, mixins or code-defined game logic.

The current bridge remains partial. Java custom entities are owned and ticked by the JVM mirror; Rust consumes exported snapshots and forwards selected interactions, but does not implement each mod's AI, physics, goals, capabilities or entity-side world logic. The renderer supports the model/mesh/item data the Java exporter provides, not arbitrary Java render callbacks, layers, post-processing or special render features. Java mob effects map only when a native Starling effect definition is available, and arbitrary custom block-entity types remain unsupported.

Captured custom Java screens work locally through the GUI callback bridge. For LAN guests, custom Java menus whose type or slot layout Starling cannot reproduce are flattened to a generic chest-style native container, with the player's 36 inventory slots below it; clicks and close events are forwarded to the Java menu. Menus with more than 54 mod-owned slots expose 52 content slots per page, with previous/next controls in the remaining two chest slots, so later slots remain reachable. Guests do not receive the original custom screen styling. The native `ServerPlayer.openMenu` mapping remains limited to the listed workstation and ender-chest types.

The dynamic-registry path imports supported content at startup and exports extra Java dimension stems, build shapes, the modded Java entity-type ids used by command suggestions and `/summon` forwarding, and the serialized dimension-type properties Starling maps into its dimension facts and environment. Mapped fields include the exact numeric fixed-time value (not only its presence flag, and used for sky rendering), coordinate scale, build height, infiniburn, ambient light, monster-spawn light rules, bed/respawn-anchor/piglin/raid rules, dimension effects, timelines and default clocks. It does not synchronize arbitrary registries live, import every dimension-type property, or provide complete custom-dimension world generation. The renderer supports the `renderpearl` GPU-device API backed by `wgpu` and shaderc/SPIR-V, not every Java shader pipeline or shader pack; Iris/OptiFine and mods that require ownership of Minecraft's renderer/GPU thread remain outside the supported path. The bridge mirrors the active dimension rather than every dimension simultaneously. Inventory slot actions are applied to Starling's local player, not arbitrary LAN guests.

Renderer replacement mods such as Sodium/Iris are excluded because Starling owns its renderer. Mods that depend on unsupported registries, gameplay classes or 3D client rendering are not expected to work merely because Fabric accepts their JAR. Worlds that depend on custom Java content the porter cannot represent are not safe to treat as fully supported Starling worlds.


## Requested compatibility targets

| Target | Current Starling status |
|---|---|
| Java custom entities | Partial: JVM mirror owns/ticks mod entities and exports snapshots; Rust handles visual snapshots, crosshair target checks and selected local/remote attack/use routing. Mod entity type ids are added to command registries, and `/summon` for those ids is forwarded to the Java server dispatcher. AI, custom physics, goals and arbitrary entity behavior are not Rust-native. |
| Custom entity renderer | Partial: exported Java model parts/poses, mesh geometry with per-vertex packed lighting, held/equipped items, first-/third-person hand layers and burning-entity flame overlays are rendered by Starling; arbitrary Java render callbacks/layers, leashes and unsupported special features are not. |
| Custom menus | Partial: local Java GUI screen/input bridge works; LAN guests receive a generic chest-style container with item/carried-stack sync and click/close forwarding. Menus over 54 mod-owned slots are paged in 52-slot views; original Java screen styling is not mirrored. |
| Dynamic registries | Partial: startup content import, Java mob-effect and modded entity-type id lookup, `/summon` command forwarding for Java entity types, extra level-stem/build-shape export and mapped `DimensionType` codec fields for gameplay/environment facts, including exact fixed time used by sky rendering, timelines and default clocks. No general live registry sync, complete registry import, or universal Java worldgen registry. |
| Java shader and rendering pipeline | Partial: `renderpearl` GPU-device API is implemented over `wgpu` with shaderc/SPIR-V support, including three-channel RGB textures emulated through RGBA storage; 2D GUI/HUD capture is separate. This is not the full Java world-renderer, Iris/OptiFine shader-pack or arbitrary render-layer support. |

## Build and validation

The Android app enables `horizon-client/java-mods` and sets `HORIZON_BRIDGE_PLATFORM=source`. The latter embeds Java platform sources for compilation on the device through Starling's compiler launcher instead of requiring a desktop JDK or fetching Fabric artifacts during the native build. The desktop game target enables the same feature by default.

The provided build environment does not contain `cargo` or `rustc`, so a Rust compile, Android native build, APK build and device runtime test could not be run here. The source was checked with static manifest, syntax-shape and archive checks; these do not replace a real build. Treat this as an integrated experimental bridge, not verified full Fabric compatibility.
