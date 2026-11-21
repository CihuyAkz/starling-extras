// The configuration screen.
fn liteminer_screen_draw(
    game: &crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
    screen: &horizon_client::starling_mod_api::NativeScreenView<'_>,
) {
    crate::game::liteminer::screen_draw(game, batch, screen);
}
