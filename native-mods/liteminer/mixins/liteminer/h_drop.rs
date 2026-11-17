// A vein block's drops pop at the block mined (drops together).
fn liteminer_drop(value: [i32; 3]) -> [i32; 3] {
    crate::game::liteminer::drop_block(value)
}
