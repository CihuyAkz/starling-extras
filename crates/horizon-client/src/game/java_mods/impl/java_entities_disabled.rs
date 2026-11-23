use super::Game;

pub(super) fn used_past_entities(_: &str) -> bool {
    false
}

impl Game<'_> {
    pub(super) fn java_guest_payload(&self, _: u32, _: &str, _: &[u8]) -> bool {
        false
    }

    pub(super) fn java_menu_click(
        &self,
        _: u32,
        _: &horizon_protocol::packets::play::ContainerClick,
    ) -> bool {
        false
    }

    pub(super) fn java_menu_closed(&mut self, _: u32, _: i32) -> bool {
        false
    }
}

impl Game<'_> {
    pub(super) fn java_entity_shadows(&self, _: f64) -> Vec<(glam::DVec3, f32, f32)> {
        Vec::new()
    }

    pub(super) fn java_entity_names(
        &self,
        _: f64,
    ) -> Vec<(i32, glam::DVec3, f64, horizon_client::text::Text, bool)> {
        Vec::new()
    }

    pub(super) fn java_entity_hit(&self) -> Option<(i32, f64)> {
        None
    }

    pub(super) fn java_entity_is_nearest(&self, _: f64) -> bool {
        false
    }

    pub(super) fn attack_java_entity_local(&mut self, _: i32) -> bool {
        false
    }

    pub(super) fn remote_attack_java_entity(&mut self) -> bool {
        false
    }

    pub(super) fn java_entity_location(&self, _: i32, _: f64) -> [f64; 3] {
        [0.0; 3]
    }
}
