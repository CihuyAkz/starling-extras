use super::Game;
use horizon_client::mesher::Vertex;
use horizon_client::mobs::{Part, anim::Poses, render};
use horizon_core::geometry::{Aabb, Vec3};
use horizon_protocol::packets::play::{self, Serverbound as S};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub(super) const CHANNEL: &str = "horizon:java_entities";

#[derive(Default)]
pub(super) struct JavaEntities {
    trees: HashMap<String, Arc<Part>>,
    raw_trees: HashMap<String, Value>,
    raw_looks: HashMap<i64, Value>,
    known_entities: HashMap<u32, HashSet<i64>>,
    known_trees: HashMap<u32, HashSet<String>>,
    now: Vec<Shown>,
    before: HashMap<i32, Shown>,
    own: Vec<Shown>,
    hands: Option<Hands>,
}

#[derive(Clone)]
struct Drawn {
    key: String,
    sprite: String,
    matrix: [f32; 16],
    tint: u32,
    overlay: u32,
    light: Option<u32>,
    poses: HashMap<String, [f32; 9]>,
    hidden: Vec<String>,
    skip: Vec<String>,
}

#[derive(Clone)]
struct Item {
    stack: horizon_client::items::ItemStack,
    context: horizon_assets::item_model::DisplayContext,
    matrix: [f32; 16],
}

#[derive(Clone)]
struct Mesh {
    sprite: String,
    overlay: u32,
    glow: bool,
    light: Option<u32>,
    vertex_lights: Option<Vec<Option<u32>>>,
    vertices: Vec<[f32; 8]>,
    colors: Vec<u32>,
}

#[derive(Clone)]
struct Shown {
    id: i32,
    kind: String,
    position: [f64; 3],
    width: f64,
    height: f64,
    models: Vec<Drawn>,
    meshes: Vec<Mesh>,
    items: Vec<Item>,
    name: Option<horizon_client::text::Text>,
    name_visible: bool,
    name_y: Option<f64>,
    shadow: f32,
    on_fire: bool,
    block: bool,
}

#[derive(Clone, Default)]
struct Hands {
    models: Vec<Drawn>,
    meshes: Vec<Mesh>,
    items: Vec<Item>,
    hide: [bool; 2],
}

#[derive(serde::Deserialize)]
struct RawPayload {
    #[serde(default)]
    entities: Vec<RawShown>,
    #[serde(default)]
    own: Vec<RawShown>,
    #[serde(default)]
    hands: Option<RawHands>,
    #[serde(default)]
    models: HashMap<String, Value>,
}

#[derive(serde::Deserialize)]
struct RawHands {
    #[serde(default)]
    models: Vec<RawModel>,
    #[serde(default)]
    items: Vec<Value>,
    #[serde(default)]
    hide: [bool; 2],
}

#[derive(serde::Deserialize)]
struct RawShown {
    id: i64,
    #[serde(rename = "type", default)]
    kind: String,
    x: f64,
    y: f64,
    z: f64,
    w: Option<f64>,
    h: Option<f64>,
    #[serde(default)]
    block: bool,
    #[serde(default)]
    whole: bool,
    #[serde(default)]
    models: Vec<RawModel>,
    #[serde(default)]
    items: Vec<Value>,
    name: Option<Value>,
    #[serde(default)]
    name_visible: bool,
    tag: Option<Value>,
    #[serde(default)]
    tag_always: bool,
    tag_y: Option<f64>,
    #[serde(default)]
    shadow: f32,
    #[serde(default)]
    on_fire: bool,
    #[serde(default)]
    keep: bool,
}

#[derive(serde::Deserialize)]
struct RawModel {
    key: Option<String>,
    tex: Option<String>,
    m: Option<[f32; 16]>,
    tint: Option<i64>,
    overlay: Option<i64>,
    light: Option<i64>,
    #[serde(default)]
    poses: HashMap<String, [f32; 9]>,
    #[serde(default)]
    hidden: Vec<String>,
    #[serde(default)]
    skip: Vec<String>,
    v: Option<Vec<f64>>,
    #[serde(default)]
    vertex_lights: Vec<Option<i64>>,
    #[serde(default)]
    glow: bool,
}

impl RawModel {
    fn drawn(self) -> Option<Drawn> {
        Some(Drawn {
            key: self.key?,
            sprite: sprite_key(self.tex.as_deref()?),
            matrix: self.m?,
            tint: self.tint.unwrap_or(-1) as u32,
            overlay: self.overlay.unwrap_or(0) as u32,
            light: self.light.map(|value| vertex_light(value as u32)),
            poses: self.poses,
            hidden: self.hidden,
            skip: self.skip,
        })
    }

    fn mesh(self) -> Option<Mesh> {
        let raw = self.v?;
        let mut vertices = Vec::with_capacity(raw.len() / 9);
        let mut colors = Vec::with_capacity(raw.len() / 9);
        for values in raw.chunks_exact(9) {
            vertices.push(std::array::from_fn(|index| values[index] as f32));
            colors.push(values[8] as u32);
        }
        let lights = self
            .vertex_lights
            .into_iter()
            .map(|light| light.and_then(|value| u32::try_from(value).ok()).map(vertex_light))
            .collect::<Vec<_>>();
        Some(Mesh {
            sprite: sprite_key(self.tex.as_deref()?),
            overlay: self.overlay.unwrap_or(0) as u32,
            glow: self.glow,
            light: self.light.map(|value| vertex_light(value as u32)),
            vertex_lights: (lights.len() == vertices.len()).then_some(lights),
            vertices,
            colors,
        })
    }
}

impl RawShown {
    fn shown(self) -> Shown {
        let (tag, visible) = match self.tag {
            Some(tag) => (Some(tag), self.tag_always),
            None => (self.name, self.name_visible),
        };
        let (mut models, mut meshes) = (Vec::new(), Vec::new());
        for model in self.models {
            if model.v.is_some() {
                meshes.extend(model.mesh());
            } else {
                models.extend(model.drawn());
            }
        }
        Shown {
            id: self.id as i32,
            kind: self.kind,
            position: [self.x, self.y, self.z],
            width: self.w.unwrap_or(0.6),
            height: self.h.unwrap_or(1.8),
            models,
            meshes,
            items: self.items.iter().filter_map(parse_item).collect(),
            name: tag.map(|value| {
                horizon_client::component::to_text(&value, &mut |key| {
                    horizon_client::text::Text::literal(key)
                })
            }),
            name_visible: visible,
            name_y: self.tag_y,
            shadow: self.shadow,
            on_fire: self.on_fire,
            block: self.block,
        }
    }
}

fn sprite_key(id: &str) -> String {
    let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
    let path = path
        .trim_start_matches("textures/")
        .trim_end_matches(".png");
    if namespace == "minecraft" {
        path.to_owned()
    } else {
        format!("{namespace}:{path}")
    }
}

fn floats<const N: usize>(value: &Value) -> Option<[f32; N]> {
    let values = value.as_array()?;
    (values.len() == N)
        .then(|| std::array::from_fn(|index| values[index].as_f64().unwrap_or(0.0) as f32))
}

fn parse_item(value: &Value) -> Option<Item> {
    let (tag, _) = horizon_core::nbt::parse_snbt(value.get("item")?.as_str()?).ok()?;
    Some(Item {
        stack: horizon_items::item_nbt::item_from_nbt(&tag)?,
        context: horizon_assets::item_model::DisplayContext::by_name(
            value.get("ctx")?.as_str()?,
        )?,
        matrix: floats(value.get("m")?)?,
    })
}

fn parse_models(raw: Vec<RawModel>) -> (Vec<Drawn>, Vec<Mesh>) {
    let (mut models, mut meshes) = (Vec::new(), Vec::new());
    for model in raw {
        if model.v.is_some() {
            meshes.extend(model.mesh());
        } else {
            models.extend(model.drawn());
        }
    }
    (models, meshes)
}

pub(super) fn used_past_entities(item: &str) -> bool {
    matches!(
        item,
        "minecraft:bow"
            | "minecraft:crossbow"
            | "minecraft:trident"
            | "minecraft:snowball"
            | "minecraft:egg"
            | "minecraft:ender_pearl"
            | "minecraft:experience_bottle"
            | "minecraft:splash_potion"
            | "minecraft:lingering_potion"
            | "minecraft:firework_rocket"
    )
}

fn abgr(argb: u32) -> u32 {
    let (a, r, g, b) = (
        argb >> 24,
        (argb >> 16) & 255,
        (argb >> 8) & 255,
        argb & 255,
    );
    a << 24 | b << 16 | g << 8 | r
}

fn overlay_code(packed: u32) -> u32 {
    let (u, v) = (packed & 0xFFFF, packed >> 16);
    if v < 8 {
        1
    } else if u == 0 {
        0
    } else {
        horizon_client::overlay::WHITE + u.min(15)
    }
}

fn vertex_light(packed: u32) -> u32 {
    (packed & 0xFF) | (((packed >> 16) & 0xFF) << 8)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl JavaEntities {
    pub(super) fn retain_recipients(&mut self, recipients: &HashSet<u32>) {
        self.known_entities
            .retain(|recipient, _| recipients.contains(recipient));
        self.known_trees
            .retain(|recipient, _| recipients.contains(recipient));
    }

    pub(super) fn snapshot_payload(
        &mut self,
        snapshot: &str,
        dimension: &str,
        uuid: &str,
        position: [f64; 3],
        recipient: u32,
    ) -> Option<Vec<u8>> {
        let root = serde_json::from_str::<Value>(snapshot).ok()?;
        if let Some(models) = root.get("models").and_then(Value::as_object) {
            self.raw_trees.extend(
                models
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone())),
            );
        }
        let dims = root.get("dims")?.as_object()?;
        let mut present = HashSet::new();
        for entries in dims.values().filter_map(Value::as_array) {
            for entry in entries {
                if let Some(id) = entry.get("id").and_then(Value::as_i64) {
                    present.insert(id);
                    if !entry.get("keep").and_then(Value::as_bool).unwrap_or(false)
                        && let Some(raw) = entry.get("e")
                    {
                        self.raw_looks.insert(id, raw.clone());
                    }
                }
            }
        }
        self.raw_looks.retain(|id, _| present.contains(id));
        let selected = dims
            .get(dimension)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut entities = Vec::new();
        let mut own = Vec::new();
        let mut current = HashSet::new();
        let mut model_keys = HashSet::new();
        for entry in selected {
            let Some(x) = entry.get("x").and_then(Value::as_f64) else {
                continue;
            };
            let Some(y) = entry.get("y").and_then(Value::as_f64) else {
                continue;
            };
            let Some(z) = entry.get("z").and_then(Value::as_f64) else {
                continue;
            };
            let dx = x - position[0];
            let dy = y - position[1];
            let dz = z - position[2];
            if dx * dx + dy * dy + dz * dz > 160.0 * 160.0 {
                continue;
            }
            let Some(raw) = entry.get("e").cloned() else {
                continue;
            };
            let id = entry
                .get("id")
                .and_then(Value::as_i64)
                .or_else(|| raw.get("id").and_then(Value::as_i64));
            let is_keep = entry.get("keep").and_then(Value::as_bool).unwrap_or(false);
            let is_own = raw.get("owner").and_then(Value::as_str) == Some(uuid);
            let known = id.is_some_and(|id| {
                self.known_entities
                    .get(&recipient)
                    .is_some_and(|entities| entities.contains(&id))
            });
            let mut look = if is_keep && !is_own && !known {
                let Some(previous) = id.and_then(|id| self.raw_looks.get(&id).cloned()) else {
                    continue;
                };
                previous
            } else {
                raw
            };
            if is_keep {
                if let Some(object) = look.as_object_mut() {
                    if !known {
                        object.remove("keep");
                    }
                    object.insert("x".into(), Value::from(x));
                    object.insert("y".into(), Value::from(y));
                    object.insert("z".into(), Value::from(z));
                }
            }
            if let Some(models) = look.get("models").and_then(Value::as_array) {
                for model in models {
                    if let Some(key) = model
                        .get("key")
                        .or_else(|| model.get("img"))
                        .and_then(Value::as_str)
                    {
                        model_keys.insert(key.to_owned());
                    }
                }
            }
            if is_own {
                own.push(look);
            } else {
                if let Some(id) = id {
                    current.insert(id);
                }
                entities.push(look);
            }
        }
        self.known_entities.insert(recipient, current);
        let hands = root
            .get("hands")
            .and_then(|value| value.get(uuid))
            .cloned()
            .unwrap_or(Value::Null);
        if let Some(keys) = hands.get("keys").and_then(Value::as_array) {
            for key in keys.iter().filter_map(Value::as_str) {
                model_keys.insert(key.to_owned());
            }
        }
        let known = self.known_trees.entry(recipient).or_default();
        let mut models = serde_json::Map::new();
        for key in model_keys {
            if known.contains(&key) {
                continue;
            }
            if let Some(tree) = self.raw_trees.get(&key) {
                models.insert(key.clone(), tree.clone());
                known.insert(key);
            }
        }
        let payload = serde_json::json!({
            "entities": entities,
            "own": own,
            "hands": hands,
            "models": models,
        });
        serde_json::to_vec(&payload).ok()
    }

    fn receive(&mut self, data: &[u8]) -> Vec<[i32; 3]> {
        let Ok(payload) = serde_json::from_slice::<RawPayload>(data) else {
            return Vec::new();
        };
        let whole = payload
            .entities
            .iter()
            .filter(|raw| raw.whole && !raw.keep)
            .map(|raw| [raw.x.floor() as i32, raw.y.floor() as i32, raw.z.floor() as i32])
            .collect();
        for (key, tree) in payload.models {
            if !key.starts_with("img:") {
                self.trees.insert(key, Arc::new(Part::parse("root", &tree)));
            }
        }
        let last: HashMap<i32, &Shown> = self.now.iter().map(|shown| (shown.id, shown)).collect();
        let now = payload
            .entities
            .into_iter()
            .filter_map(|raw| {
                if raw.keep {
                    let mut previous = (**last.get(&(raw.id as i32))?).clone();
                    previous.position = [raw.x, raw.y, raw.z];
                    Some(previous)
                } else {
                    Some(raw.shown())
                }
            })
            .collect();
        self.own = payload.own.into_iter().map(RawShown::shown).collect();
        self.hands = payload.hands.map(|hands| {
            let (models, meshes) = parse_models(hands.models);
            Hands {
                models,
                meshes,
                items: hands.items.iter().filter_map(parse_item).collect(),
                hide: hands.hide,
            }
        });
        self.before = std::mem::replace(&mut self.now, now)
            .into_iter()
            .map(|shown| (shown.id, shown))
            .collect();
        whole
    }

    fn position(&self, shown: &Shown, alpha: f64) -> [f64; 3] {
        let Some(previous) = self.before.get(&shown.id) else {
            return shown.position;
        };
        std::array::from_fn(|index| {
            previous.position[index] + (shown.position[index] - previous.position[index]) * alpha
        })
    }

    fn bounds(&self, shown: &Shown, alpha: f64) -> Aabb {
        let [x, y, z] = self.position(shown, alpha);
        let half = shown.width / 2.0;
        Aabb::new(
            Vec3::new(x - half, y, z - half),
            Vec3::new(x + half, y + shown.height, z + half),
        )
    }
}

impl Game<'_> {
    pub(super) fn remote_java_entities(&mut self, data: &[u8]) {
        let whole = self.java_entities.receive(data);
        let mut changed = false;
        for [x, y, z] in whole {
            let state = self.world.block(x, y, z);
            if state.0 != 0 {
                changed |= horizon_client::mesher::hide_model_drawn_by_block_entity(state);
            }
        }
        if changed {
            self.all_sections_changed();
        }
    }

    pub(super) fn java_entity_shadows(&self, alpha: f64) -> Vec<(glam::DVec3, f32, f32)> {
        self.java_entities
            .now
            .iter()
            .filter(|shown| shown.shadow > 0.0 && !shown.block)
            .map(|shown| {
                (
                    glam::DVec3::from_array(self.java_entities.position(shown, alpha)),
                    shown.shadow,
                    1.0,
                )
            })
            .collect()
    }

    pub(super) fn java_entity_names(
        &self,
        alpha: f64,
    ) -> Vec<(i32, glam::DVec3, f64, horizon_client::text::Text, bool)> {
        self.java_entities
            .now
            .iter()
            .filter_map(|shown| {
                let name = shown.name.clone()?;
                let position = glam::DVec3::from_array(self.java_entities.position(shown, alpha));
                Some((shown.id, position, shown.name_y.unwrap_or(shown.height), name, shown.name_visible))
            })
            .collect()
    }

    pub(super) fn java_entity_hit(&self) -> Option<(i32, f64)> {
        let (eye, dir) = self.aim()?;
        let reach = if self.creative { 5.0 } else { 3.0 };
        self.java_entities
            .now
            .iter()
            .filter(|shown| !shown.block)
            .filter_map(|shown| {
                Some((shown.id, super::ray_box(eye, dir, reach, self.java_entities.bounds(shown, 1.0))?))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    pub(super) fn java_entity_is_nearest(&self, distance: f64) -> bool {
        let nearer = [
            self.painting_hit().map(|hit| hit.1),
            self.frame_hit().map(|hit| hit.1),
            self.armor_stand_hit().map(|hit| hit.1),
            self.cushion_hit().map(|hit| hit.1),
            self.boat_hit().map(|hit| hit.1),
            self.cart_hit().map(|hit| hit.1),
            self.interaction_hit().map(|hit| hit.1),
            self.dragon_hit().map(|hit| hit.1),
            self.mob_hit().map(|hit| hit.1),
            self.redirectable_hit().map(|hit| hit.1),
        ]
        .into_iter()
        .flatten()
        .any(|other| other < distance);
        !nearer && self.before_block(distance)
    }

    pub(super) fn java_entity_interact_target(
        &self,
        target: Option<super::InteractTarget>,
    ) -> Option<(i32, [f64; 3])> {
        if let Some(target) = target {
            let shown = self
                .java_entities
                .now
                .iter()
                .find(|shown| shown.id == target.id && !shown.block)?;
            return Some((
                target.id,
                std::array::from_fn(|axis| shown.position[axis] + target.location[axis]),
            ));
        }
        let (id, distance) = self.java_entity_hit()?;
        if !self.java_entity_is_nearest(distance) {
            return None;
        }
        let (eye, direction) = self.aim()?;
        Some((
            id,
            std::array::from_fn(|axis| eye[axis] + direction[axis] * distance),
        ))
    }

    pub(super) fn attack_java_entity_local(&mut self, id: i32) -> bool {
        super::Game::attack_java_entity(self, id)
    }

    pub(super) fn interact_java_entity_local(
        &mut self,
        target: Option<super::InteractTarget>,
        hand: super::Hand,
    ) -> Option<horizon_jvm::events::UseResult> {
        let (id, point) = self.java_entity_interact_target(target)?;
        Some(super::Game::interact_java_entity(
            self,
            id,
            hand == super::Hand::Off,
            point,
        ))
    }

    pub(super) fn remote_attack_java_entity(&mut self) -> bool {
        let Some((id, distance)) = self.java_entity_hit() else {
            return false;
        };
        if !self.java_entity_is_nearest(distance) {
            return false;
        }
        self.send(S::Attack(play::Attack { id }));
        self.reset_attack_strength();
        true
    }

    pub(super) fn java_entity_location(&self, id: i32, distance: f64) -> [f64; 3] {
        let (Some((eye, dir)), Some(shown)) = (
            self.aim(),
            self.java_entities.now.iter().find(|shown| shown.id == id),
        ) else {
            return [0.0; 3];
        };
        let point = std::array::from_fn::<_, 3, _>(|index| eye[index] + dir[index] * distance);
        std::array::from_fn(|index| point[index] - shown.position[index])
    }

    pub(super) fn java_entity_geometry(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<Vertex>,
    ) {
        let java = &self.java_entities;
        if java.now.is_empty() && java.own.is_empty() {
            return;
        }
        let t = alpha as f32;
        for shown in &java.now {
            let at = java.position(shown, alpha);
            let relative = glam::DVec3::from_array(at) - camera;
            let range = if shown.block {
                64.0
            } else {
                (shown.width * 2.0 + shown.height) / 3.0 * 64.0 * self.view_scale()
            };
            if relative.length_squared() >= range * range {
                continue;
            }
            let lit_y = if shown.block {
                at[1] + 1.0
            } else {
                at[1] + shown.height * 0.85
            };
            let light = self.world.light(
                at[0].floor() as i32,
                lit_y.floor() as i32,
                at[2].floor() as i32,
            );
            let packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            let before = java.before.get(&shown.id);
            let base = glam::Mat4::from_translation(relative.as_vec3());
            self.draw_java_look(
                shown.id,
                &shown.models,
                &shown.meshes,
                &shown.items,
                before,
                base,
                packed,
                t,
                out,
            );
            if shown.on_fire && !shown.block {
                let fire_yaw = if self.camera_type == horizon_client::camera::CameraType::ThirdPersonFront {
                    self.yaw + 180.0
                } else {
                    self.yaw
                };
                self.mark_entity(out.len(), "minecraft:entity_flame");
                horizon_client::fire_render::flames(
                    at,
                    shown.width as f32,
                    shown.height as f32,
                    camera,
                    fire_yaw,
                    &self.gui.atlas.sprites,
                    self.world.light(
                        at[0].floor() as i32,
                        at[1].floor() as i32,
                        at[2].floor() as i32,
                    ),
                    out,
                );
            }
        }
        if !self.camera_type.is_first_person()
            && let Some(player) = self.player_model.as_ref()
        {
            let at = glam::DVec3::new(player.position.x, player.position.y, player.position.z);
            let light = self.world.light(
                at.x.floor() as i32,
                (at.y + 1.62).floor() as i32,
                at.z.floor() as i32,
            );
            let packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            for shown in &java.own {
                let base = glam::Mat4::from_translation((at - camera).as_vec3());
                self.draw_java_look(
                    shown.id,
                    &shown.models,
                    &shown.meshes,
                    &shown.items,
                    None,
                    base,
                    packed,
                    t,
                    out,
                );
            }
        }
    }

    pub(super) fn java_hand_geometry(
        &self,
        to_world: glam::Mat4,
        packed: u32,
        out: &mut Vec<Vertex>,
    ) -> [bool; 2] {
        if let Some(hands) = &self.java_entities.hands {
            self.draw_java_look(
                -1,
                &hands.models,
                &hands.meshes,
                &hands.items,
                None,
                to_world,
                packed,
                1.0,
                out,
            );
            hands.hide
        } else {
            [false; 2]
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_java_look(
        &self,
        id: i32,
        models: &[Drawn],
        meshes: &[Mesh],
        items: &[Item],
        before: Option<&Shown>,
        base: glam::Mat4,
        packed: u32,
        partial: f32,
        out: &mut Vec<Vertex>,
    ) {
        for (index, model) in models.iter().enumerate() {
            let Some(tree) = self.java_entities.trees.get(&model.key) else {
                continue;
            };
            let Some(sprite) = self.gui.atlas.sprites.get(&model.sprite) else {
                continue;
            };
            let previous = before
                .and_then(|shown| shown.models.get(index))
                .filter(|previous| previous.key == model.key);
            let matrix = match previous {
                Some(previous) => {
                    std::array::from_fn(|i| lerp(previous.matrix[i], model.matrix[i], partial))
                }
                None => model.matrix,
            };
            let mut poses = Poses::new(tree);
            for (path, pose) in &model.poses {
                let previous_pose = previous.and_then(|last| last.poses.get(path));
                *poses.get(path) = match previous_pose {
                    Some(from) => std::array::from_fn(|i| lerp(from[i], pose[i], partial)),
                    None => *pose,
                };
            }
            if let Some(previous) = previous {
                for (path, from) in &previous.poses {
                    if !model.poses.contains_key(path) && poses.has(path) {
                        let initial = *poses.get(path);
                        *poses.get(path) =
                            std::array::from_fn(|i| lerp(from[i], initial[i], partial));
                    }
                }
            }
            for path in &model.hidden {
                poses.visible(path, false);
            }
            for path in &model.skip {
                poses.skip_draw(path);
            }
            render::draw(
                tree,
                base * glam::Mat4::from_cols_array(&matrix),
                &poses,
                [sprite.u0, sprite.v0, sprite.u1, sprite.v1],
                abgr(model.tint),
                model.light.unwrap_or(packed),
                overlay_code(model.overlay),
                out,
            );
        }
        for mesh in meshes {
            let Some(sprite) = self.gui.atlas.sprites.get(&mesh.sprite) else {
                continue;
            };
            let overlay = overlay_code(mesh.overlay);
            for (index, (vertex, &color)) in mesh
                .vertices
                .iter()
                .zip(&mesh.colors)
                .enumerate()
            {
                let normal = [vertex[5], vertex[6], vertex[7]]
                    .map(|component| (component.clamp(-1.0, 1.0) * 127.0) as i8);
                let vertex_light = mesh
                    .vertex_lights
                    .as_ref()
                    .and_then(|lights| lights.get(index).copied().flatten());
                out.push(Vertex {
                    pos: base
                        .transform_point3(glam::Vec3::new(vertex[0], vertex[1], vertex[2]))
                        .to_array(),
                    uv: [
                        sprite.u0 + (sprite.u1 - sprite.u0) * vertex[3],
                        sprite.v0 + (sprite.v1 - sprite.v0) * vertex[4],
                    ],
                    layer: overlay,
                    color: abgr(color),
                    light: if mesh.glow {
                        horizon_client::mobs::render::FULL_BRIGHT
                    } else {
                        vertex_light.or(mesh.light).unwrap_or(packed)
                    },
                    normal: [normal[0], normal[1], normal[2], -127],
                    glint_uv: [0.0, 0.0],
                });
            }
        }
        for item in items {
            let seed = id.wrapping_add(item.context as i32);
            let state = self.resolve_item(&item.stack, item.context, None, seed);
            let pose = base * glam::Mat4::from_cols_array(&item.matrix);
            self.submit_item_world(&state, pose, packed, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HashSet, JavaEntities, RawModel, vertex_light};
    use serde_json::Value;

    #[test]
    fn idle_entity_keeps_the_last_look_for_a_known_player() {
        let mut entities = JavaEntities::default();
        let first = r#"{"dims":{"minecraft:overworld":[{"x":1.0,"y":2.0,"z":3.0,"id":7,"e":{"id":7,"type":"example:mob","x":1.0,"y":2.0,"z":3.0,"w":0.8,"h":1.4,"models":[{"key":"example:model","tex":"example:textures/entity/mob.png","m":[1.0,0.0,0.0,0.0,0.0,1.0,0.0,0.0,0.0,0.0,1.0,0.0,0.0,0.0,0.0,1.0]}]}}]},"models":{"example:model":{}}}"#;
        let first = entities
            .snapshot_payload(first, "minecraft:overworld", "player-a", [0.0; 3], 1)
            .unwrap();
        let first: Value = serde_json::from_slice(&first).unwrap();
        assert_eq!(first["entities"][0]["models"][0]["key"], "example:model");

        let second = r#"{"dims":{"minecraft:overworld":[{"x":2.0,"y":2.0,"z":3.0,"id":7,"keep":true,"e":{"id":7,"type":"example:mob","x":2.0,"y":2.0,"z":3.0,"keep":true}}]}}"#;
        let second = entities
            .snapshot_payload(second, "minecraft:overworld", "player-a", [0.0; 3], 1)
            .unwrap();
        let second: Value = serde_json::from_slice(&second).unwrap();
        assert_eq!(second["entities"][0]["keep"], true);
        assert_eq!(second["entities"][0]["x"], 2.0);
    }

    #[test]
    fn idle_entity_sends_a_full_look_to_a_new_player() {
        let mut entities = JavaEntities::default();
        let first = r#"{"dims":{"minecraft:overworld":[{"x":1.0,"y":2.0,"z":3.0,"id":7,"e":{"id":7,"type":"example:mob","x":1.0,"y":2.0,"z":3.0,"w":0.8,"h":1.4,"models":[{"key":"example:model","tex":"example:textures/entity/mob.png","m":[1.0,0.0,0.0,0.0,0.0,1.0,0.0,0.0,0.0,0.0,1.0,0.0,0.0,0.0,0.0,1.0]}]}}]},"models":{"example:model":{}}}"#;
        entities
            .snapshot_payload(first, "minecraft:overworld", "player-a", [0.0; 3], 1)
            .unwrap();

        let keep = r#"{"dims":{"minecraft:overworld":[{"x":2.0,"y":2.0,"z":3.0,"id":7,"keep":true,"e":{"id":7,"type":"example:mob","x":2.0,"y":2.0,"z":3.0,"keep":true}}]}}"#;
        let payload = entities
            .snapshot_payload(keep, "minecraft:overworld", "player-b", [0.0; 3], 2)
            .unwrap();
        let payload: Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(payload["entities"][0]["models"][0]["key"], "example:model");
        assert!(payload["entities"][0].get("keep").is_none());
    }

    #[test]
    fn burning_java_entities_keep_their_fire_state_in_the_snapshot() {
        let raw: RawShown = serde_json::from_str(
            r#"{"id":9,"type":"example:ember_beast","x":1.0,"y":2.0,"z":3.0,"on_fire":true}"#,
        )
        .unwrap();
        let shown = raw.shown();
        assert!(shown.on_fire);
        assert_eq!(shown.kind, "example:ember_beast");
    }

    #[test]
    fn captured_model_light_is_preserved_for_rendering() {
        let model: RawModel = serde_json::from_str(
            r#"{"key":"example:model","tex":"example:textures/entity/mob.png","m":[1.0,0.0,0.0,0.0,0.0,1.0,0.0,0.0,0.0,0.0,1.0,0.0,0.0,0.0,0.0,1.0],"light":15728880}"#,
        )
        .unwrap();
        assert_eq!(model.drawn().unwrap().light, Some(0xF0F0));
        assert_eq!(vertex_light(0xF000F0), horizon_client::mobs::render::FULL_BRIGHT);
    }

    #[test]
    fn captured_custom_mesh_light_is_preserved_for_rendering() {
        let mesh: RawModel = serde_json::from_str(
            r#"{"tex":"example:textures/entity/mesh.png","v":[0.0,0.0,0.0,0.0,0.0,0.0,1.0,0.0,4294967295],"light":15728880}"#,
        )
        .unwrap();
        assert_eq!(mesh.mesh().unwrap().light, Some(0xF0F0));
    }

    #[test]
    fn custom_mesh_keeps_different_light_per_vertex() {
        let mesh: RawModel = serde_json::from_str(
            r#"{"tex":"example:textures/entity/mesh.png","v":[0.0,0.0,0.0,0.0,0.0,0.0,0.0,1.0,4294967295,1.0,0.0,0.0,1.0,0.0,0.0,0.0,1.0,4294967295],"vertex_lights":[15728880,655365]}"#,
        )
        .unwrap();
        let mesh = mesh.mesh().unwrap();
        assert_eq!(
            mesh.vertex_lights,
            Some(vec![Some(0xF0F0), Some(0x0A05)])
        );
    }

    #[test]
    fn disconnected_recipients_forget_entity_and_model_delivery_state() {
        let mut entities = JavaEntities::default();
        entities.known_entities.insert(1, HashSet::from([7]));
        entities.known_entities.insert(2, HashSet::from([8]));
        entities.known_trees.insert(1, HashSet::from(["one".to_owned()]));
        entities.known_trees.insert(2, HashSet::from(["two".to_owned()]));
        entities.retain_recipients(&HashSet::from([1]));
        assert!(entities.known_entities.contains_key(&1));
        assert!(!entities.known_entities.contains_key(&2));
        assert!(entities.known_trees.contains_key(&1));
        assert!(!entities.known_trees.contains_key(&2));
    }
}
