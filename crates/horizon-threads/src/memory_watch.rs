use std::{
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
    time::{Duration, Instant},
};

const MB: u64 = 1 << 20;
const STEP: u64 = 256 * MB;
const EVERY: Duration = Duration::from_millis(250);
const WARN_SHARE: f64 = 0.75;
const SLOTS: usize = 16;

static PEAKS: [AtomicU64; SLOTS] = [const { AtomicU64::new(0) }; SLOTS];
static STARTED: OnceLock<()> = OnceLock::new();

fn size(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
    } else {
        format!("{} MB", bytes / MB)
    }
}

fn change(from: u64, to: u64) -> String {
    if to >= from {
        format!("+{}", size(to - from))
    } else {
        format!("-{}", size(from - to))
    }
}

fn resident() -> u64 {
    crate::process_memory::resident_bytes().unwrap_or(0)
}

fn heap() -> u64 {
    crate::allocator_commit().0 as u64
}

fn allowed() -> Option<u64> {
    cgroup_limit().or_else(machine_ram)
}

#[cfg(target_os = "linux")]
fn cgroup_limit() -> Option<u64> {
    let own = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let path = own.lines().find_map(|l| l.strip_prefix("0::"))?;
    let max = std::fs::read_to_string(format!("/sys/fs/cgroup{path}/memory.max")).ok()?;
    max.trim().parse().ok()
}

#[cfg(not(target_os = "linux"))]
fn cgroup_limit() -> Option<u64> {
    None
}

#[cfg(target_os = "linux")]
fn meminfo(key: &str) -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|l| l.starts_with(key))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

#[cfg(target_os = "linux")]
fn machine_ram() -> Option<u64> {
    meminfo("MemTotal:")
}

#[cfg(not(target_os = "linux"))]
fn machine_ram() -> Option<u64> {
    crate::process_memory::usage().map(|[_, _, ram]| ram)
}

#[cfg(target_os = "linux")]
fn available() -> Option<u64> {
    meminfo("MemAvailable:")
}

#[cfg(not(target_os = "linux"))]
fn available() -> Option<u64> {
    None
}

fn holders() -> String {
    if !crate::HEAP_CENSUS {
        return String::from("(build with --features alloc-census to see who holds the heap)");
    }
    crate::heap_census::census()
        .into_iter()
        .filter(|(_, bytes)| *bytes > 0)
        .take(12)
        .map(|(tag, bytes)| format!("{tag} {}", size(bytes as u64)))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn start() {
    if std::env::var("HORIZON_MEMORY_WATCH").is_ok_and(|v| v.trim() == "0") {
        return;
    }
    STARTED.get_or_init(|| {
        let spawned = std::thread::Builder::new()
            .name("memory-watch".into())
            .spawn(watch);
        if let Err(e) = spawned {
            log::warn!("Horizon memory: the watcher did not start: {e}");
        }
    });
}

fn watch() {
    let mut allowed = allowed();
    let mut samples = 0u32;
    let started = Instant::now();
    let (mut logged_resident, mut logged_heap) = (resident(), heap());
    let mut logged_at = started;
    let mut peak = logged_resident;
    let mut warned_peak = 0;
    log::info!(
        "Horizon memory: watching (resident {}, heap {}, allowed {})",
        size(logged_resident),
        size(logged_heap),
        allowed.map_or_else(|| "unknown".into(), size),
    );
    loop {
        std::thread::sleep(EVERY);
        samples = samples.wrapping_add(1);
        if samples.is_multiple_of(8) {
            let now = self::allowed();
            if now != allowed {
                log::info!(
                    "Horizon memory: the game may now use {}",
                    now.map_or_else(|| "unknown".into(), size)
                );
                allowed = now;
            }
        }
        let (now_resident, now_heap) = (resident(), heap());
        for slot in &PEAKS {
            let mut seen = slot.load(Relaxed);
            while seen != 0 && now_resident > seen {
                match slot.compare_exchange_weak(seen, now_resident, Relaxed, Relaxed) {
                    Ok(_) => break,
                    Err(now) => seen = now,
                }
            }
        }
        peak = peak.max(now_resident);
        if now_resident.abs_diff(logged_resident) >= STEP || now_heap.abs_diff(logged_heap) >= STEP
        {
            log::info!(
                "Horizon memory: resident {} ({} in {:.1} s), heap {} ({}), peak {}{}",
                size(now_resident),
                change(logged_resident, now_resident),
                logged_at.elapsed().as_secs_f32(),
                size(now_heap),
                change(logged_heap, now_heap),
                size(peak),
                available().map_or_else(String::new, |a| format!(", RAM free {}", size(a))),
            );
            (logged_resident, logged_heap, logged_at) = (now_resident, now_heap, Instant::now());
        }
        if let Some(allowed) = allowed
            && now_resident as f64 >= allowed as f64 * WARN_SHARE
            && now_resident >= warned_peak + STEP
        {
            warned_peak = now_resident;
            log::warn!(
                "Horizon memory: {} of the {} the game may use; the heap: {}",
                size(now_resident),
                size(allowed),
                holders(),
            );
        }
    }
}

pub struct Phase {
    name: &'static str,
    slot: Option<usize>,
    started: Instant,
    resident: u64,
    heap: u64,
    _tag: Option<crate::heap_census::Tag>,
}

pub fn phase(name: &'static str) -> Phase {
    let resident = resident();
    let slot = PEAKS.iter().position(|s| {
        s.compare_exchange(0, resident.max(1), Relaxed, Relaxed)
            .is_ok()
    });
    Phase {
        name,
        slot,
        started: Instant::now(),
        resident,
        heap: heap(),
        _tag: crate::heap_tag(name),
    }
}

impl Drop for Phase {
    fn drop(&mut self) {
        let (resident, heap) = (resident(), heap());
        let peak = self
            .slot
            .map_or(resident, |s| PEAKS[s].swap(0, Relaxed).max(resident));
        log::info!(
            "Horizon memory: {} took {:.2} s: resident {} -> {} ({}, peak {}), heap {}",
            self.name,
            self.started.elapsed().as_secs_f32(),
            size(self.resident),
            size(resident),
            change(self.resident, resident),
            size(peak),
            change(self.heap, heap),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_as_megabytes_then_gigabytes() {
        assert_eq!(size(340 * MB), "340 MB");
        assert_eq!(size(3 << 29), "1.5 GB");
        assert_eq!(change(2 << 30, 1 << 30), "-1.0 GB");
        assert_eq!(change(MB, 3 * MB), "+2 MB");
    }

    #[test]
    fn a_phase_sees_the_peak_the_watcher_records_and_frees_its_slot() {
        let measured = phase("test");
        let slot = measured.slot.expect("a free slot");
        PEAKS[slot].fetch_max(u64::MAX / 2, Relaxed);
        assert_eq!(PEAKS[slot].load(Relaxed), u64::MAX / 2);
        drop(measured);
        assert_eq!(PEAKS[slot].load(Relaxed), 0, "the slot is free again");
    }
}
