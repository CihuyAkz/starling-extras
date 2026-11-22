use std::cell::RefCell;
use std::collections::VecDeque;

#[derive(Clone, Copy)]
struct Receipt { session: usize, item: i32, player: i32, amount: u32 }
thread_local! { static RECEIPTS: RefCell<VecDeque<Receipt>> = RefCell::new(VecDeque::new()); }

pub fn observe_packet(
    session: &crate::net::Session,
    packet: &horizon_protocol::packets::play::Clientbound,
) {
    if let horizon_protocol::packets::play::Clientbound::TakeItemEntity(p) = packet {
        if p.amount > 0 {
            RECEIPTS.with(|receipts| {
                let mut receipts = receipts.borrow_mut();
                if receipts.len() == 256 { receipts.pop_front(); }
                receipts.push_back(Receipt {
                    session: session as *const _ as usize,
                    item: p.item,
                    player: p.player,
                    amount: p.amount as u32,
                });
            });
        }
    }
}

pub fn take(session: &crate::net::Session, item: u32, player: i32) -> Option<u32> {
    RECEIPTS.with(|receipts| {
        let mut receipts = receipts.borrow_mut();
        let key = session as *const _ as usize;
        let index = receipts.iter().position(|r| r.session == key && r.item == item as i32 && r.player == player)?;
        receipts.remove(index).map(|receipt| receipt.amount)
    })
}

pub fn clear() { RECEIPTS.with(|receipts| receipts.borrow_mut().clear()); }
