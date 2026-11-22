// A player breaks a block: with the key held, the rest of the shape goes too.
fn liteminer_destroy(game: &mut crate::game::Game<'_>, pos: [i32; 3], canceled: &mut bool) {
    crate::game::liteminer::destroy(game, pos, canceled);
}
