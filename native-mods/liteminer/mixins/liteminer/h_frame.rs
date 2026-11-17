// Once a frame: the key state, the host told, the selection.
fn liteminer_frame(game: &mut crate::game::Game<'_>) {
    crate::game::liteminer::frame(game);
}
