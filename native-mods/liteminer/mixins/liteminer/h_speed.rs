// Increased harvesting time: the break speed over the selection.
fn liteminer_speed(value: f32, game: &mut crate::game::Game<'_>) -> f32 {
    crate::game::liteminer::break_speed(game, value)
}
