// A click on the configuration screen.
fn liteminer_screen_click(
    game: &crate::game::Game<'_>,
    screen: &horizon_client::mod_api::NativeScreenView<'_>,
    button: i32,
    canceled: &mut bool,
) {
    crate::game::liteminer::screen_click(game, screen, button, canceled);
}
