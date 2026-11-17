// /liteminer, /liteminer config, /liteminer shape set <index>.
fn liteminer_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    crate::game::liteminer::command(game, command, ran);
}
