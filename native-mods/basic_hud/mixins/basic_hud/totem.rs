fn basic_hud_totem(
    game: &mut crate::game::Game<'_>,
    target: &horizon_client::particles::TrackingTarget,
    body: &Body,
) {
    use crate::game::{basic_hud as m, basic_hud_players as players};
    if !game.native_on(m::ID) || !game.native_window() {
        return;
    }
    let own = *target == horizon_client::particles::TrackingTarget::LocalPlayer;
    let name = if own {
        Some(game.own_profile_name())
    } else {
        players::at(game, body.bounds.feet)
    };
    if let Some(name) = name {
        m::pops(|p| p.add(&name));
    }
}
