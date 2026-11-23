use horizon_world::registry::BlockStateId;

static DRAWN_BY_BLOCK_ENTITY: std::sync::LazyLock<
    std::sync::RwLock<std::sync::Arc<Vec<bool>>>,
> = std::sync::LazyLock::new(Default::default);

pub(super) fn snapshot() -> std::sync::Arc<Vec<bool>> {
    DRAWN_BY_BLOCK_ENTITY
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

pub fn hide_model_drawn_by_block_entity(state: BlockStateId) -> bool {
    let index = state.0 as usize;
    let mut set = DRAWN_BY_BLOCK_ENTITY
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if set.get(index).copied().unwrap_or(false) {
        return false;
    }
    let mut next = (**set).clone();
    if next.len() <= index {
        next.resize(index + 1, false);
    }
    next[index] = true;
    *set = std::sync::Arc::new(next);
    true
}
