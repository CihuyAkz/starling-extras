fn basic_hud_pickup_remote_ingress_start(
    events: &mut std::collections::VecDeque<crate::game::network::raw_backlog::Received>,
) {
    crate::net::basic_hud_pickup_packets::mark_backlog_start(events.len());
}
