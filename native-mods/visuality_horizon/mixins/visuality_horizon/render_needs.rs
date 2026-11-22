fn visuality_render_needs(needs: &mut RenderNeeds) {
    let mut bridged = horizon_client_render::render_slots::RenderNeeds {
        entity_spans: needs.entity_spans,
        world_depth: needs.world_depth,
    };
    horizon_client_render::render_slots::visuality_horizon::render_needs(&mut bridged);
    needs.entity_spans = bridged.entity_spans;
    needs.world_depth = bridged.world_depth;
}
