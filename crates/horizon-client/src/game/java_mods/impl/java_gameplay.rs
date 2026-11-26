use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};

use horizon_core::random::RandomSource;

use super::stations::{Kind as StationKind, Station};
use horizon_client::biome::Biomes;
use horizon_client::chunk::MIN_Y;
use horizon_client::world::World;
use horizon_world::registry::{BlockRegistry, BlockStateId};

static JAVA_PLAYER_STATE: LazyLock<Mutex<std::collections::HashMap<String, bool>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));
static JAVA_PENDING_EVENTS: Mutex<Vec<horizon_jvm::events::Event>> = Mutex::new(Vec::new());
static JAVA_INVENTORY_STATE: LazyLock<Mutex<std::collections::HashMap<String, u64>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));
static JAVA_RULE_STATE: Mutex<String> = Mutex::new(String::new());
static JAVA_UNHANDLED_ACTIONS_WARNED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static JAVA_MOB_EFFECT_TYPES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
static JAVA_MENU_WINDOWS: LazyLock<Mutex<std::collections::HashMap<String, (u32, i32, String)>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

pub(in crate::game) fn reset() {
    super::java_gui::reset();
    JAVA_PLAYER_STATE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();
    JAVA_PENDING_EVENTS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();
    JAVA_INVENTORY_STATE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();
    JAVA_RULE_STATE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();
    JAVA_UNHANDLED_ACTIONS_WARNED.store(false, std::sync::atomic::Ordering::Relaxed);
    JAVA_MENU_WINDOWS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();
}

struct StarlingJavaBlocks<'a> {
    world: &'a World,
    registry: &'static BlockRegistry,
    biomes: &'a Biomes,
    dimension: &'a str,
}

impl horizon_jvm::world::Blocks for StarlingJavaBlocks<'_> {
    fn block(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String> {
        if dimension != self.dimension || !self.world.loaded(x, z) {
            return None;
        }
        Some(java_state_string(
            self.registry,
            self.world.block(x, y, z),
        ))
    }

    fn light(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<u8> {
        if dimension != self.dimension || !self.world.loaded(x, z) {
            return None;
        }
        Some(self.world.light(x, y, z))
    }

    fn block_entity(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String> {
        if dimension != self.dimension || !self.world.is_loaded_at(x, y, z) {
            return None;
        }
        let entity = self.world.block_entities.get(&(x, y, z))?;
        let state = self.world.block(x, y, z);
        let name = self.registry.state(state)?.name.clone();
        let mut tag = horizon_client::save::block_entity_encode::encode((x, y, z), entity, &name);
        if let horizon_core::nbt::Tag::Compound(compound) = &mut tag {
            compound.remove(&"keepPacked".into());
        }
        Some(horizon_client::snbt::to_snbt(&tag))
    }

    fn column(
        &self,
        dimension: &str,
        cx: i32,
        cz: i32,
        min_y: i32,
        out: &mut [i32],
    ) -> Option<Vec<String>> {
        if dimension != self.dimension {
            return None;
        }
        let chunk = self.world.chunks.get(&(cx, cz))?;
        thread_local! {
            static PLACES: std::cell::RefCell<Vec<i32>> = const { std::cell::RefCell::new(Vec::new()) };
        }
        let mut ids: Vec<u32> = Vec::new();
        let mut unpacked = [0u32; 4096];
        for (section_index, part) in out.chunks_mut(4096).enumerate() {
            let base = min_y + section_index as i32 * 16;
            let section = (base - MIN_Y) >> 4;
            let aligned = base & 15 == 0 && part.len() == 4096;
            match usize::try_from(section)
                .ok()
                .and_then(|index| chunk.sections.get(index))
            {
                Some(section) if aligned => {
                    section.unpack(&mut unpacked)
                }
                _ => {
                    for (index, value) in unpacked.iter_mut().enumerate().take(part.len()) {
                        let x = (index & 15) as i32;
                        let z = ((index >> 4) & 15) as i32;
                        let y = base + (index >> 8) as i32;
                        *value = chunk.block(x, y, z).0;
                    }
                }
            }
            PLACES.with_borrow_mut(|places| {
                for (slot, &state) in part.iter_mut().zip(unpacked.iter()) {
                    let index = state as usize;
                    if index >= places.len() {
                        places.resize(index + 1, -1);
                    }
                    if places[index] < 0 {
                        places[index] = ids.len() as i32;
                        ids.push(state);
                    }
                    *slot = places[index];
                }
            });
        }
        PLACES.with_borrow_mut(|places| {
            for &id in &ids {
                places[id as usize] = -1;
            }
        });
        Some(
            ids.into_iter()
                .map(|id| java_state_string(self.registry, BlockStateId(id)))
                .collect(),
        )
    }

    fn section_ids(
        &self,
        dimension: &str,
        cx: i32,
        sy: i32,
        cz: i32,
        out: &mut [i32],
    ) -> Option<bool> {
        if dimension != self.dimension {
            return Some(false);
        }
        let Some(chunk) = self.world.chunks.get(&(cx, cz)) else {
            return Some(false);
        };
        let section = sy - (MIN_Y >> 4);
        let mut unpacked = [0u32; 4096];
        match usize::try_from(section)
            .ok()
            .and_then(|index| chunk.sections.get(index))
        {
            Some(section) => section.unpack(&mut unpacked),
            None => unpacked.fill(0),
        }
        for (slot, state) in out.iter_mut().zip(unpacked) {
            *slot = state as i32;
        }
        Some(true)
    }

    fn chunk_revision(&self, dimension: &str, cx: i32, cz: i32) -> i64 {
        if dimension != self.dimension {
            return -2;
        }
        self.world.chunks.get(&(cx, cz)).map_or(-2, |_| {
            self.world.revision.min(i64::MAX as u64) as i64
        })
    }

    fn state_name(&self, id: i32) -> Option<String> {
        let id = u32::try_from(id).ok()?;
        let state = BlockStateId(id);
        self.registry.state(state)?;
        Some(java_state_string(self.registry, state))
    }

    fn surface(&self, dimension: &str, x: i32, z: i32) -> Option<i32> {
        if dimension != self.dimension {
            return None;
        }
        let chunk = self.world.chunks.get(&(x >> 4, z >> 4))?;
        Some(chunk.world_surface(x & 15, z & 15, |state| {
            self.registry.state(BlockStateId(state)).is_none_or(|block| {
                matches!(
                    block.name.as_str(),
                    "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
                )
            })
        }))
    }

    fn column_biomes(
        &self,
        dimension: &str,
        cx: i32,
        cz: i32,
        min_y: i32,
        out: &mut [i32],
    ) -> Option<Vec<String>> {
        if dimension != self.dimension {
            return None;
        }
        let chunk = self.world.chunks.get(&(cx, cz))?;
        let mut palette = Vec::new();
        let mut indexes = std::collections::HashMap::new();
        for (index, slot) in out.iter_mut().enumerate() {
            let qx = (index & 3) as i32;
            let qz = ((index >> 2) & 3) as i32;
            let qy = (index >> 4) as i32;
            let biome = chunk.biome(qx * 4, min_y + qy * 4, qz * 4);
            *slot = *indexes.entry(biome).or_insert_with(|| {
                palette.push(self.biomes.get(biome).name.clone());
                palette.len() as i32 - 1
            });
        }
        Some(palette)
    }
}

fn java_state_string(registry: &BlockRegistry, state: BlockStateId) -> String {
    let Some(block) = registry.state(state) else {
        return "minecraft:air".into();
    };
    if block.properties.is_empty() {
        return block.name.clone();
    }
    let properties = block
        .properties
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{}[{properties}]", block.name)
}

fn java_block_state(registry: &BlockRegistry, descriptor: &str) -> Option<BlockStateId> {
    let (name, properties) = match descriptor.split_once('[') {
        Some((name, properties)) => (name, properties.strip_suffix(']')?),
        None => (descriptor, ""),
    };
    if name.is_empty() {
        return None;
    }
    let name = if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    };
    let properties = if properties.is_empty() {
        horizon_world::chunk::BlockProperties::new()
    } else {
        properties
            .split(',')
            .map(|property| {
                let (key, value) = property.split_once('=')?;
                (!key.is_empty() && !value.is_empty())
                    .then(|| (key.to_owned(), value.to_owned()))
            })
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .collect()
    };
    registry
        .resolve(&horizon_world::chunk::BlockState { name, properties })
        .ok()
}

fn apply_java_abilities(
    game: &mut super::Game<'_>,
    mode_id: i32,
    flags: i8,
    flying_speed: f32,
    walking_speed: f32,
) -> bool {
    if game.player.is_none() || !flying_speed.is_finite() || !walking_speed.is_finite() {
        return false;
    }
    let Some(mode) = horizon_core::game_mode::GameMode::from_id(mode_id) else {
        return false;
    };
    if game.mode != mode {
        game.set_game_mode(mode);
        if game.mode != mode {
            return false;
        }
    }
    let mut abilities = game.acting_abilities.unwrap_or_default();
    if game.acting_abilities.is_none() {
        abilities.update_for_mode(mode.id());
    }
    abilities.invulnerable = flags & 1 != 0;
    abilities.flying = flags & 2 != 0;
    abilities.may_fly = flags & 4 != 0;
    abilities.instabuild = flags & 8 != 0;
    abilities.flying_speed = flying_speed;
    abilities.walking_speed = walking_speed;
    game.acting_abilities = Some(abilities);
    let Some(player) = game.player.as_mut() else {
        return false;
    };
    player.invulnerable = abilities.invulnerable;
    player.flying = abilities.flying;
    player.may_fly = abilities.may_fly;
    player.flying_speed = abilities.flying_speed;
    true
}

fn java_damage_kind(
    catalog: &horizon_world::damage_source::Catalog,
    damage_type: &str,
) -> Option<horizon_client::mobs::DamageKind> {
    use horizon_client::mobs::{DamageKind as K, NamedDamage};
    Some(match damage_type {
        "minecraft:player_attack" => K::PlayerAttack,
        "minecraft:mace_smash" => K::MaceSmash,
        "minecraft:spear" => K::Spear,
        "minecraft:mob_attack" => K::MobAttack,
        "minecraft:mob_attack_no_aggro" => K::MobAttackNoAggro,
        "minecraft:arrow" => K::Arrow,
        "minecraft:thrown" => K::Thrown,
        "minecraft:fireball" => K::Fireball,
        "minecraft:explosion" | "minecraft:player_explosion" => K::Explosion,
        "minecraft:fall" => K::Fall,
        "minecraft:fly_into_wall" => K::FlyIntoWall,
        "minecraft:falling_block" => K::FallingBlock,
        "minecraft:falling_stalactite" => K::FallingStalactite,
        "minecraft:on_fire" => K::OnFire,
        "minecraft:in_fire" => K::InFire,
        "minecraft:campfire" => K::Campfire,
        "minecraft:lava" => K::Lava,
        "minecraft:drown" => K::Drown,
        "minecraft:dry_out" => K::DryOut,
        "minecraft:magic" => K::Magic,
        "minecraft:indirect_magic" => K::IndirectMagic,
        "minecraft:sting" => K::Sting,
        "minecraft:cactus" => K::Cactus,
        "minecraft:sweet_berry_bush" => K::SweetBerryBush,
        "minecraft:generic" => K::Generic,
        "minecraft:in_wall" => K::InWall,
        "minecraft:thorns" => K::Thorns,
        "minecraft:sonic_boom" => K::SonicBoom,
        "minecraft:freeze" => K::Freeze,
        "minecraft:out_of_world" => K::OutOfWorld,
        "minecraft:cramming" => K::Cramming,
        "minecraft:falling_anvil" => K::FallingAnvil,
        "minecraft:fireworks" => K::Fireworks,
        "minecraft:generic_kill" => K::GenericKill,
        "minecraft:lightning_bolt" => K::LightningBolt,
        "minecraft:outside_border" => K::OutsideBorder,
        "minecraft:spit" => K::Spit,
        "minecraft:stalagmite" => K::Stalagmite,
        "minecraft:starve" => K::Starve,
        "minecraft:hot_floor" => K::HotFloor,
        "minecraft:sulfur_cube_hot" => K::SulfurCubeHot,
        "minecraft:wind_charge" => K::WindCharge,
        "minecraft:wither" => K::Wither,
        "minecraft:wither_skull" => K::WitherSkull,
        other => K::Named(NamedDamage::registered(catalog, other)?),
    })
}

pub(super) fn java_uuid_string(level: &horizon_client::save::Level) -> String {
    let uuid = java_uuid(level);
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (uuid >> 96) as u32,
        (uuid >> 80) as u16,
        (uuid >> 64) as u16,
        (uuid >> 48) as u16,
        uuid & 0x0000_ffff_ffff
    )
}

pub(super) fn java_uuid(level: &horizon_client::save::Level) -> u128 {
    level.player_uuid.map_or_else(
        || horizon_protocol::crypto::offline_uuid("Player"),
        |[a, b, c, d]| {
            (u128::from(a as u32) << 96)
                | (u128::from(b as u32) << 64)
                | (u128::from(c as u32) << 32)
                | u128::from(d as u32)
        },
    )
}

fn java_inventory_slots(
    inventory: &[Option<horizon_items::items::ItemStack>],
    armor: &[Option<horizon_items::items::ItemStack>; 4],
    offhand: Option<&horizon_items::items::ItemStack>,
    uuid: &str,
) -> Option<Vec<String>> {
    let slots = (0..horizon_jvm::events::INVENTORY_SLOTS)
        .map(|slot| {
            let stack = match slot {
                0..=35 => inventory.get(slot).and_then(Option::as_ref),
                36..=39 => armor.get(39 - slot).and_then(Option::as_ref),
                40 => offhand,
                _ => None,
            };
            stack.map_or_else(String::new, |stack| {
                horizon_client::snbt::to_snbt(&horizon_items::item_nbt::item_to_nbt(stack))
            })
        })
        .collect::<Vec<_>>();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    slots.hash(&mut hasher);
    let hash = hasher.finish();
    let mut previous = JAVA_INVENTORY_STATE
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if previous.get(uuid).copied() != Some(hash) {
        previous.insert(uuid.to_owned(), hash);
        Some(slots)
    } else {
        None
    }
}

fn java_inventory(game: &super::Game<'_>, uuid: &str) -> Option<Vec<String>> {
    java_inventory_slots(&game.inventory, &game.armor, game.offhand.as_ref(), uuid)
}

fn java_guest_inventory(
    guest: &horizon_client::lan::guest::Guest,
    uuid: &str,
) -> Option<Vec<String>> {
    java_inventory_slots(&guest.inventory, &guest.armor, guest.offhand.as_ref(), uuid)
}

pub(in crate::game) fn queue_chat(level: &horizon_client::save::Level, text: &str) {
    queue_chat_uuid(java_uuid(level), text);
}

pub(in crate::game) fn queue_chat_uuid(uuid: u128, text: &str) {
    if !horizon_jvm::events::enabled() {
        return;
    }
    JAVA_PENDING_EVENTS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push(horizon_jvm::events::Event::Chat {
            uuid: horizon_protocol::crypto::uuid_string(uuid),
            text: text.to_owned(),
        });
}

pub(in crate::game) fn before_player_break(
    game: &mut super::Game<'_>,
    position: [i32; 3],
    state: BlockStateId,
) -> bool {
    if !horizon_jvm::events::enabled() || game.remote.is_some() {
        return true;
    }
    let uuid = game
        .lan_acting_uuid
        .unwrap_or_else(|| java_uuid(&game.level));
    let uuid_string = horizon_protocol::crypto::uuid_string(uuid);
    let dimension = game.dim.id().to_owned();
    let state = java_state_string(game.pack.assets.registry, state);
    let allowed = {
        let blocks = StarlingJavaBlocks {
            world: &game.world,
            registry: game.pack.assets.registry,
            biomes: world_biomes!(game),
            dimension: &dimension,
        };
        horizon_jvm::world::with_blocks(&blocks, || {
            horizon_jvm::events::before_break(&uuid_string, &dimension, position, &state)
        })
    };
    game.apply_java_actions(&uuid_string);
    match allowed {
        Ok(allowed) => allowed,
        Err(error) => {
            log::warn!("Java mods: block-break callback failed: {error}");
            true
        }
    }
}

pub(in crate::game) fn queue_block(
    game: &mut super::Game<'_>,
    position: [i32; 3],
    state: BlockStateId,
) {
    queue_block_event(game, position, state, false);
}

pub(in crate::game) fn queue_block_placed(
    game: &mut super::Game<'_>,
    position: [i32; 3],
    state: BlockStateId,
) {
    queue_block_event(game, position, state, true);
}

fn queue_block_event(
    game: &mut super::Game<'_>,
    position: [i32; 3],
    state: BlockStateId,
    placed: bool,
) {
    if !horizon_jvm::events::enabled() || game.remote.is_some() {
        return;
    }
    let uuid = game
        .lan_acting_uuid
        .unwrap_or_else(|| java_uuid(&game.level));
    let state = java_state_string(game.pack.assets.registry, state);
    JAVA_PENDING_EVENTS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push(horizon_jvm::events::Event::Block {
            uuid: horizon_protocol::crypto::uuid_string(uuid),
            position,
            state,
            placed,
        });
}

fn java_rules_string(game: &super::Game<'_>) -> String {
    let rules = game
        .ui
        .rules
        .available_rules()
        .filter_map(|rule| {
            game.ui
                .rules
                .get(&rule.name)
                .map(|value| format!("{}={value}", rule.name))
        })
        .collect::<Vec<_>>();
    let difficulty = game.ui.difficulty;
    rules
        .into_iter()
        .chain(std::iter::once(format!("difficulty={difficulty}")))
        .collect::<Vec<_>>()
        .join(";")
}

fn java_text(text: &str) -> horizon_client::text::Text {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(component) if component.is_object() || component.is_array() || component.is_string() => {
            horizon_client::packs::component(&component).with_component(component)
        }
        _ => horizon_client::text::Text::literal(text),
    }
}

fn java_menu_station(menu: &str) -> Option<StationKind> {
    Some(match menu {
        "minecraft:stonecutter" => StationKind::Stonecutter,
        "minecraft:grindstone" => StationKind::Grindstone,
        "minecraft:loom" => StationKind::Loom,
        "minecraft:cartography_table" => StationKind::Cartography,
        "minecraft:smithing" => StationKind::Smithing,
        _ => return None,
    })
}

fn java_ender_menu(menu: &str, title: &serde_json::Value) -> bool {
    menu == "minecraft:generic_9x3" && title.to_string().contains("container.enderchest")
}

#[derive(serde::Deserialize)]
struct JavaModParticles {
    t: String,
    p: [f64; 3],
    d: [f64; 3],
    s: [f64; 3],
    n: i32,
    #[serde(default)]
    f: bool,
}

impl super::Game<'_> {
    pub(super) fn java_mod_particles(&mut self, data: &[u8]) {
        let Ok(particles) = serde_json::from_slice::<JavaModParticles>(data) else {
            return;
        };
        if !self.particles.has_mod(&particles.t)
            || particles
                .p
                .iter()
                .chain(particles.d.iter())
                .chain(particles.s.iter())
                .any(|value| !value.is_finite())
        {
            return;
        }
        if particles.n <= 0 {
            let velocity = std::array::from_fn(|index| particles.s[index] * particles.d[index]);
            self.particles
                .add_mod(&particles.t, particles.p, velocity, particles.f);
            return;
        }
        for _ in 0..particles.n.min(4096) {
            let (position, velocity) = {
                let random = self.particles.random();
                let position = std::array::from_fn(|index| {
                    particles.p[index] + random.next_gaussian() * particles.d[index]
                });
                let velocity = std::array::from_fn(|index| {
                    random.next_gaussian() * particles.s[index]
                });
                (position, velocity)
            };
            self.particles
                .add_mod(&particles.t, position, velocity, particles.f);
        }
    }

    fn java_open_menu(&mut self, uuid: &str, menu: &str, title: Option<&str>) -> bool {
        let Some((conn, at, ender_items)) = self.lan.as_ref().and_then(|host| {
            host.hub
                .online_players()
                .find(|guest| {
                    guest.dimension == self.dim.id()
                        && horizon_protocol::crypto::uuid_string(guest.uuid) == uuid
                })
                .map(|guest| {
                    (
                        guest.conn,
                        [
                            guest.position[0].floor() as i32,
                            guest.position[1].floor() as i32,
                            guest.position[2].floor() as i32,
                        ],
                        guest.ender_items.clone(),
                    )
                })
        }) else {
            return false;
        };
        let title = title
            .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
            .unwrap_or(serde_json::Value::Null);
        if menu == "minecraft:crafting" {
            self.lan_open_crafting(conn, at);
            true
        } else if let Some(kind) = java_menu_station(menu) {
            self.lan_open_station(conn, &Station::new_kind(kind, at))
        } else if java_ender_menu(menu, &title) {
            if !self.guest_may_open(conn, menu, None) {
                return true;
            }
            self.guest_close_all(conn);
            let opened = self
                .lan
                .as_mut()
                .and_then(|host| {
                    host.hub.open_menu_window(
                        conn,
                        menu,
                        horizon_client::lan::data::component_nbt(&title),
                        None,
                        horizon_client::lan::menu::Slots::Ender,
                        ender_items,
                        None,
                    )
                })
                .is_some();
            if opened {
                self.player_menu_opening(conn);
            }
            opened
        } else {
            false
        }
    }

    pub(super) fn java_guest_payload(
        &self,
        conn: u32,
        channel: &str,
        body: &[u8],
    ) -> bool {
        let Some(uuid) = self.lan.as_ref().and_then(|host| {
            host.hub
                .guests
                .iter()
                .find(|guest| guest.conn == conn)
                .map(|guest| horizon_protocol::crypto::uuid_string(guest.uuid))
        }) else {
            return false;
        };
        match horizon_jvm::events::guest_payload(&uuid, channel, body) {
            Ok(handled) => handled,
            Err(error) => {
                log::warn!(
                    "Java mods: custom payload {channel} from {uuid} was not delivered: {error}"
                );
                false
            }
        }
    }

    pub(super) fn java_menu_click(
        &self,
        conn: u32,
        packet: &horizon_protocol::packets::play::ContainerClick,
    ) -> bool {
        let Some(host) = self.lan.as_ref() else {
            return false;
        };
        let Some(uuid) = host
            .hub
            .guests
            .iter()
            .find(|guest| guest.conn == conn)
            .map(|guest| horizon_protocol::crypto::uuid_string(guest.uuid))
        else {
            return false;
        };
        let window = JAVA_MENU_WINDOWS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&uuid)
            .map(|(owner, window, _)| (*owner, *window));
        if window != Some((conn, packet.container)) {
            return false;
        }
        if let Err(error) = horizon_jvm::events::menu_click(
            &uuid,
            i32::from(packet.slot),
            i32::from(packet.button),
            packet.input,
        ) {
            log::warn!("Java mods: menu click was not passed to {uuid}: {error}");
        }
        true
    }

    pub(super) fn java_menu_closed(&mut self, conn: u32, window: i32) -> bool {
        let Some(uuid) = self.lan.as_ref().and_then(|host| {
            host.hub
                .guests
                .iter()
                .find(|guest| guest.conn == conn)
                .map(|guest| horizon_protocol::crypto::uuid_string(guest.uuid))
        }) else {
            return false;
        };
        let removed = {
            let mut windows = JAVA_MENU_WINDOWS
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if windows
                .get(&uuid)
                .is_some_and(|(owner, current, _)| *owner == conn && *current == window)
            {
                windows.remove(&uuid);
                true
            } else {
                false
            }
        };
        if !removed {
            return false;
        }
        if let Err(error) = horizon_jvm::events::menu_close(&uuid) {
            log::warn!("Java mods: menu close was not passed to {uuid}: {error}");
        }
        if let Some(host) = self.lan.as_mut() {
            host.hub.container_closed(conn, window);
        }
        true
    }

    fn java_menu_content(
        &mut self,
        uuid: &str,
        open: Option<(String, Option<String>)>,
        items: Option<Vec<Option<String>>>,
        carried: Option<String>,
    ) -> bool {
        let Some(items) = items else {
            let removed = JAVA_MENU_WINDOWS
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(uuid);
            if let Some((conn, _, _)) = removed.as_ref()
                && let Some(host) = self.lan.as_mut()
            {
                host.hub.close_container(*conn);
            }
            return removed.is_some();
        };
        let Some(host) = self.lan.as_mut() else {
            return false;
        };
        let Some((guest_index, conn)) = host
            .hub
            .guests
            .iter()
            .enumerate()
            .find(|(_, guest)| horizon_protocol::crypto::uuid_string(guest.uuid) == uuid)
            .map(|(index, guest)| (index, guest.conn))
        else {
            return false;
        };
        let parse_item = |snbt: &str| {
            let (tag, _) = horizon_core::nbt::parse_snbt(snbt).ok()?;
            horizon_items::item_nbt::item_from_nbt(&tag)
        };
        let stacks = items
            .iter()
            .map(|item| item.as_deref().and_then(parse_item))
            .collect::<Vec<_>>();
        let carried = carried.as_deref().and_then(parse_item);
        host.hub.guests.update(guest_index, |guest| {
            guest.carried = carried;
        });
        let existing = JAVA_MENU_WINDOWS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(uuid)
            .cloned();
        if let Some((existing_conn, window, _)) = existing {
            if existing_conn == conn && open.is_none() {
                return host.hub.set_native_menu_items(conn, window, stacks);
            }
            if existing_conn == conn {
                host.hub.close_container(conn);
            }
            JAVA_MENU_WINDOWS
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(uuid);
        }
        let Some((menu, title)) = open else {
            return false;
        };
        let title = title
            .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
            .unwrap_or(serde_json::Value::Null);
        let Some(window) = host.hub.open_menu_window(
            conn,
            &menu,
            horizon_client::lan::data::component_nbt(&title),
            None,
            horizon_client::lan::menu::Slots::Native,
            stacks,
            None,
        ) else {
            log::warn!("Java mods: custom menu type {menu} is not registered in Starling's menu registry");
            return false;
        };
        JAVA_MENU_WINDOWS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(uuid.to_owned(), (conn, window, menu));
        true
    }

    pub(super) fn tick_java_server(&mut self) {
        if !horizon_jvm::events::enabled() {
            return;
        }
        let connected = self
            .lan
            .as_ref()
            .map(|host| {
                host.hub
                    .online_players()
                    .map(|guest| horizon_protocol::crypto::uuid_string(guest.uuid))
                    .collect::<std::collections::HashSet<_>>()
            })
            .unwrap_or_default();
        JAVA_MENU_WINDOWS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|uuid, _| connected.contains(uuid));
        let local_uuid = java_uuid(&self.level);
        let local_id = horizon_protocol::crypto::uuid_string(local_uuid);
        super::java_gui::set_local_uuid(&local_id);
        let mut players = Vec::new();
        let mut events = std::mem::take(
            &mut *JAVA_PENDING_EVENTS
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
        if let Some(player) = self.player.as_ref() {
            let input = u8::from(self.keys.held(horizon_client::actions::Action::Forward))
                | (u8::from(self.keys.held(horizon_client::actions::Action::Back)) << 1)
                | (u8::from(self.keys.held(horizon_client::actions::Action::Left)) << 2)
                | (u8::from(self.keys.held(horizon_client::actions::Action::Right)) << 3)
                | (u8::from(self.keys.held(horizon_client::actions::Action::Jump)) << 4)
                | (u8::from(self.keys.held(horizon_client::actions::Action::Sneak)) << 5)
                | (u8::from(self.keys.held(horizon_client::actions::Action::Sprint)) << 6);
            players.push(horizon_jvm::events::Player {
                uuid: local_id.clone(),
                name: horizon_client::accounts::player_name().unwrap_or_else(|| "Player".into()),
                position: [player.position.x, player.position.y, player.position.z],
                yaw: player.yaw,
                pitch: player.pitch,
                dimension: self.dim.id().to_owned(),
                game_mode: self.level.mode(),
                health: player.health.health(),
                food: player.health.food.food,
                saturation: player.health.food.saturation,
                exhaustion: player.health.food.exhaustion,
                permission: if self.level.allow_commands.unwrap_or(false) { 4 } else { 0 },
                sneaking: self.keys.held(horizon_client::actions::Action::Sneak),
                selected: self.selected.min(8) as u8,
                input,
                inventory: java_inventory(self, &local_id),
            });
        }
        if let Some(host) = self.lan.as_ref() {
            for guest in host.hub.online_players() {
                if guest.dimension != self.dim.id() {
                    continue;
                }
                let uuid = horizon_protocol::crypto::uuid_string(guest.uuid);
                players.push(horizon_jvm::events::Player {
                    uuid: uuid.clone(),
                    name: guest.name.clone(),
                    position: guest.position,
                    yaw: guest.yaw,
                    pitch: guest.pitch,
                    dimension: guest.dimension.clone(),
                    game_mode: guest.game_mode,
                    health: guest.health,
                    food: guest.food,
                    saturation: guest.saturation,
                    exhaustion: guest.motion.as_deref().map_or_else(
                        || guest.loaded_body.as_ref().map_or(0.0, |body| body.food_exhaustion),
                        |motion| motion.health.food.exhaustion,
                    ),
                    permission: guest.permission,
                    sneaking: guest.sneaking,
                    selected: guest.selected.min(8) as u8,
                    input: guest.input,
                    inventory: java_guest_inventory(guest, &uuid),
                });
            }
        }
        let current_players = players
            .iter()
            .map(|player| (player.uuid.clone(), player.health <= 0.0))
            .collect::<std::collections::HashMap<_, _>>();
        {
            let mut previous = JAVA_PLAYER_STATE
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            for uuid in previous.keys() {
                if !current_players.contains_key(uuid) {
                    events.push(horizon_jvm::events::Event::Left {
                        uuid: uuid.clone(),
                    });
                }
            }
            for (uuid, dead) in &current_players {
                match previous.get(uuid) {
                    None => events.push(horizon_jvm::events::Event::Joined {
                        uuid: uuid.clone(),
                    }),
                    Some(was_dead) if !*was_dead && *dead => {
                        events.push(horizon_jvm::events::Event::Died {
                            uuid: uuid.clone(),
                            cause: "minecraft:generic".into(),
                        });
                    }
                    Some(was_dead) if *was_dead && !*dead => {
                        events.push(horizon_jvm::events::Event::Respawned {
                            uuid: uuid.clone(),
                        });
                    }
                    _ => {}
                }
            }
            *previous = current_players;
        }
        {
            let active = players
                .iter()
                .map(|player| player.uuid.as_str())
                .collect::<std::collections::HashSet<_>>();
            JAVA_INVENTORY_STATE
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .retain(|uuid, _| active.contains(uuid.as_str()));
        }

        let tick = self.ticks.min(i64::MAX as u64) as i64;
        let game_time = self.world.game_time;
        let seed = self.level.seed;
        let world_root = horizon_client::save::dimension_dir(
            &self.world_root,
            "minecraft:overworld",
        );
        let blocks = StarlingJavaBlocks {
            world: &self.world,
            registry: self.pack.assets.registry,
            biomes: world_biomes!(self),
            dimension: self.dim.id(),
        };
        let rules = java_rules_string(self);
        let rules_changed = {
            let mut previous = JAVA_RULE_STATE
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if *previous == rules {
                false
            } else {
                *previous = rules.clone();
                true
            }
        };
        let tick_result = horizon_jvm::world::with_blocks(&blocks, || {
            if rules_changed
                && let Err(error) = horizon_jvm::events::game_rules(&rules)
            {
                log::warn!("Java mods: game rules not passed: {error}");
            }
            if let Some((uuid, bodies)) = crate::lan::packet_tap::take()
                && let Err(error) = horizon_jvm::events::client_packets(
                    &horizon_protocol::crypto::uuid_string(uuid),
                    &bodies,
                )
            {
                log::warn!("Java mods: the local player's packets were not passed to the client: {error}");
            }
            horizon_jvm::events::game_time(game_time, seed).and_then(|()| {
                horizon_jvm::events::server_tick(tick, false, &world_root, &players, &events)
            })
        });
        if let Err(error) = tick_result {
            log::warn!("Java mods: server tick failed: {error}");
        }
        self.apply_java_actions(&local_id);
        if let Ok(snapshot) = horizon_jvm::events::entities() {
            let guests = self
                .lan
                .as_ref()
                .map(|host| {
                    host.hub
                        .online_players()
                        .map(|guest| {
                            (
                                guest.conn,
                                horizon_protocol::crypto::uuid_string(guest.uuid),
                                guest.dimension.clone(),
                                guest.position,
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let recipients = std::iter::once(u32::MAX)
                .chain(guests.iter().map(|guest| guest.0))
                .collect::<std::collections::HashSet<_>>();
            self.java_entities.retain_recipients(&recipients);
            let local_position = self.player.as_ref().map_or([0.0; 3], |player| {
                [player.position.x, player.position.y, player.position.z]
            });
            if let Some(data) = self.java_entities.snapshot_payload(
                &snapshot,
                self.dim.id(),
                &local_id,
                local_position,
                u32::MAX,
            ) {
                self.remote_java_entities(&data);
            }
            for (conn, uuid, dimension, position) in guests {
                let Some(data) = self.java_entities.snapshot_payload(
                    &snapshot, &dimension, &uuid, position, conn,
                ) else {
                    continue;
                };
                if let Some(host) = self.lan.as_ref() {
                    let packet = horizon_protocol::packets::play::Clientbound::CustomPayload(
                        horizon_protocol::packets::configuration::CustomPayload {
                            channel: super::java_entities::CHANNEL.to_owned(),
                            data,
                        },
                    );
                    host.hub.send_to(conn, &packet);
                }
            }
        }
    }

    fn java_mob_effect(&self, index: i32) -> Option<&'static horizon_client::brewing::EffectDef> {
        let index = usize::try_from(index).ok()?;
        let types = JAVA_MOB_EFFECT_TYPES.get_or_init(|| {
            horizon_jvm::events::mob_effect_types().unwrap_or_default()
        });
        let id = types.get(index)?;
        horizon_client::brewing::effect(id)
    }

    fn apply_java_mob_effect(&mut self, packet: &horizon_protocol::packets::play::UpdateMobEffect) -> bool {
        let Some(def) = self.java_mob_effect(packet.effect) else {
            return false;
        };
        let mut effect = horizon_client::effects::EffectInstance::from_def(
            def,
            packet.duration,
            packet.amplifier,
        );
        effect.ambient = packet.flags & 1 != 0;
        effect.visible = packet.flags & 2 != 0;
        effect.show_icon = packet.flags & 4 != 0;
        if packet.flags & 8 == 0 && self.potions.effects.get(def.id).is_none() {
            let shown = !effect.ends_within(0);
            effect.client_blend = (shown, shown);
        }
        self.add_player_effect(effect);
        true
    }

    fn remove_java_mob_effect(&mut self, packet: &horizon_protocol::packets::play::RemoveMobEffect) -> bool {
        let Some(def) = self.java_mob_effect(packet.effect) else {
            return false;
        };
        self.remove_player_effects(Some(def.index));
        true
    }

    pub(super) fn apply_java_actions(&mut self, local_id: &str) {
        use horizon_protocol::packets::play::{self, Clientbound as C};
        let mut unhandled = 0usize;
        for action in horizon_jvm::events::take_actions() {
            match action {
                horizon_jvm::events::Action::Tell { uuid, text } => {
                    let message = java_text(&text);
                    if uuid == local_id {
                        self.system_message(message);
                    } else if let Some(host) = self.lan.as_ref()
                        && let Some(guest) = host
                            .hub
                            .online_players()
                            .find(|guest| horizon_protocol::crypto::uuid_string(guest.uuid) == uuid)
                    {
                        host.hub.send_to(
                            guest.conn,
                            &C::SystemChat(play::SystemChat {
                                content: horizon_client::lan::data::component_nbt(
                                    &message.component(),
                                ),
                                overlay: false,
                            }),
                        );
                    } else {
                        unhandled += 1;
                    }
                }
                horizon_jvm::events::Action::Overlay { uuid, text } => {
                    let message = java_text(&text);
                    if uuid == local_id {
                        self.native_client_action_bar(message, true);
                    } else if let Some(host) = self.lan.as_ref()
                        && let Some(guest) = host
                            .hub
                            .online_players()
                            .find(|guest| horizon_protocol::crypto::uuid_string(guest.uuid) == uuid)
                    {
                        host.hub.send_to(
                            guest.conn,
                            &C::SystemChat(play::SystemChat {
                                content: horizon_client::lan::data::component_nbt(
                                    &message.component(),
                                ),
                                overlay: true,
                            }),
                        );
                    } else {
                        unhandled += 1;
                    }
                }
                horizon_jvm::events::Action::Broadcast { text } => {
                    let message = java_text(&text);
                    self.system_message(message.clone());
                    if let Some(host) = self.lan.as_ref() {
                        host.hub.broadcast(&C::SystemChat(play::SystemChat {
                            content: horizon_client::lan::data::component_nbt(
                                &message.component(),
                            ),
                            overlay: false,
                        }));
                    }
                }
                horizon_jvm::events::Action::GameRule { id, value } => {
                    let parsed = match self.ui.rules.get(&id) {
                        Some(horizon_client::gamerules::Value::Bool(_)) => {
                            value.parse::<bool>().ok().map(horizon_client::gamerules::Value::Bool)
                        }
                        Some(horizon_client::gamerules::Value::Int(_)) => {
                            value.parse::<i32>().ok().map(horizon_client::gamerules::Value::Int)
                        }
                        None => None,
                    };
                    if let Some(rule) = self.ui.rules.rule(&id).cloned()
                        && let Some(value) = parsed
                    {
                        self.set_game_rule(&rule, value, None);
                    } else {
                        unhandled += 1;
                    }
                }
                horizon_jvm::events::Action::WorldSpawn {
                    dimension,
                    position,
                    yaw,
                    pitch,
                } if dimension == "minecraft:overworld" => {
                    self.set_world_spawn(position, [yaw, pitch]);
                }
                horizon_jvm::events::Action::SetBlock {
                    dimension,
                    position,
                    state,
                } if dimension == self.dim.id() => {
                    let registry = self.pack.assets.registry;
                    if let Some(state) = java_block_state(registry, &state) {
                        let flags = mod_api::BlockUpdateFlags::NEIGHBORS
                            | mod_api::BlockUpdateFlags::CLIENTS;
                        if !self.native_set_block(&dimension, position, state, flags.bits()) {
                            unhandled += 1;
                        }
                    } else {
                        unhandled += 1;
                    }
                }
                horizon_jvm::events::Action::SetBlockEntity {
                    dimension,
                    position,
                    snbt,
                } if dimension == self.dim.id() => {
                    let Ok((tag, _)) = horizon_core::nbt::parse_snbt(&snbt) else {
                        unhandled += 1;
                        continue;
                    };
                    if !matches!(&tag, horizon_core::nbt::Tag::Compound(_)) {
                        unhandled += 1;
                        continue;
                    }
                    let key = (position[0], position[1], position[2]);
                    if self.world.is_loaded_at(key.0, key.1, key.2)
                        && self.load_block_entity_data(key, &tag)
                    {
                        self.block_entity_updated(key);
                    } else {
                        unhandled += 1;
                    }
                }
                horizon_jvm::events::Action::Teleport {
                    uuid,
                    dimension,
                    position,
                    yaw,
                    pitch,
                } if dimension == self.dim.id()
                    && position.iter().all(|value| value.is_finite())
                    && yaw.is_finite()
                    && pitch.is_finite() =>
                {
                    if uuid == local_id {
                        let to = horizon_core::geometry::Vec3::new(
                            position[0],
                            position[1],
                            position[2],
                        );
                        if self.java_teleport_player(to) {
                            self.yaw = yaw;
                            self.pitch = pitch.clamp(-90.0, 90.0);
                        } else {
                            unhandled += 1;
                        }
                    } else {
                        let conn = self.lan.as_ref().and_then(|host| {
                            host.hub
                                .online_players()
                                .find(|guest| horizon_protocol::crypto::uuid_string(guest.uuid) == uuid)
                                .map(|guest| guest.conn)
                        });
                        let teleported = conn
                            .and_then(|conn| {
                                self.as_guest(conn, |game| {
                                    let to = horizon_core::geometry::Vec3::new(
                                        position[0],
                                        position[1],
                                        position[2],
                                    );
                                    if !game.java_teleport_player(to) {
                                        return false;
                                    }
                                    game.yaw = yaw;
                                    game.pitch = pitch.clamp(-90.0, 90.0);
                                    true
                                })
                            })
                            .unwrap_or(false);
                        if !teleported {
                            unhandled += 1;
                        }
                    }
                }
                horizon_jvm::events::Action::DropItem {
                    dimension,
                    position,
                    velocity,
                    item,
                    count,
                } if dimension == self.dim.id()
                    && position.iter().all(|value| value.is_finite())
                    && velocity.iter().all(|value| value.is_finite()) =>
                {
                    if item == "#experience_orb" {
                        if count > 0 {
                            self.spawn_orbs_toward(position, velocity, count as u32);
                        }
                    } else {
                        let item = if item.contains(':') {
                            item
                        } else {
                            format!("minecraft:{item}")
                        };
                        if let Some(info) = self.res.items().get(&item) {
                            let maximum = info.max_stack.max(1);
                            let mut remaining = count.max(0) as u32;
                            while remaining > 0 {
                                let stack_count = remaining.min(maximum);
                                self.spawn_item(
                                    horizon_items::items::ItemStack::new(&item, stack_count),
                                    position,
                                    velocity,
                                    0,
                                );
                                remaining -= stack_count;
                            }
                        } else {
                            unhandled += 1;
                        }
                    }
                }
                horizon_jvm::events::Action::SpawnEntity { dimension, snbt }
                    if dimension == self.dim.id() =>
                {
                    let Ok((tag, _)) = horizon_core::nbt::parse_snbt(&snbt) else {
                        unhandled += 1;
                        continue;
                    };
                    let Some(horizon_core::nbt::Tag::String(kind)) = tag.get("id") else {
                        unhandled += 1;
                        continue;
                    };
                    let Some(position) = horizon_client::entity_nbt::position(&tag) else {
                        unhandled += 1;
                        continue;
                    };
                    let kind = kind.to_string_lossy();
                    let position = [position.x, position.y, position.z];
                    if !position.iter().all(|value| value.is_finite())
                        || !matches!(self.summon_with_nbt(&kind, position, &snbt), Ok(Some(_)))
                    {
                        unhandled += 1;
                    }
                }
                horizon_jvm::events::Action::LevelEvent {
                    dimension,
                    id,
                    position,
                    data,
                    state,
                    except,
                } if dimension == self.dim.id() => {
                    let data = if matches!(id, 2001 | 2014 | 3008) {
                        let Some(state) = state
                            .as_deref()
                            .and_then(|state| java_block_state(self.pack.assets.registry, state))
                        else {
                            unhandled += 1;
                            continue;
                        };
                        state.0 as i32
                    } else {
                        data
                    };
                    let source = except
                        .as_deref()
                        .and_then(horizon_client::lan::arrivals::parse_uuid)
                        .map(horizon_world::level::LevelEventPlayer);
                    self.level_event_from(source, id, position, data);
                }
                horizon_jvm::events::Action::SetSlot { uuid, slot, item } if uuid == local_id => {
                    let stack = match item {
                        Some(item) => match horizon_items::item_data::parse_stack(&item) {
                            Ok(stack) => stack,
                            Err(_) => {
                                unhandled += 1;
                                continue;
                            }
                        },
                        None => None,
                    };
                    if self.player.is_none() {
                        unhandled += 1;
                        continue;
                    }
                    match slot {
                        0..=35 => {
                            if let Some(target) = self.inventory.get_mut(slot) {
                                *target = stack;
                            } else {
                                unhandled += 1;
                            }
                        }
                        36..=39 => self.armor[39 - slot] = stack,
                        40 => self.offhand = stack,
                        _ => unhandled += 1,
                    }
                }
                horizon_jvm::events::Action::Food {
                    uuid,
                    food,
                    saturation,
                } if uuid == local_id => {
                    if let Some(player) = self.player.as_mut() {
                        player.health.food.food = food.clamp(0, 20);
                        player.health.food.saturation = saturation.clamp(0.0, food.clamp(0, 20) as f32);
                    }
                }
                horizon_jvm::events::Action::Food {
                    uuid,
                    food,
                    saturation,
                } => {
                    let target = self
                        .lan
                        .as_ref()
                        .and_then(|host| {
                            host.hub
                                .online_players()
                                .find(|guest| horizon_protocol::crypto::uuid_string(guest.uuid) == uuid)
                                .map(|guest| (guest.conn, guest.health))
                        });
                    if let Some((conn, health)) = target {
                        if let Some(host) = self.lan.as_mut() {
                            let food = food.clamp(0, 20);
                            let saturation = saturation.clamp(0.0, food as f32);
                            host.hub.update_guest(conn, |guest| {
                                guest.food = food;
                                guest.saturation = saturation;
                            });
                            host.hub.send_to(
                                conn,
                                &C::SetHealth(play::SetHealth {
                                    health,
                                    food,
                                    saturation,
                                }),
                            );
                        }
                    } else {
                        unhandled += 1;
                    }
                }
                horizon_jvm::events::Action::OpenMenu { uuid, menu, title } => {
                    if !self.java_open_menu(&uuid, &menu, title.as_deref()) {
                        unhandled += 1;
                    }
                }
                horizon_jvm::events::Action::Abilities {
                    uuid,
                    game_mode,
                    flags,
                    flying_speed,
                    walking_speed,
                } => {
                    let applied = if uuid == local_id {
                        apply_java_abilities(
                            self,
                            game_mode,
                            flags,
                            flying_speed,
                            walking_speed,
                        )
                    } else {
                        let conn = self.lan.as_ref().and_then(|host| {
                            host.hub
                                .online_players()
                                .find(|guest| horizon_protocol::crypto::uuid_string(guest.uuid) == uuid)
                                .map(|guest| guest.conn)
                        });
                        conn.and_then(|conn| {
                            self.as_guest(conn, |game| {
                                apply_java_abilities(
                                    game,
                                    game_mode,
                                    flags,
                                    flying_speed,
                                    walking_speed,
                                )
                            })
                        })
                        .unwrap_or(false)
                    };
                    if !applied {
                        unhandled += 1;
                    }
                }
                horizon_jvm::events::Action::HurtPlayer {
                    uuid,
                    damage_type,
                    amount,
                    from,
                    by,
                } if amount.is_finite() && amount > 0.0 => {
                    let from = from.filter(|position| position.iter().all(|value| value.is_finite()));
                    let damage_type = if damage_type.contains(':') {
                        damage_type
                    } else {
                        format!("minecraft:{damage_type}")
                    };
                    if uuid == local_id {
                        if let Some(by) = by {
                            self.note_direct_entity(
                                &damage_type,
                                horizon_client::text::Text::literal(by),
                            );
                        }
                        if !self.hurt_player_from(amount, from, &damage_type, Some(None), false) {
                            unhandled += 1;
                        }
                    } else {
                        let conn = self.lan.as_ref().and_then(|host| {
                            host.hub
                                .online_players()
                                .find(|guest| horizon_protocol::crypto::uuid_string(guest.uuid) == uuid)
                                .map(|guest| guest.conn)
                        });
                        let applied = conn
                            .and_then(|conn| {
                                self.as_guest(conn, |game| {
                                    if let Some(by) = by {
                                        game.note_direct_entity(
                                            &damage_type,
                                            horizon_client::text::Text::literal(by),
                                        );
                                    }
                                    game.hurt_player_from(amount, from, &damage_type, Some(None), false)
                                })
                            })
                            .unwrap_or(false);
                        if !applied {
                            unhandled += 1;
                        }
                    }
                }
                horizon_jvm::events::Action::HurtMob {
                    dimension,
                    id,
                    damage_type,
                    amount,
                    from,
                    by,
                    reset_invulnerable,
                } if dimension == self.dim.id() && amount.is_finite() && amount > 0.0 => {
                    let Some(id) = u32::try_from(id).ok() else {
                        unhandled += 1;
                        continue;
                    };
                    let Some(kind) = java_damage_kind(&self.health_resources.damage, &damage_type) else {
                        unhandled += 1;
                        continue;
                    };
                    let from = from.filter(|position| position.iter().all(|value| value.is_finite()));
                    let attacker = by
                        .and_then(|attacker| u32::try_from(attacker).ok())
                        .filter(|attacker| self.mobs.iter().any(|mob| mob.id == *attacker));
                    if reset_invulnerable
                        && let Some(mob) = self.mobs.iter_mut().find(|mob| mob.id == id)
                    {
                        mob.invulnerable = 0;
                    }
                    let source = horizon_client::mobs::Damage {
                        kind,
                        attacker: attacker.map(horizon_client::mobs::Who::Mob),
                        from,
                        attacker_species: None,
                    };
                    let old_health = self
                        .mobs
                        .iter()
                        .find(|mob| mob.id == id)
                        .map(|mob| mob.health);
                    if self.mob_hits_mob(id, amount, source) {
                        if old_health.is_some_and(|old| {
                            self.mobs
                                .iter()
                                .find(|mob| mob.id == id)
                                .is_none_or(|mob| mob.health != old)
                        }) {
                            self.mob_damage_event(id);
                        }
                        if let Err(error) = self.settle_hurt_teleports() {
                            log::warn!("Java mods: entity hurt teleport: {error}");
                        }
                    } else {
                        unhandled += 1;
                    }
                }
                horizon_jvm::events::Action::ScreenFrame { uuid, frame } => {
                    if uuid == local_id {
                        super::java_gui::set_screen_frame(&frame);
                    }
                }
                horizon_jvm::events::Action::HudFrame { uuid, frame } => {
                    if uuid == local_id {
                        super::java_gui::set_hud_frame(&frame);
                    }
                }
                horizon_jvm::events::Action::OverlayFrame { uuid, frame } => {
                    if uuid == local_id {
                        super::java_gui::set_overlay_frame(&frame);
                    }
                }
                horizon_jvm::events::Action::ClientScreen { uuid, open } => {
                    if uuid == local_id {
                        super::java_gui::set_screen_open(open);
                    }
                }
                horizon_jvm::events::Action::MenuContent { uuid, open, items, carried } => {
                    if uuid == local_id {
                        super::java_gui::set_menu_open(items.is_some());
                    } else if !self.java_menu_content(&uuid, open, items, carried) {
                        unhandled += 1;
                    }
                }
                horizon_jvm::events::Action::Packet { uuid, data } => {
                    let mut reader = horizon_protocol::buf::Reader::new(&data);
                    let Ok(packet_id) = reader.varint() else {
                        unhandled += 1;
                        continue;
                    };
                    let body = reader.rest().to_vec();
                    if uuid == local_id {
                        match C::decode(packet_id, &body) {
                            Ok(C::CustomPayload(payload)) if payload.channel == "horizon:java_particles" => {
                                self.java_mod_particles(&payload.data);
                            }
                            Ok(C::UpdateMobEffect(packet)) => {
                                if !self.apply_java_mob_effect(&packet) {
                                    unhandled += 1;
                                }
                            }
                            Ok(C::RemoveMobEffect(packet)) => {
                                if !self.remove_java_mob_effect(&packet) {
                                    unhandled += 1;
                                }
                            }
                            _ => unhandled += 1,
                        }
                    } else if let Some(host) = self.lan.as_ref()
                        && let Some(conn) = host
                            .hub
                            .online_players()
                            .find(|guest| horizon_protocol::crypto::uuid_string(guest.uuid) == uuid)
                            .map(|guest| guest.conn)
                    {
                        host.hub.send_to(conn, &C::Unknown(packet_id, body));
                    } else {
                        unhandled += 1;
                    }
                }
                horizon_jvm::events::Action::Tell { .. }
                | horizon_jvm::events::Action::Overlay { .. }
                | horizon_jvm::events::Action::WorldSpawn { .. }
                | horizon_jvm::events::Action::SetBlock { .. }
                | horizon_jvm::events::Action::SetBlockEntity { .. }
                | horizon_jvm::events::Action::Teleport { .. } => unhandled += 1,
                _ => unhandled += 1,
            }
        }
        if unhandled > 0
            && !JAVA_UNHANDLED_ACTIONS_WARNED.swap(
                true,
                std::sync::atomic::Ordering::Relaxed,
            )
        {
            log::warn!(
                "Java mods: {unhandled} action(s) were ignored because Starling does not yet provide a compatible gameplay operation for them"
            );
        }
    }

    pub(super) fn attack_java_entity(&mut self, id: i32) -> bool {
        if self.remote.is_some() || !horizon_jvm::events::enabled() {
            return false;
        }
        let uuid = self
            .lan_acting_uuid
            .unwrap_or_else(|| java_uuid(&self.level));
        let uuid = horizon_protocol::crypto::uuid_string(uuid);
        let dimension = self.dim.id().to_owned();
        let blocks = StarlingJavaBlocks {
            world: &self.world,
            registry: self.pack.assets.registry,
            biomes: world_biomes!(self),
            dimension: &dimension,
        };
        let result = horizon_jvm::world::with_blocks(&blocks, || {
            horizon_jvm::events::attack_entity(&uuid, id)
        });
        self.apply_java_actions(&uuid);
        match result {
            Ok(taken) => taken,
            Err(error) => {
                log::warn!("Java mods: custom entity attack callback failed: {error}");
                false
            }
        }
    }

    pub(super) fn interact_java_entity(
        &mut self,
        id: i32,
        off_hand: bool,
        point: [f64; 3],
    ) -> horizon_jvm::events::UseResult {
        if self.remote.is_some() || !horizon_jvm::events::enabled() {
            return horizon_jvm::events::UseResult::Pass;
        }
        let uuid = self
            .lan_acting_uuid
            .unwrap_or_else(|| java_uuid(&self.level));
        let uuid = horizon_protocol::crypto::uuid_string(uuid);
        let dimension = self.dim.id().to_owned();
        let selected = self.selected as i32;
        let blocks = StarlingJavaBlocks {
            world: &self.world,
            registry: self.pack.assets.registry,
            biomes: world_biomes!(self),
            dimension: &dimension,
        };
        let result = horizon_jvm::world::with_blocks(&blocks, || {
            horizon_jvm::events::interact_entity(
                &uuid,
                id,
                off_hand,
                point,
                selected,
            )
        });
        self.apply_java_actions(&uuid);
        match result {
            Ok(answer) => answer,
            Err(error) => {
                log::warn!("Java mods: custom entity interaction callback failed: {error}");
                horizon_jvm::events::UseResult::Pass
            }
        }
    }
}


#[cfg(test)]
mod java_menu_tests {
    use super::{StationKind, java_ender_menu, java_menu_station};

    #[test]
    fn java_station_menu_ids_map_to_native_station_kinds() {
        assert_eq!(
            java_menu_station("minecraft:stonecutter"),
            Some(StationKind::Stonecutter)
        );
        assert_eq!(
            java_menu_station("minecraft:grindstone"),
            Some(StationKind::Grindstone)
        );
        assert_eq!(
            java_menu_station("minecraft:loom"),
            Some(StationKind::Loom)
        );
        assert_eq!(
            java_menu_station("minecraft:cartography_table"),
            Some(StationKind::Cartography)
        );
        assert_eq!(
            java_menu_station("minecraft:smithing"),
            Some(StationKind::Smithing)
        );
        assert_eq!(java_menu_station("minecraft:generic_9x3"), None);
    }

    #[test]
    fn java_ender_chest_menu_requires_the_vanilla_type_and_title() {
        let title = serde_json::json!({"translate": "container.enderchest"});
        assert!(java_ender_menu("minecraft:generic_9x3", &title));
        assert!(!java_ender_menu("minecraft:generic_9x4", &title));
        assert!(!java_ender_menu("minecraft:generic_9x3", &serde_json::Value::Null));
    }
}

#[cfg(test)]
mod java_particle_payload_tests {
    use super::JavaModParticles;

    #[test]
    fn java_particle_payload_defaults_override_limiter_to_false() {
        let payload = br#"{"t":"example:spark","p":[1.0,2.0,3.0],"d":[0.1,0.2,0.3],"s":[0.4,0.5,0.6],"n":4}"#;
        let particles = serde_json::from_slice::<JavaModParticles>(payload).unwrap();
        assert_eq!(particles.t, "example:spark");
        assert_eq!(particles.n, 4);
        assert!(!particles.f);
    }

    #[test]
    fn java_particle_payload_rejects_malformed_vectors() {
        let payload = br#"{"t":"example:spark","p":[1.0,2.0],"d":[0.1,0.2,0.3],"s":[0.4,0.5,0.6],"n":4}"#;
        assert!(serde_json::from_slice::<JavaModParticles>(payload).is_err());
    }
}
