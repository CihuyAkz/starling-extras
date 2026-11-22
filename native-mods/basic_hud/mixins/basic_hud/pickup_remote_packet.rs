fn basic_hud_pickup_remote_packet(
    game: &mut crate::game::Game<'_>,
    packet: &horizon_protocol::packets::play::Clientbound,
) {
    if let Some(remote) = &game.remote {
        crate::net::basic_hud_pickup_packets::observe_packet(&remote.session, packet);
    }
}
