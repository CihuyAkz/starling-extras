// The wheel changes the shape while the key is held.
fn liteminer_scroll(game: &mut crate::game::Game<'_>, scroll: [f64; 2], canceled: &mut bool) {
    crate::game::liteminer::scroll(game, scroll, canceled);
}
