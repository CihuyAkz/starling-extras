fn eatinganimation_embedded_content(
    root: &std::path::Path,
    sources: &ModsState,
) -> core::ops::ControlFlow<horizon_client::Result<horizon_assets::resource_source::ResourceRoot>> {
    static CONTENT: &[u8] = horizon_client::native_data!("eatinganimation", "eatinganimation-content.hmod");
    let mod_enabled = sources.content_ids.contains("eatinganimation");
    crate::item_render::set_official_eating_animation_enabled(mod_enabled);
    if !mod_enabled {
        return core::ops::ControlFlow::Continue(());
    }
    let dir = std::env::current_dir().unwrap_or_default().join(".horizon-cache");
    let file = dir.join("eatinganimation-embedded-content.hmod");
    if sources.dev.contains(&file) {
        return core::ops::ControlFlow::Continue(());
    }
    let same = std::fs::read(&file).is_ok_and(|on_disk| on_disk == CONTENT);
    if !same
        && std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(&file, CONTENT))
            .is_err()
    {
        log::warn!("Eating Animation: its frames could not be written to {}", file.display());
        return core::ops::ControlFlow::Continue(());
    }
    let mut with = sources.clone();
    with.dev.push(file);
    core::ops::ControlFlow::Break(content_root(root, &with))
}
