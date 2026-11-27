use super::*;

#[derive(Clone, Debug)]
pub struct CraftedTake {
    pub recipe: String,
    pub taken: ItemStack,
    pub width: usize,
    pub grid: Vec<Option<ItemStack>>,
}

impl CraftedTake {
    pub(super) fn of(
        id: &str,
        taken: &ItemStack,
        width: usize,
        grid: &[Option<ItemStack>],
    ) -> Self {
        Self {
            recipe: if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") },
            taken: taken.clone(),
            width,
            grid: grid.to_vec(),
        }
    }
}
