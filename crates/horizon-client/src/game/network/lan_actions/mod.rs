//! What guests do in a hosted world. The host's gameplay code is written for
//! the one local player, so a guest's action runs through it with the guest's
//! state swapped in for the duration of the call (position, inventory, game
//! mode, experience, mining and item use): digging, placing, using items and
//! blocks, attacking and the commands all behave exactly as they do for the
//! host, and every world change they cause reaches all players through the
//! block log, the level events and the entity tracker.
use super::{
    Dim, Game, GuestUse, Hit, InteractTarget, Mining, Screen,
    building_players::{BuildingPlayer, BuildingPlayers, HubBodies, shared},
    lan::LanHost,
    lan_crafting::MenuRemoval,
    level_sounds::LevelSound,
    respawn_anchor::RespawnConfig,
};
use horizon_client::{
    containers::BlockEntity,
    entities::Experience,
    first_person::{Hand, SwingAnimation},
    items::ItemStack,
    lan::{
        guest::SPECTATOR,
        hub::TickCtx,
        menu::{Rules, Slots},
    },
    motion::{MotionEvent, MotionWorld, PlayerMotion},
    pose::Dimensions,
};
use horizon_core::{
    geometry::{Aabb, Vec3},
    random::RandomSource,
};
use horizon_protocol::{
    packets::play::{self, Clientbound as C, Serverbound as S},
    text::literal,
};
use horizon_world::entity_move::EntityMoveWorld;
use mod_api::{ActionRefused, GameMode, SoundSource};
use winit::keyboard::KeyCode;

#[path = "attack_checks.rs"] mod attack_checks;
#[path = "commands.rs"] mod commands;
#[path = "death_state.rs"] mod death_state;
#[path = "effects.rs"] mod effects;
#[path = "guest_body.rs"] mod guest_body;
#[path = "hub_click.rs"] mod hub_click;
#[path = "interaction.rs"] mod interaction;
#[path = "menus.rs"] mod menus;
#[path = "mining.rs"] mod mining;
#[path = "movement.rs"] mod movement;
#[path = "packets.rs"] mod packets;
#[path = "pickups.rs"] mod pickups;
#[path = "respawn.rs"] mod respawn;
#[path = "sleep.rs"] mod sleep;
#[path = "tick.rs"] mod tick;
#[path = "turns.rs"] mod turns;
include!("shared.rs");
