use super::Game;
use horizon_client::mobs::{Mob, MobType, Species};
use horizon_core::geometry::Vec3;
use std::{cell::RefCell, collections::HashMap, sync::Arc};

thread_local! {
    static KINDS: RefCell<HashMap<String, Option<Arc<MobType>>>> = RefCell::new(HashMap::new());
    static PROXIES: RefCell<HashMap<u32, (Mob, i64)>> = RefCell::new(HashMap::new());
}

pub(super) struct Proxy<'a> {
    pub entity: &'a str,
    pub id: u32,
    pub at: glam::DVec3,
    pub yaw: f32,
    pub head_yaw: f32,
    pub pitch: f32,
    pub base: Option<glam::Mat4>,
    pub packed: u32,
}

fn proxy_id(entity: &str, id: u32) -> u32 {
    let kind = entity.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    0x4000_0000 | ((id ^ (kind << 22)) & 0x3FFF_FFFF)
}

impl Game<'_> {
    pub(super) fn pack_draws(&self, entity: &str) -> bool {
        let bedrock = self
            .res
            .mob_data()
            .bedrock
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        !bedrock.is_empty() && bedrock.has_entity(entity)
    }

    pub(super) fn pack_projectile(
        &self,
        projectile: &horizon_client::projectiles::Projectile,
        at: [f64; 3],
        camera_relative: glam::Vec3,
        camera: glam::DVec3,
        alpha: f64,
        packed: u32,
        out: &mut Vec<horizon_client::mesher::Vertex>,
    ) -> Option<bool> {
        use horizon_client::projectiles::ProjectileKind;
        let alpha32 = alpha as f32;
        let yaw = projectile.previous_yaw
            + alpha32 * (projectile.yaw - projectile.previous_yaw);
        let pitch = projectile.previous_pitch
            + alpha32 * (projectile.pitch - projectile.previous_pitch);
        let trident = matches!(projectile.kind, ProjectileKind::Trident);
        let proxy = Proxy {
            entity: projectile.entity_type(),
            id: projectile.id,
            at: glam::DVec3::from_array(at),
            yaw: 0.0,
            head_yaw: yaw,
            pitch,
            base: trident.then(|| {
                glam::Mat4::from_translation(camera_relative)
                    * glam::Mat4::from_rotation_y((projectile.yaw - 90.0).to_radians())
                    * glam::Mat4::from_rotation_z((projectile.pitch + 90.0).to_radians())
            }),
            packed,
        };
        self.pack_proxy(&proxy, camera, alpha, |_| {}, out)
            .then(|| projectile.arrow.as_ref().is_some_and(|arrow| arrow.on_fire()))
    }

    pub(super) fn pack_cart(
        &self,
        entity: &str,
        id: u32,
        at: glam::DVec3,
        yaw_pitch: (f32, f32),
        base: glam::Mat4,
        packed: u32,
        setup: impl FnOnce(&mut Mob),
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<horizon_client::mesher::Vertex>,
    ) -> bool {
        let (yaw, pitch) = yaw_pitch;
        let proxy = Proxy {
            entity,
            id,
            at,
            yaw: 90.0 - yaw,
            head_yaw: 90.0 - yaw,
            pitch,
            base: Some(
                base * glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.25 - 1.501, 0.0)),
            ),
            packed,
        };
        self.pack_proxy(&proxy, camera, alpha, setup, out)
    }

    pub(super) fn pack_proxy(
        &self,
        proxy: &Proxy,
        camera: glam::DVec3,
        alpha: f64,
        setup: impl FnOnce(&mut Mob),
        out: &mut Vec<horizon_client::mesher::Vertex>,
    ) -> bool {
        if !self.pack_draws(proxy.entity) {
            return false;
        }
        let Some(kind) = KINDS.with(|kinds| {
            kinds
                .borrow_mut()
                .entry(proxy.entity.to_owned())
                .or_insert_with(|| {
                    let types = self.res.mob_types();
                    let base = types
                        .get("minecraft:mannequin")
                        .or_else(|| types.get("minecraft:zombie"))
                        .or_else(|| types.values().next())?;
                    let mut kind = (**base).clone();
                    kind.id = proxy.entity.to_owned();
                    kind.species = Species::Mannequin;
                    kind.eye_height = kind.height * 0.85;
                    Some(Arc::new(kind))
                })
                .clone()
        }) else {
            return false;
        };
        let now = self.world.game_time;
        let atlas = &self.gui.atlas;
        let lookup = |name: &str| atlas.sprites.get(name).map(|rect| [rect.u0, rect.v0, rect.u1, rect.v1]);
        let data = self.res.mob_data();
        PROXIES.with(|proxies| {
            let mut proxies = proxies.borrow_mut();
            if proxies.len() > 256 {
                proxies.retain(|_, (_, seen)| now - *seen < 200);
            }
            let at = Vec3::new(proxy.at.x, proxy.at.y, proxy.at.z);
            let id = proxy_id(proxy.entity, proxy.id);
            let (mob, seen) = proxies
                .entry(id)
                .or_insert_with(|| (Mob::new(id, kind.clone(), at, proxy.yaw), now));
            if !Arc::ptr_eq(&mob.kind, &kind) {
                *mob = Mob::new(id, kind.clone(), at, proxy.yaw);
            }
            *seen = now;
            mob.position = at;
            mob.previous = at;
            mob.yaw = proxy.yaw;
            mob.previous_yaw = proxy.yaw;
            mob.body_yaw = proxy.yaw;
            mob.previous_body_yaw = proxy.yaw;
            mob.head_yaw = proxy.head_yaw;
            mob.previous_head_yaw = proxy.head_yaw;
            mob.head_pitch = proxy.pitch;
            mob.previous_head_pitch = proxy.pitch;
            mob.age = now.max(0) as u32;
            mob.invisible = false;
            setup(mob);
            let drew = match proxy.base {
                Some(base) => horizon_client::mobs::render::render_pack_at(
                    mob,
                    alpha as f32,
                    base,
                    &lookup,
                    proxy.packed,
                    data,
                    out,
                ),
                None => horizon_client::mobs::render::render_pack_only(
                    mob,
                    alpha as f32,
                    camera,
                    &lookup,
                    proxy.packed,
                    data,
                    out,
                ),
            };
            if drew && (mob.extra.main_hand_id().is_some() || mob.extra.off_hand_id().is_some()) {
                self.held_item_geometry(mob, camera, alpha, proxy.packed, true, None, out);
            }
            drew
        })
    }
}
