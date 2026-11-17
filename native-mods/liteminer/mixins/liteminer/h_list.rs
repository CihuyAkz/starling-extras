// The mod in Options > Mods, with its key.
fn liteminer_list(mods: &mut Vec<horizon_client::mod_api::NativeMod>) {
    crate::game::liteminer::listed_mod(mods);
}
