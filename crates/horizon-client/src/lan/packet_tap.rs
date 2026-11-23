use std::sync::{
    Mutex, PoisonError,
    atomic::{AtomicU8, Ordering},
};

const OFF: u8 = 0;
const UNDECIDED: u8 = 1;
const ON: u8 = 2;
const MAX_PENDING_BYTES: usize = 64 << 20;

#[derive(Default)]
struct Kept {
    uuid: Option<u128>,
    bodies: Vec<Vec<u8>>,
    bytes: usize,
    overflowed: bool,
}

static STATE: AtomicU8 = AtomicU8::new(OFF);
static KEPT: Mutex<Kept> = Mutex::new(Kept {
    uuid: None,
    bodies: Vec::new(),
    bytes: 0,
    overflowed: false,
});

fn kept() -> std::sync::MutexGuard<'static, Kept> {
    KEPT.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn undecided() {
    let _ = STATE.compare_exchange(OFF, UNDECIDED, Ordering::SeqCst, Ordering::SeqCst);
}

pub fn want(on: bool) {
    STATE.store(if on { ON } else { OFF }, Ordering::SeqCst);
    if !on {
        *kept() = Kept::default();
    }
}

pub fn keeping() -> bool {
    STATE.load(Ordering::Relaxed) != OFF
}

pub fn begin(uuid: u128) {
    if keeping() {
        *kept() = Kept {
            uuid: Some(uuid),
            bodies: vec![Vec::new()],
            bytes: 0,
            overflowed: false,
        };
    }
}

pub fn keep(body: &[u8]) {
    let state = STATE.load(Ordering::Relaxed);
    if state == OFF {
        return;
    }
    let mut kept = kept();
    if kept.uuid.is_none() || kept.overflowed {
        return;
    }
    let Some(total) = kept.bytes.checked_add(body.len()) else {
        kept.bodies.clear();
        kept.bytes = 0;
        kept.overflowed = true;
        return;
    };
    if total > MAX_PENDING_BYTES {
        kept.bodies.clear();
        kept.bytes = 0;
        kept.overflowed = true;
        return;
    }
    kept.bytes = total;
    kept.bodies.push(body.to_vec());
}

pub fn take() -> Option<(u128, Vec<Vec<u8>>)> {
    if STATE.load(Ordering::Relaxed) != ON {
        return None;
    }
    let mut kept = kept();
    let uuid = kept.uuid?;
    if kept.overflowed {
        log::warn!("Java mods: packet recording batch exceeded 64 MiB and was dropped");
        kept.bytes = 0;
        kept.overflowed = false;
        kept.bodies.clear();
        return None;
    }
    if kept.bodies.is_empty() {
        return None;
    }
    kept.bytes = 0;
    Some((uuid, std::mem::take(&mut kept.bodies)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_packets_from_memory_login_only_after_request() {
        want(false);
        begin(7);
        keep(b"before");
        assert!(take().is_none());
        undecided();
        begin(7);
        keep(b"a");
        assert!(take().is_none());
        want(true);
        keep(b"b");
        assert_eq!(take(), Some((7, vec![Vec::new(), b"a".to_vec(), b"b".to_vec()])));
        assert!(take().is_none());
        begin(9);
        keep(b"c");
        assert_eq!(take(), Some((9, vec![Vec::new(), b"c".to_vec()])));
        want(false);
        keep(b"d");
        assert!(take().is_none());
    }
}

#[cfg(test)]
mod bounds_tests {
    use super::*;

    #[test]
    fn drops_an_oversized_pending_batch_without_growing_memory() {
        want(false);
        undecided();
        begin(11);
        {
            let mut batch = kept();
            batch.bytes = MAX_PENDING_BYTES;
        }
        keep(b"overflow");
        let kept = kept();
        assert!(kept.overflowed);
        assert!(kept.bodies.is_empty());
        drop(kept);
        want(false);
    }
}
