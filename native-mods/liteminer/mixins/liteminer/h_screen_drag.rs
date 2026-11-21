// A slider held on the configuration screen.
fn liteminer_screen_drag(
    game: &crate::game::Game<'_>,
    screen: &horizon_client::starling_mod_api::NativeScreenView<'_>,
    canceled: &mut bool,
) {
    crate::game::liteminer::screen_drag(game, screen, canceled);
}
