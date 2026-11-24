use crate::{
    biome::Biomes,
    chunk::{Chunk, LevelShape},
    streaming::{ChunkGenerator, Proto},
};
use horizon_world::{
    chunk_status::ChunkStatus,
    registry::BlockRegistry,
};
use std::{path::Path, sync::Arc};

pub(crate) struct JavaWorldgenTerrain {
    dimension: String,
    registry: &'static BlockRegistry,
    biomes: Arc<Biomes>,
    shape: LevelShape,
}

struct JavaWorldgenProto {
    dimension: String,
    x: i32,
    z: i32,
    token: Option<i64>,
    decorated: bool,
}

impl Drop for JavaWorldgenProto {
    fn drop(&mut self) {
        #[cfg(feature = "java-mods")]
        if let Some(token) = self.token.take() {
            let _ = horizon_jvm::events::worldgen_forget(
                &self.dimension,
                self.x,
                self.z,
                token,
            );
        }
    }
}

impl JavaWorldgenTerrain {
    fn empty(&self, x: i32, z: i32) -> Chunk {
        Chunk::with_layout(x, z, self.shape.column())
    }

    fn decode(&self, bytes: &[u8], x: i32, z: i32) -> Option<Chunk> {
        let (root, _) = horizon_core::nbt::read_named(bytes, horizon_core::nbt::Limits::saved())
            .ok()?;
        let mut chunk = crate::save::decode_with(
            &root,
            self.registry,
            &self.biomes,
            None,
            self.shape.column(),
        )?;
        chunk.x = x;
        chunk.z = z;
        Some(chunk)
    }
}

impl ChunkGenerator for JavaWorldgenTerrain {
    type Worker = ();

    fn worker(&self) -> Self::Worker {}

    fn proto(&self, _worker: &mut Self::Worker, x: i32, z: i32) -> Proto {
        #[cfg(feature = "java-mods")]
        let token = match horizon_jvm::events::worldgen_proto(&self.dimension, x, z) {
            Ok(token) => token,
            Err(error) => {
                log::warn!(
                    "Java mods: {} couldn't generate terrain for chunk {x} {z}: {error}",
                    self.dimension
                );
                None
            }
        };
        #[cfg(not(feature = "java-mods"))]
        let token = None;

        if token.is_none() {
            log::warn!(
                "Java mods: {} returned no terrain for chunk {x} {z}",
                self.dimension
            );
        }

        Box::new(JavaWorldgenProto {
            dimension: self.dimension.clone(),
            x,
            z,
            token,
            decorated: false,
        })
    }

    fn decorates(&self) -> bool {
        true
    }

    fn decorate(&self, _worker: &mut Self::Worker, region: &mut [Proto; 9]) {
        let Some(center) = region[4].downcast_mut::<JavaWorldgenProto>() else {
            return;
        };
        if center.token.is_none() {
            return;
        }
        #[cfg(feature = "java-mods")]
        match horizon_jvm::events::worldgen_decorate(&center.dimension, center.x, center.z) {
            Ok(()) => center.decorated = true,
            Err(error) => log::warn!(
                "Java mods: {} couldn't decorate chunk {} {}: {error}",
                center.dimension,
                center.x,
                center.z
            ),
        }
    }

    fn finish(&self, proto: Proto) -> Chunk {
        let Ok(mut proto) = proto.downcast::<JavaWorldgenProto>() else {
            return self.empty(0, 0);
        };
        let (x, z) = (proto.x, proto.z);
        #[cfg(feature = "java-mods")]
        if let Some(token) = proto.token.take() {
            match horizon_jvm::events::worldgen_finish(&proto.dimension, x, z, token) {
                Ok(Some(bytes)) => match self.decode(&bytes, x, z) {
                    Some(chunk) => return chunk,
                    None => log::warn!(
                        "Java mods: {} returned unreadable chunk data for {x} {z}",
                        proto.dimension
                    ),
                },
                Ok(None) => log::warn!(
                    "Java mods: {} returned no finished chunk for {x} {z}",
                    proto.dimension
                ),
                Err(error) => log::warn!(
                    "Java mods: {} couldn't finish chunk {x} {z}: {error}",
                    proto.dimension
                ),
            }
            let _ = horizon_jvm::events::worldgen_forget(&proto.dimension, x, z, token);
        }
        self.empty(x, z)
    }

    fn is_decorated(&self, proto: &Proto) -> bool {
        proto
            .downcast_ref::<JavaWorldgenProto>()
            .is_some_and(|proto| proto.decorated)
    }

    fn proto_status(&self, proto: &Proto) -> Option<ChunkStatus> {
        let proto = proto.downcast_ref::<JavaWorldgenProto>()?;
        Some(if proto.decorated {
            ChunkStatus::Features
        } else {
            ChunkStatus::Terrain
        })
    }

    fn light_view(&self, proto: &Proto) -> Option<Chunk> {
        let proto = proto.downcast_ref::<JavaWorldgenProto>()?;
        if !proto.decorated {
            return None;
        }
        #[cfg(feature = "java-mods")]
        {
            let token = proto.token?;
            match horizon_jvm::events::worldgen_snapshot(
                &proto.dimension,
                proto.x,
                proto.z,
                token,
            ) {
                Ok(Some(bytes)) => self.decode(&bytes, proto.x, proto.z),
                Ok(None) => None,
                Err(error) => {
                    log::warn!(
                        "Java mods: {} couldn't snapshot chunk {} {}: {error}",
                        proto.dimension,
                        proto.x,
                        proto.z
                    );
                    None
                }
            }
        }
        #[cfg(not(feature = "java-mods"))]
        {
            None
        }
    }
}

pub(crate) fn worldgen_terrain(
    dimension: &str,
    seed: i64,
    structures: bool,
    world: &Path,
    shape: LevelShape,
    registry: &'static BlockRegistry,
    biomes: Arc<Biomes>,
) -> Option<JavaWorldgenTerrain> {
    #[cfg(feature = "java-mods")]
    {
        if !crate::java_mods::compatibility_enabled() {
            return None;
        }
        crate::java_mods::wait_for_startup();
        if !horizon_jvm::events::ready() {
            return None;
        }
        match horizon_jvm::events::worldgen_start(dimension, seed, structures, world, &[]) {
            Ok(true) => Some(JavaWorldgenTerrain {
                dimension: dimension.to_owned(),
                registry,
                biomes,
                shape,
            }),
            Ok(false) => None,
            Err(error) => {
                log::warn!("Java mods: couldn't prepare world generation for {dimension}: {error}");
                None
            }
        }
    }
    #[cfg(not(feature = "java-mods"))]
    {
        let _ = (dimension, seed, structures, world, shape, registry, biomes);
        None
    }
}
