use super::PlayerSender;

impl PlayerSender {
    pub(super) fn tap(&self, body: &[u8]) {
        if self.tapped {
            super::super::packet_tap::keep(body);
        }
    }
}
