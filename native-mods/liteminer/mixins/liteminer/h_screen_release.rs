// The mouse let go on the configuration screen.
fn liteminer_screen_release(game: &crate::game::Game<'_>, canceled: &mut bool) {
    crate::game::liteminer::screen_release(game, canceled);
}
