fn basic_hud_pickup_remote_ingress(
    value: (),
    remote: &crate::game::network::remote::Remote,
    events: &mut std::collections::VecDeque<crate::game::network::raw_backlog::Received>,
) -> () {
    let start = crate::net::basic_hud_pickup_packets::take_backlog_start();
    for received in events.iter().skip(start) {
        if let crate::game::network::raw_backlog::Received::Event(
            crate::net::Event::Packet(packet),
            _,
        ) = received
        {
            crate::net::basic_hud_pickup_packets::observe_packet(
                &remote.session,
                packet.as_ref(),
            );
        }
    }
    value
}
