// A guest's key and shape.
fn liteminer_net(player: u128, channel: &str, data: &[u8]) {
    crate::game::liteminer::net_from_client(player, channel, data);
}
