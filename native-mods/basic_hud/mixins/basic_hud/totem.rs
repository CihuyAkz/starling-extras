// Entity event 35, in the window's game only: the single player server thread
// passes here too, for the same totem.
fn basic_hud_totem(
    game: &mut crate::game::Game<'_>,
    target: horizon_client::particles::TrackingTarget,
    body: &Body,
) {
    use crate::game::{basic_hud as m, basic_hud_players as players};
    if !game.native_on(m::ID) || !game.native_window() {
        return;
    }
    let name = if target == horizon_client::particles::TrackingTarget::LocalPlayer {
        Some(game.own_profile_name())
    } else {
        players::at(game, body.bounds.feet)
    };
    if let Some(name) = name {
        m::pops(|p| p.add(&name));
    }
}
