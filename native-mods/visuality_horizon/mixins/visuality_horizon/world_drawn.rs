fn visuality_world_drawn(world: &mut WorldDrawn<'_>) {
    let mut bridged = horizon_client_render::render_slots::WorldDrawn {
        device: world.device,
        queue: world.queue,
        encoder: &mut *world.encoder,
        color: world.color,
        color_view: world.color_view,
        color_format: world.color_format,
        size: world.size,
        depth: world.depth,
        depth_size: world.depth_size,
        view_proj: world.view_proj,
        jittered: world.jittered,
        camera: world.camera,
        entities: world.entities,
        spans: world.spans,
        motion: world.motion,
        atlas: world.atlas,
        quad_indices: world.quad_indices,
        block_sprites: horizon_client_render::render_slots::BlockSpriteArrays {
            views: world.block_sprites.views,
            split: world.block_sprites.split,
            size: world.block_sprites.size,
            nearest: world.block_sprites.nearest,
            filtered: world.block_sprites.filtered,
        },
    };
    horizon_client_render::render_slots::visuality_horizon::world_drawn(&mut bridged);
}
