// LiteMiner for Horizon: vein mining, a port of Liteminer by Kaf (MIT, Copyright (c) 2024
// Kaf, https://github.com/iamkaf/liteminer). Hold ` (grave) and break a block: the blocks
// of the selected shape go with it. Scroll while holding to change the shape.
//
// A child module of `game`, so it reads the game's own state and calls its own break and
// use paths: drops, experience, tool wear, statistics and ice melting are the game's.

use super::Game;
use horizon_client::hud::{HudBatch, rgba};
use horizon_client::items::ItemStack;
use horizon_client::starling_mod_api::{NativeBlockRim, NativeMod, NativeScreenView};
use horizon_client::text::Text;
use horizon_world::registry::BlockStateId;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

pub(crate) const ID: &str = "liteminer";
const NAME: &str = "LiteMiner";
const VERSION: &str = "1.0.0";
const DESCRIPTION: &str = "Vein mining, ported from Liteminer by Kaf. Hold ` (grave) and break a block: the whole vein, tunnel, staircase or 3x3 goes with it. Scroll while holding to change the shape. /liteminer opens the configuration.";
const KEY: (&str, &str, &str) = ("veinmine", "Vein Mine", "key.keyboard.grave.accent");
/// The channel a guest's client tells the host its key and shape on.
const CHANNEL: &str = "liteminer:state";
/// `LiteminerClient.PACKET_DELAY`: the key state changes at most this often.
const PACKET_DELAY: Duration = Duration::from_millis(125);

type Pos = [i32; 3];

// ---------------------------------------------------------------- configuration

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum DropMode {
    Together,
    EachBlock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum KeyMode {
    Hold,
    Toggle,
}

/// Liteminer's two configuration files in one: the common (gameplay) settings, used by the
/// world you host, and the client's own. The four lists stand in for Liteminer's tags
/// (`liteminer:excluded_blocks` and the rest), which data packs can still fill.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Config {
    pub prevent_tool_breaking: bool,
    pub require_correct_tool_enabled: bool,
    pub block_break_limit: u32,
    pub drop_mode: DropMode,
    pub harvest_time_per_block_modifier_enabled: bool,
    pub harvest_time_per_block_modifier: f32,
    pub food_exhaustion_enabled: bool,
    pub food_exhaustion: f32,
    pub allow_vein_mining_at_zero_hunger: bool,
    pub distinguish_grown_crops: bool,
    pub match_deepslate_ore_variants: bool,
    pub excluded_blocks: Vec<String>,
    pub block_whitelist: Vec<String>,
    pub excluded_tools: Vec<String>,
    pub included_tools: Vec<String>,
    pub key_mode: KeyMode,
    pub show_hud: bool,
    pub hud_scale: f32,
    pub show_highlights: bool,
    pub highlight_foreground_line_color: u32,
    pub highlight_see_through_line_color: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            prevent_tool_breaking: true,
            require_correct_tool_enabled: false,
            block_break_limit: 64,
            drop_mode: DropMode::Together,
            harvest_time_per_block_modifier_enabled: false,
            harvest_time_per_block_modifier: 2.0,
            food_exhaustion_enabled: true,
            food_exhaustion: 0.2,
            allow_vein_mining_at_zero_hunger: true,
            distinguish_grown_crops: true,
            match_deepslate_ore_variants: true,
            excluded_blocks: vec!["minecraft:bedrock".into()],
            block_whitelist: Vec::new(),
            excluded_tools: Vec::new(),
            included_tools: Vec::new(),
            key_mode: KeyMode::Hold,
            show_hud: true,
            hud_scale: 1.0,
            show_highlights: true,
            highlight_foreground_line_color: 0xF9FFFE,
            highlight_see_through_line_color: 0x169C9C,
        }
    }
}

impl Config {
    /// Liteminer's ranges.
    fn sanitized(mut self) -> Self {
        self.block_break_limit = self.block_break_limit.clamp(1, 2048);
        self.harvest_time_per_block_modifier = finite(self.harvest_time_per_block_modifier, 2.0).clamp(1.0, 10.0);
        self.food_exhaustion = finite(self.food_exhaustion, 0.2).clamp(0.0, 1.0);
        self.hud_scale = finite(self.hud_scale, 1.0).clamp(0.5, 2.0);
        self.highlight_foreground_line_color &= 0xFF_FFFF;
        self.highlight_see_through_line_color &= 0xFF_FFFF;
        self
    }
    fn reset_common(&mut self) {
        let d = Config::default();
        let client = (
            self.key_mode,
            self.show_hud,
            self.hud_scale,
            self.show_highlights,
            self.highlight_foreground_line_color,
            self.highlight_see_through_line_color,
        );
        *self = d;
        (
            self.key_mode,
            self.show_hud,
            self.hud_scale,
            self.show_highlights,
            self.highlight_foreground_line_color,
            self.highlight_see_through_line_color,
        ) = client;
    }
    fn reset_client(&mut self) {
        let d = Config::default();
        self.key_mode = d.key_mode;
        self.show_hud = d.show_hud;
        self.hud_scale = d.hud_scale;
        self.show_highlights = d.show_highlights;
        self.highlight_foreground_line_color = d.highlight_foreground_line_color;
        self.highlight_see_through_line_color = d.highlight_see_through_line_color;
    }
}

fn finite(v: f32, d: f32) -> f32 {
    if v.is_finite() { v } else { d }
}

/// The 16 wool colours Liteminer's colour options take (`lineColorToHex`).
pub(crate) const WOOL: [(&str, u32); 16] = [
    ("White", 0xF9FFFE),
    ("Orange", 0xF9801D),
    ("Magenta", 0xC74EBD),
    ("Light Blue", 0x3AB3DA),
    ("Yellow", 0xFED83D),
    ("Lime", 0x80C71F),
    ("Pink", 0xF38BAA),
    ("Gray", 0x474F52),
    ("Light Gray", 0x9D9D97),
    ("Cyan", 0x169C9C),
    ("Purple", 0x8932B8),
    ("Blue", 0x3C44AA),
    ("Brown", 0x835432),
    ("Green", 0x5E7C16),
    ("Red", 0xB02E26),
    ("Black", 0x1D1D21),
];

fn wool_name(c: u32) -> String {
    WOOL.iter().find(|w| w.1 == c).map_or_else(|| format!("#{c:06X}"), |w| w.0.to_owned())
}
fn wool_step(c: u32, back: bool) -> u32 {
    let i = WOOL.iter().position(|w| w.1 == c).unwrap_or(0);
    let n = WOOL.len();
    WOOL[if back { (i + n - 1) % n } else { (i + 1) % n }].1
}

struct Store {
    loaded: bool,
    cfg: Option<Arc<Config>>,
    dir: Option<PathBuf>,
    stamp: Option<SystemTime>,
    checked: Option<Instant>,
}

static STORE: Mutex<Store> = Mutex::new(Store { loaded: false, cfg: None, dir: None, stamp: None, checked: None });

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn read(dir: &std::path::Path) -> (Config, Option<SystemTime>) {
    let path = dir.join("liteminer.json");
    let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let cfg = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<Config>(&t).ok())
        .unwrap_or_default()
        .sanitized();
    (cfg, stamp)
}

/// Loads the configuration once, and again when its file changes (a server's owner edits
/// it), looked at every two seconds at most.
fn ensure_loaded(game: &Game<'_>) {
    let mut s = lock(&STORE);
    if !s.loaded {
        let dir = game.native_data_dir(ID);
        let (cfg, stamp) = read(&dir);
        s.cfg = Some(Arc::new(cfg));
        s.stamp = stamp;
        s.dir = Some(dir);
        s.loaded = true;
        s.checked = Some(Instant::now());
        return;
    }
    if s.checked.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
        return;
    }
    s.checked = Some(Instant::now());
    let Some(dir) = s.dir.clone() else { return };
    let stamp = std::fs::metadata(dir.join("liteminer.json")).and_then(|m| m.modified()).ok();
    if stamp.is_some() && stamp != s.stamp {
        let (cfg, stamp) = read(&dir);
        s.cfg = Some(Arc::new(cfg));
        s.stamp = stamp;
    }
}

pub(crate) fn cfg() -> Arc<Config> {
    lock(&STORE).cfg.clone().unwrap_or_default()
}

fn edit(f: impl FnOnce(&mut Config)) {
    let mut s = lock(&STORE);
    let mut c = s.cfg.as_deref().cloned().unwrap_or_default();
    f(&mut c);
    let c = Arc::new(c.sanitized());
    s.cfg = Some(c.clone());
    let Some(dir) = s.dir.clone() else { return };
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(text) = serde_json::to_string_pretty(&*c) {
        let tmp = dir.join("liteminer.json.tmp");
        if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, dir.join("liteminer.json")).is_ok() {
            s.stamp = std::fs::metadata(dir.join("liteminer.json")).and_then(|m| m.modified()).ok();
        }
    }
}

// ---------------------------------------------------------------- shapes

/// `LiteminerShapes`, in Liteminer's order.
pub(crate) const SHAPES: [&str; 5] = ["Shapeless", "Small Tunnel", "Staircase Up", "Staircase Down", "3x3"];

pub(crate) fn shape_index(i: i64) -> usize {
    i.rem_euclid(SHAPES.len() as i64) as usize
}

/// What the walkers know of a block.
#[derive(Clone, Debug)]
pub(crate) struct Blk {
    pub name: Arc<str>,
    pub air: bool,
    pub hardness: f32,
    /// `FlowerBlock`, `TallGrassBlock` or `DoublePlantBlock`.
    pub flora: bool,
    /// For a block with an `age` or `stage`: whether it is at its last.
    pub grown: Option<bool>,
}

/// The world as the walkers see it, for one player and one tool.
pub(crate) trait Probe {
    fn blk(&self, p: Pos) -> Blk;
    /// `TagHelper.isExcludedBlock`.
    fn excluded(&self, b: &Blk) -> bool;
    /// `VeinmineChecks.shouldMine`.
    fn should_mine(&self, p: Pos) -> bool;
}

/// A set that keeps the order blocks went in.
#[derive(Default, Debug)]
pub(crate) struct Sel {
    pub list: Vec<Pos>,
    set: HashSet<Pos>,
}
impl Sel {
    fn with(origin: Pos) -> Self {
        let mut s = Self::default();
        s.add(origin);
        s
    }
    fn add(&mut self, p: Pos) -> bool {
        let new = self.set.insert(p);
        if new {
            self.list.push(p);
        }
        new
    }
    pub fn len(&self) -> usize {
        self.list.len()
    }
}

fn add(a: Pos, b: Pos) -> Pos {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub(crate) fn manhattan(a: Pos, b: Pos) -> i32 {
    (a[0] - b[0]).abs() + (a[1] - b[1]).abs() + (a[2] - b[2]).abs()
}
const UP: Pos = [0, 1, 0];
const DOWN: Pos = [0, -1, 0];

/// `ShapelessWalker.NEIGHBOR_OFFSETS`.
const NEIGHBORS: [Pos; 26] = [
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [1, 0, 0],
    [-1, 0, 0],
    [1, 0, -1],
    [-1, 0, -1],
    [1, 0, 1],
    [-1, 0, 1],
    [0, 1, -1],
    [0, 1, 1],
    [1, 1, 0],
    [-1, 1, 0],
    [1, 1, -1],
    [-1, 1, -1],
    [1, 1, 1],
    [-1, 1, 1],
    [0, -1, -1],
    [0, -1, 1],
    [1, -1, 0],
    [-1, -1, 0],
    [1, -1, -1],
    [-1, -1, -1],
    [1, -1, 1],
    [-1, -1, 1],
];

/// `BlockFamily.BLOCK_MATCHES`.
const FAMILIES: [&[&str]; 8] = [
    &["minecraft:raw_iron_block", "minecraft:iron_ore", "minecraft:deepslate_iron_ore"],
    &["minecraft:raw_gold_block", "minecraft:gold_ore", "minecraft:deepslate_gold_ore"],
    &["minecraft:raw_copper_block", "minecraft:copper_ore", "minecraft:deepslate_copper_ore"],
    &["minecraft:coal_ore", "minecraft:deepslate_coal_ore"],
    &["minecraft:diamond_ore", "minecraft:deepslate_diamond_ore"],
    &["minecraft:emerald_ore", "minecraft:deepslate_emerald_ore"],
    &["minecraft:lapis_ore", "minecraft:deepslate_lapis_ore"],
    &["minecraft:redstone_ore", "minecraft:deepslate_redstone_ore"],
];

fn path(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, p)| p)
}

/// `BlockFamily.matchesDeepslateOreVariant`: `<name>_ore` and `deepslate_<name>_ore`.
fn deepslate_variant(from: &str, to: &str) -> bool {
    let (f, t) = (path(from), path(to));
    if !f.ends_with("_ore") || !t.ends_with("_ore") {
        return false;
    }
    let base = |s: &'_ str| s.strip_prefix("deepslate_").map_or(s.to_owned(), str::to_owned);
    base(f) == base(t) && f.starts_with("deepslate_") != t.starts_with("deepslate_")
}

/// `BlockFamily.matches(Block, Block)`.
fn block_matches(cfg: &Config, from: &Blk, to: &Blk) -> bool {
    if to.name == from.name {
        return true;
    }
    if cfg.match_deepslate_ore_variants && deepslate_variant(&from.name, &to.name) {
        return true;
    }
    if let Some(family) = FAMILIES.iter().find(|f| f.contains(&&*to.name)) {
        return family.contains(&&*from.name);
    }
    from.flora && to.flora
}

/// `BlockFamily.matches(BlockState, BlockState)`: grown crops apart from growing ones.
pub(crate) fn state_matches(cfg: &Config, from: &Blk, to: &Blk) -> bool {
    if !block_matches(cfg, from, to) {
        return false;
    }
    if !cfg.distinguish_grown_crops || from.name != to.name {
        return true;
    }
    match (from.grown, to.grown) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

/// `ShapelessWalker`: the connected blocks that match the first, through all 26 neighbours,
/// nearest first, depth first as Liteminer's recursion goes.
fn shapeless(probe: &dyn Probe, cfg: &Config, origin: Pos) -> Sel {
    let first = probe.blk(origin);
    if first.air {
        return Sel::default();
    }
    let mut sel = Sel::with(origin);
    if probe.excluded(&first) {
        return sel;
    }
    let limit = cfg.block_break_limit as usize;
    let mut visited = PosSet::default();
    // Positions turned down once are turned down every time: kept so they're asked once.
    let mut refused = PosSet::default();
    let mut stack = vec![origin];
    while let Some(p) = stack.pop() {
        if visited.len() >= limit {
            break;
        }
        if visited.contains(&p) || refused.contains(&p) {
            continue;
        }
        if !state_matches(cfg, &first, &probe.blk(p)) || !probe.should_mine(p) {
            refused.insert(p);
            continue;
        }
        sel.add(p);
        visited.insert(p);
        let mut next = NEIGHBORS.map(|d| add(p, d));
        next.sort_by_key(|n| manhattan(*n, origin));
        stack.extend(next.iter().rev().filter(|n| !visited.contains(*n) && !refused.contains(*n)));
    }
    sel
}

/// `TunnelWalker`: straight on, into the face mined, while the blocks may be mined.
fn tunnel(probe: &dyn Probe, cfg: &Config, origin: Pos, dir: Pos) -> Sel {
    let first = probe.blk(origin);
    if first.air {
        return Sel::default();
    }
    let mut sel = Sel::with(origin);
    if probe.excluded(&first) {
        return sel;
    }
    let limit = cfg.block_break_limit as usize;
    let mut cursor = origin;
    for _ in 0..=limit {
        if sel.len() >= limit || !probe.should_mine(cursor) {
            break;
        }
        sel.add(cursor);
        cursor = add(cursor, dir);
    }
    sel
}

/// `StaircaseUpWalker` and `StaircaseDownWalker`: three blocks high, a step up (or down)
/// for each block on.
fn staircase(probe: &dyn Probe, cfg: &Config, origin: Pos, dir: Pos, up: bool) -> Sel {
    let first = probe.blk(origin);
    if first.air {
        return Sel::default();
    }
    let mut sel = Sel::with(origin);
    if probe.excluded(&first) {
        return sel;
    }
    let limit = cfg.block_break_limit as usize;
    let mut cursor = origin;
    for _ in 0..=limit {
        if sel.len() >= limit {
            break;
        }
        let below = add(cursor, DOWN);
        let third = if up { add(cursor, UP) } else { add(below, DOWN) };
        let (m_third, m_cursor, m_below) =
            (probe.should_mine(third), probe.should_mine(cursor), probe.should_mine(below));
        if !m_third && !m_cursor && !m_below {
            break;
        }
        for (ok, p) in [(m_cursor, cursor), (m_below, below), (m_third, third)] {
            if ok && sel.len() < limit {
                sel.add(p);
            }
        }
        cursor = add(add(cursor, dir), if up { UP } else { DOWN });
    }
    sel
}

/// `Direction.getCounterClockWise` and `getClockWise`, about the vertical.
fn ccw(d: Pos) -> Pos {
    [d[2], 0, -d[0]]
}
fn cw(d: Pos) -> Pos {
    [-d[2], 0, d[0]]
}

/// `ThreeByThreeWalker`: the nine blocks across the face mined.
fn three_by_three(probe: &dyn Probe, cfg: &Config, origin: Pos, dir: Pos) -> Sel {
    let first = probe.blk(origin);
    if first.air {
        return Sel::default();
    }
    let mut sel = Sel::with(origin);
    if probe.excluded(&first) || !probe.should_mine(origin) {
        return sel;
    }
    let p = origin;
    let positions: [Pos; 9] = if dir[1] != 0 {
        let (n, s, e, w) = ([0, 0, -1], [0, 0, 1], [1, 0, 0], [-1, 0, 0]);
        [
            add(p, n),
            add(add(p, n), e),
            add(add(p, n), w),
            add(p, e),
            p,
            add(p, w),
            add(p, s),
            add(add(p, s), e),
            add(add(p, s), w),
        ]
    } else {
        let (l, r) = (ccw(dir), cw(dir));
        [
            add(add(p, l), UP),
            add(p, UP),
            add(add(p, r), UP),
            add(p, l),
            p,
            add(p, r),
            add(add(p, l), DOWN),
            add(p, DOWN),
            add(add(p, r), DOWN),
        ]
    };
    let limit = cfg.block_break_limit as usize;
    for q in positions {
        if sel.len() >= limit {
            break;
        }
        if probe.should_mine(q) {
            sel.add(q);
        }
    }
    sel
}

/// `Direction.fromYRot`: the way a player at `yaw` faces.
pub(crate) fn facing(yaw: f32) -> Pos {
    match ((yaw / 90.0 + 0.5).floor() as i64).rem_euclid(4) {
        0 => [0, 0, 1],
        1 => [-1, 0, 0],
        2 => [0, 0, -1],
        _ => [1, 0, 0],
    }
}

/// The blocks shape `shape` takes from `origin`, mined through `face` (its normal, toward
/// the player) by a player facing `yaw`: the shape's walk, nearest first. The geometric
/// shapes don't grow from blocks that break at once (`withoutInstantBreakExpansion`).
pub(crate) fn walk(probe: &dyn Probe, cfg: &Config, shape: usize, origin: Pos, face: Pos, yaw: f32) -> Vec<Pos> {
    let dir = [-face[0], -face[1], -face[2]];
    let sel = if shape == 0 {
        shapeless(probe, cfg, origin)
    } else {
        let first = probe.blk(origin);
        if first.air || first.hardness > 0.0 {
            let stair = if dir[1] != 0 { facing(yaw) } else { dir };
            match shape {
                1 => tunnel(probe, cfg, origin, dir),
                2 => staircase(probe, cfg, origin, stair, true),
                3 => staircase(probe, cfg, origin, stair, false),
                _ => three_by_three(probe, cfg, origin, dir),
            }
        } else {
            Sel::with(origin)
        }
    };
    let mut list = sel.list;
    list.sort_by_key(|p| manhattan(*p, origin));
    list
}

/// `Liteminer.getScaledBreakSpeedModifier`.
pub(crate) fn speed_factor(cfg: &Config, count: usize) -> f32 {
    if count <= 1 {
        return 1.0;
    }
    1.0 / (1.0 + (count - 1) as f32 * cfg.harvest_time_per_block_modifier * 0.1)
}

// ---------------------------------------------------------------- the game as the walkers see it

/// The most `age` or `stage` each block takes, by name and property.
type GrowthMax = HashMap<(Arc<str>, &'static str), Option<i32>>;
thread_local! {
    static GROWTH: RefCell<GrowthMax> = RefCell::new(HashMap::new());
}

fn listed(list: &[String], tags: &HashMap<String, HashSet<String>>, name: &str) -> bool {
    list.iter().any(|e| match e.strip_prefix('#') {
        Some(tag) => tags.get(tag).is_some_and(|t| t.contains(name)),
        None => e == name,
    })
}

/// FxHash, for the walkers' block sets: positions are small and trusted.
#[derive(Default, Clone, Copy)]
pub(crate) struct Fx(u64);
impl std::hash::Hasher for Fx {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.write_u64(u64::from(*b));
        }
    }
    fn write_u32(&mut self, v: u32) {
        self.write_u64(u64::from(v));
    }
    fn write_i32(&mut self, v: i32) {
        self.write_u64(v as u32 as u64);
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }
}
pub(crate) type FxBuild = std::hash::BuildHasherDefault<Fx>;
pub(crate) type PosSet = HashSet<Pos, FxBuild>;

/// The tags one look reads, found once.
struct Tags<'a> {
    excluded_blocks: [Option<&'a HashSet<String>>; 2],
    whitelist: [Option<&'a HashSet<String>>; 2],
    excluded_tools: [Option<&'a HashSet<String>>; 2],
    included_tools: [Option<&'a HashSet<String>>; 2],
}
fn has(sets: &[Option<&HashSet<String>>; 2], name: &str) -> bool {
    sets.iter().flatten().any(|s| s.contains(name))
}

/// The world as one player with one tool sees it. What a block state is (its `Blk`, and
/// whether it may be mined, but for where it is) is worked out once a state.
pub(crate) struct Look<'a, 'g> {
    pub game: &'a Game<'g>,
    pub cfg: &'a Config,
    pub tool: Option<&'a ItemStack>,
    pub creative: bool,
    tags: Tags<'a>,
    whitelist_on: bool,
    tool_ok: bool,
    states: RefCell<HashMap<u32, (Blk, bool), FxBuild>>,
}

impl<'a, 'g> Look<'a, 'g> {
    pub fn new(game: &'a Game<'g>, cfg: &'a Config, tool: Option<&'a ItemStack>) -> Self {
        let items = game.res.items();
        let (b, i) = (&items.block_tags, &items.item_tags);
        let tags = Tags {
            excluded_blocks: [b.get("liteminer:excluded_blocks"), b.get("ftbultimine:excluded_blocks")],
            whitelist: [b.get("liteminer:block_whitelist"), b.get("ftbultimine:block_whitelist")],
            excluded_tools: [i.get("liteminer:excluded_tools"), i.get("ftbultimine:excluded_tools")],
            included_tools: [i.get("liteminer:included_tools"), i.get("ftbultimine:included_tools")],
        };
        let whitelist_on = tags.whitelist.iter().flatten().any(|s| !s.is_empty()) || !cfg.block_whitelist.is_empty();
        let mut look = Look {
            game,
            cfg,
            tool,
            creative: game.creative,
            tags,
            whitelist_on,
            tool_ok: true,
            states: RefCell::new(HashMap::default()),
        };
        look.tool_ok = !look.excluded_tool();
        look
    }
    fn growth(&self, state: BlockStateId, name: &Arc<str>) -> Option<bool> {
        let registry = &self.game.pack.assets.registry;
        let props = &registry.state(state)?.properties;
        let (key, value) =
            ["age", "stage"].into_iter().find_map(|k| props.get(k).map(|v| (k, v.parse::<i32>().ok())))?;
        let value = value?;
        let max = GROWTH.with(|g| {
            *g.borrow_mut().entry((name.clone(), key)).or_insert_with(|| {
                registry.property_values(name, key).iter().filter_map(|v| v.parse::<i32>().ok()).max()
            })
        })?;
        Some(value >= max)
    }
    /// `TagHelper.isExcludedTool`.
    pub fn excluded_tool(&self) -> bool {
        let Some(tool) = self.tool else { return false };
        has(&self.tags.excluded_tools, &tool.item)
            || listed(&self.cfg.excluded_tools, &self.game.res.items().item_tags, &tool.item)
    }
    fn included_tool(&self) -> bool {
        let Some(tool) = self.tool else { return false };
        has(&self.tags.included_tools, &tool.item)
            || listed(&self.cfg.included_tools, &self.game.res.items().item_tags, &tool.item)
    }
    fn excluded_name(&self, name: &str) -> bool {
        has(&self.tags.excluded_blocks, name)
            || listed(&self.cfg.excluded_blocks, &self.game.res.items().block_tags, name)
    }
    /// The state's `Blk`, and `shouldMine` but for `mayInteract`.
    fn state(&self, state: BlockStateId) -> (Blk, bool) {
        if let Some(v) = self.states.borrow().get(&state.0) {
            return v.clone();
        }
        let f = self.game.res.facts().get(state);
        let name = f.name.clone();
        let blk = Blk {
            air: name.ends_with(":air"),
            hardness: f.hardness,
            flora: f.has_class("FlowerBlock") || f.has_class("TallGrassBlock") || f.has_class("DoublePlantBlock"),
            grown: if self.cfg.distinguish_grown_crops { self.growth(state, &name) } else { None },
            name,
        };
        let n = &*blk.name;
        let mine = !(blk.air || f.has_class("LiquidBlock") || n == "minecraft:water" || n == "minecraft:lava")
            && !(self.whitelist_on && !(has(&self.tags.whitelist, n) || listed(&self.cfg.block_whitelist, &self.game.res.items().block_tags, n)))
            && !(f.hardness < 0.0 && !self.creative)
            // `isValidTool`.
            && self.tool_ok
            && (!self.cfg.require_correct_tool_enabled
                || !f.requires_tool
                || (self.tool.is_some() && (self.game.res.items().correct_for_drops(self.tool, n) || self.included_tool())))
            && !self.excluded_name(n);
        self.states.borrow_mut().insert(state.0, (blk.clone(), mine));
        (blk, mine)
    }
}

impl Probe for Look<'_, '_> {
    fn blk(&self, p: Pos) -> Blk {
        self.state(self.game.world.block(p[0], p[1], p[2])).0
    }
    fn excluded(&self, b: &Blk) -> bool {
        self.excluded_name(&b.name)
    }
    fn should_mine(&self, p: Pos) -> bool {
        // `Level.mayInteract` is the one check that is the place's, not the state's.
        self.state(self.game.world.block(p[0], p[1], p[2])).1 && !self.game.block_action_restricted(p)
    }
}

// ---------------------------------------------------------------- who is vein mining

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PlayerState {
    pub active: bool,
    pub shape: usize,
}

/// The local player's state, which the world this process hosts reads for its host.
static HOST: Mutex<PlayerState> = Mutex::new(PlayerState { active: false, shape: 0 });
/// Guests' states, as their clients sent them.
static GUESTS: Mutex<Option<HashMap<u128, PlayerState>>> = Mutex::new(None);

fn acting_state(game: &Game<'_>) -> PlayerState {
    match game.lan_acting_uuid {
        Some(uuid) => lock(&GUESTS).as_ref().and_then(|m| m.get(&uuid).copied()).unwrap_or_default(),
        None => *lock(&HOST),
    }
}

pub(crate) fn decode(data: &[u8]) -> Option<PlayerState> {
    match data {
        [1, active, shape, ..] => Some(PlayerState { active: *active != 0, shape: shape_index(i64::from(*shape)) }),
        _ => None,
    }
}
pub(crate) fn encode(s: PlayerState) -> [u8; 3] {
    [1, u8::from(s.active), s.shape as u8]
}

/// A guest's client told the world it hosts its key and shape.
pub(crate) fn net_from_client(player: u128, channel: &str, data: &[u8]) {
    if channel != CHANNEL {
        return;
    }
    if let Some(s) = decode(data) {
        lock(&GUESTS).get_or_insert_with(HashMap::new).insert(player, s);
    }
}

/// `FoodExhaustion.isTooHungry`.
fn too_hungry(game: &Game<'_>, cfg: &Config) -> bool {
    let food_on = cfg.food_exhaustion_enabled && cfg.food_exhaustion > 0.0;
    !game.creative
        && !cfg.allow_vein_mining_at_zero_hunger
        && food_on
        && game.player.as_ref().is_some_and(|p| p.health.food.food <= 0)
}

/// `FoodExhaustion.apply`.
fn exhaust(game: &mut Game<'_>, cfg: &Config) {
    if cfg.food_exhaustion_enabled
        && cfg.food_exhaustion > 0.0
        && let Some(p) = game.player.as_mut()
    {
        p.cause_food_exhaustion(cfg.food_exhaustion);
    }
}

/// The durability a stack has left, when it takes damage.
fn left(game: &Game<'_>, stack: Option<&ItemStack>) -> Option<u32> {
    let s = stack?;
    let max = game.res.items().get(&s.item).map_or(0, |i| i.max_damage);
    (max > 0).then(|| max.saturating_sub(s.damage))
}

/// The face of `pos` the player's view enters it through: the clip Liteminer's walkers
/// take their direction from. Off the view (a lagging guest), the side facing the eyes.
fn face_toward(game: &Game<'_>, pos: Pos) -> Pos {
    let Some((eye, dir)) = game.aim() else { return [0, 1, 0] };
    face_of_ray(eye, dir, pos)
}

pub(crate) fn face_of_ray(eye: [f64; 3], dir: [f64; 3], pos: Pos) -> Pos {
    let (mut near, mut axis) = (f64::NEG_INFINITY, None);
    let mut far = f64::INFINITY;
    for i in 0..3 {
        let (lo, hi) = (f64::from(pos[i]), f64::from(pos[i]) + 1.0);
        if dir[i].abs() < 1e-12 {
            if eye[i] < lo || eye[i] > hi {
                far = f64::NEG_INFINITY;
            }
            continue;
        }
        let (a, b) = ((lo - eye[i]) / dir[i], (hi - eye[i]) / dir[i]);
        let (t0, t1) = (a.min(b), a.max(b));
        if t0 > near {
            near = t0;
            axis = Some(i);
        }
        far = far.min(t1);
    }
    if let Some(i) = axis
        && near <= far
        && far >= 0.0
    {
        let mut f = [0; 3];
        f[i] = if dir[i] > 0.0 { -1 } else { 1 };
        return f;
    }
    // The axis the eyes are furthest along from the block's middle.
    let d: [f64; 3] = std::array::from_fn(|i| eye[i] - f64::from(pos[i]) - 0.5);
    let i = (0..3).max_by(|a, b| d[*a].abs().total_cmp(&d[*b].abs())).unwrap_or(1);
    let mut f = [0; 3];
    f[i] = if d[i] >= 0.0 { 1 } else { -1 };
    f
}

thread_local! {
    /// Inside a vein's own breaks, which come through the hook again.
    static WALKING: Cell<bool> = const { Cell::new(false) };
    /// Inside a vein's own uses.
    static INTERACTING: Cell<bool> = const { Cell::new(false) };
    /// While a vein block breaks with drops together: (the block mined, this block).
    static REDIRECT: Cell<Option<(Pos, Pos)>> = const { Cell::new(None) };
    /// The block whose break progress is being worked out (`destroy_progress`).
    static MINING_AT: Cell<Option<Pos>> = const { Cell::new(None) };
}

/// `OnBlockBreak`: a player is breaking `pos`, before it goes. With the key held, the rest
/// of the shape breaks first, each through the game's own break (`break_block_as_player`):
/// its drops (at the block mined, with drops together), experience, wear and statistic.
pub(crate) fn destroy(game: &mut Game<'_>, pos: Pos, canceled: &mut bool) {
    if *canceled || WALKING.with(Cell::get) || game.remote.is_some() || !game.native_on(ID) {
        return;
    }
    let state = acting_state(game);
    if !state.active {
        return;
    }
    ensure_loaded(game);
    let cfg = cfg();
    if too_hungry(game, &cfg) {
        return;
    }
    let tool = game.held().cloned();
    let blocks = {
        let look = Look::new(game, &cfg, tool.as_ref());
        if look.excluded_tool() || left(game, tool.as_ref()) == Some(1) {
            return;
        }
        let face = face_toward(game, pos);
        let yaw = game.player.as_ref().map_or(0.0, |p| p.yaw);
        walk(&look, &cfg, state.shape, pos, face, yaw)
    };
    let limit = cfg.block_break_limit as usize;
    WALKING.with(|w| w.set(true));
    let mut done = 1;
    for b in blocks {
        if b == pos {
            continue;
        }
        if done >= limit {
            break;
        }
        let held = game.held().cloned();
        let ok = Look::new(game, &cfg, held.as_ref()).should_mine(b);
        if !ok {
            continue;
        }
        if !game.creative {
            // The last two points of durability: stop before the tool would break.
            if cfg.prevent_tool_breaking && left(game, held.as_ref()).is_some_and(|l| l <= 2) {
                break;
            }
            exhaust(game, &cfg);
        }
        let before = game.world.block(b[0], b[1], b[2]);
        if cfg.drop_mode == DropMode::Together {
            REDIRECT.with(|r| r.set(Some((pos, b))));
        }
        let _ = game.break_block_as_player(b);
        REDIRECT.with(|r| r.set(None));
        if game.world.block(b[0], b[1], b[2]) != before {
            done += 1;
        }
    }
    WALKING.with(|w| w.set(false));
}

/// `Block.popResource`'s block: the block mined, for a vein's drops together.
pub(crate) fn drop_block(value: Pos) -> Pos {
    match REDIRECT.with(Cell::get) {
        Some((origin, at)) if at == value => origin,
        _ => value,
    }
}

/// Where a vein block's experience pops: by the block mined, with drops together.
pub(crate) fn orb_at(value: [f64; 3]) -> [f64; 3] {
    match REDIRECT.with(Cell::get) {
        Some((origin, at)) => std::array::from_fn(|i| value[i] + f64::from(origin[i] - at[i])),
        None => value,
    }
}

/// `OnBlockInteraction`: a tool used on `pos` with the key held strips, tills or paths the
/// rest of the shape too (`BlockTransformer`, wear and sound included).
pub(crate) fn use_on(game: &mut Game<'_>, stack: &ItemStack, pos: Pos, face: Pos, claimed: &mut bool) {
    if *claimed || INTERACTING.with(Cell::get) || game.remote.is_some() || !game.native_on(ID) {
        return;
    }
    let state = acting_state(game);
    if !state.active {
        return;
    }
    // `isTieredItem`: the item has a `tool` component.
    if game.res.items().get(&stack.item).is_none_or(|i| i.tool.is_empty()) {
        return;
    }
    ensure_loaded(game);
    let cfg = cfg();
    if too_hungry(game, &cfg) || left(game, Some(stack)) == Some(1) {
        return;
    }
    if game.block_transform(stack, pos, face).is_none() {
        return;
    }
    // Walked before the use changes the block clicked.
    let blocks = {
        let look = Look::new(game, &cfg, Some(stack));
        let yaw = game.player.as_ref().map_or(0.0, |p| p.yaw);
        walk(&look, &cfg, state.shape, pos, face, yaw)
    };
    let stack = stack.clone();
    INTERACTING.with(|i| i.set(true));
    if !matches!(game.use_block_transformer(&stack, pos, face), Ok(true)) {
        INTERACTING.with(|i| i.set(false));
        return;
    }
    *claimed = true;
    let limit = cfg.block_break_limit as usize;
    let mut done = 1;
    for b in blocks {
        if b == pos {
            continue;
        }
        if done >= limit {
            break;
        }
        let Some(held) = game.held().cloned() else { break };
        let ok = Look::new(game, &cfg, Some(&held)).should_mine(b);
        if !ok {
            continue;
        }
        if cfg.prevent_tool_breaking && left(game, Some(&held)).is_some_and(|l| l <= 2) {
            break;
        }
        if matches!(game.use_block_transformer(&held, b, face), Ok(true)) {
            done += 1;
            exhaust(game, &cfg);
        }
    }
    INTERACTING.with(|i| i.set(false));
}

/// `BlockBehaviour.getDestroyProgress` starts on `pos`.
pub(crate) fn mining_at(pos: Pos) {
    MINING_AT.with(|m| m.set(Some(pos)));
}

/// What a world worked out of a player's selection, for the harvest time.
struct Counted {
    key: (Option<u128>, Pos, usize, u32, Option<Arc<str>>),
    count: usize,
    asked: u32,
}
static COUNTED: Mutex<Vec<Counted>> = Mutex::new(Vec::new());

/// `PlayerMixin.getDestroySpeed`: the break speed, slower for each block of the selection.
pub(crate) fn break_speed(game: &mut Game<'_>, value: f32) -> f32 {
    let hint = MINING_AT.with(Cell::take);
    if !game.native_on(ID) {
        return value;
    }
    ensure_loaded(game);
    let cfg = cfg();
    if !cfg.harvest_time_per_block_modifier_enabled {
        return value;
    }
    let count = if game.native_window() && game.lan_acting_uuid.is_none() {
        let c = lock(&CLIENT);
        if !c.active {
            return value;
        }
        c.sel.as_ref().map_or(0, |s| s.blocks.len())
    } else {
        let state = acting_state(game);
        if !state.active {
            return value;
        }
        let Some(pos) = hint.or_else(|| game.mining.as_ref().map(|m| m.pos)) else { return value };
        let block = game.world.block(pos[0], pos[1], pos[2]);
        let tool = game.held().cloned();
        let key = (game.lan_acting_uuid, pos, state.shape, block.0, tool.as_ref().map(|t| t.item.clone()));
        let mut counted = lock(&COUNTED);
        if let Some(c) = counted.iter_mut().find(|c| c.key == key && c.asked < 10) {
            c.asked += 1;
            c.count
        } else {
            drop(counted);
            let look = Look::new(game, &cfg, tool.as_ref());
            let yaw = game.player.as_ref().map_or(0.0, |p| p.yaw);
            let count = walk(&look, &cfg, state.shape, pos, face_toward(game, pos), yaw).len();
            let mut counted = lock(&COUNTED);
            counted.retain(|c| c.key.0 != key.0);
            counted.push(Counted { key, count, asked: 0 });
            count
        }
    };
    value * speed_factor(&cfg, count)
}

// ---------------------------------------------------------------- the client

/// The selection shown: the blocks and their outline.
/// What a selection hangs on: the block and face looked at, the shape, the state, the tool
/// and its wear, and the configuration's generation.
type SelKey = (Pos, Pos, usize, u32, Option<(Arc<str>, u32)>, u64);
pub(crate) struct Selection {
    key: SelKey,
    pub blocks: Vec<Pos>,
    made: u64,
}

pub(crate) struct Client {
    pub active: bool,
    pub shape: usize,
    key_down: bool,
    toggled: bool,
    last_change: Option<Instant>,
    sent: Option<PlayerState>,
    tried: Option<(PlayerState, u64)>,
    pub sel: Option<Selection>,
    // The configuration screen.
    tab: usize,
    drag: Option<Act>,
    size: [f32; 2],
    open_tries: u32,
    close: bool,
}

pub(crate) static CLIENT: Mutex<Client> = Mutex::new(Client {
    active: false,
    shape: 0,
    key_down: false,
    toggled: false,
    last_change: None,
    sent: None,
    tried: None,
    sel: None,
    tab: 0,
    drag: None,
    size: [427.0, 240.0],
    open_tries: 0,
    close: false,
});
static FRAME: AtomicU64 = AtomicU64::new(100);
/// Bumped by each change of the configuration, so the selection is worked out again.
static CFG_GEN: AtomicU64 = AtomicU64::new(1);

fn client() -> MutexGuard<'static, Client> {
    lock(&CLIENT)
}

pub(crate) fn listed_mod(mods: &mut Vec<NativeMod>) {
    mods.push(NativeMod {
        id: ID.into(),
        title: format!("{NAME} {VERSION}"),
        description: DESCRIPTION.into(),
        keys: vec![[KEY.0.to_owned(), KEY.1.to_owned(), KEY.2.to_owned()]],
    });
}

pub(crate) fn key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !game.native_window() || !game.native_on(ID) {
        return;
    }
    if game.native_mod_key(ID, KEY.0, KEY.2) != key {
        return;
    }
    ensure_loaded(game);
    let mut c = client();
    if pressed {
        if !game.native_playing() {
            return;
        }
        c.key_down = true;
        if cfg().key_mode == KeyMode::Toggle {
            c.toggled = !c.toggled;
        }
    } else {
        c.key_down = false;
    }
}

/// Shapes change with the wheel while the key is held (`HUD.onMouseScroll`).
pub(crate) fn scroll(game: &mut Game<'_>, scroll: [f64; 2], canceled: &mut bool) {
    if !game.native_window() || !game.native_on(ID) || !client().active {
        return;
    }
    *canceled = true;
    if scroll[1] == 0.0 {
        return;
    }
    let shape = {
        let mut c = client();
        let n = SHAPES.len();
        c.shape = if scroll[1] > 0.0 { (c.shape + n - 1) % n } else { (c.shape + 1) % n };
        c.sel = None;
        c.shape
    };
    if !cfg().show_hud {
        game.titles.set_overlay(Text::literal(format!("Changed Shape [{}]", SHAPES[shape])), false);
    }
    send(game);
}

/// Tells the host what the key and shape are: through the shared state when this process
/// hosts the world, on the mod channel when it's someone else's.
fn send(game: &mut Game<'_>) {
    let state = {
        let c = client();
        PlayerState { active: c.active, shape: c.shape }
    };
    *lock(&HOST) = state;
    if game.remote.is_some() {
        // A server that doesn't carry mod messages (yet) is asked again a second later, not
        // every frame.
        let frame = FRAME.load(Ordering::Relaxed);
        let due = {
            let c = client();
            c.sent != Some(state) && c.tried.is_none_or(|(s, f)| s != state || frame.saturating_sub(f) >= 60)
        };
        if due {
            let ok = game.native_net_send_to_server(CHANNEL, &encode(state)).is_ok();
            let mut c = client();
            c.tried = Some((state, frame));
            if ok {
                c.sent = Some(state);
            }
        }
    } else {
        let mut c = client();
        c.sent = None;
        c.tried = None;
    }
}

pub(crate) fn frame(game: &mut Game<'_>) {
    let frame = FRAME.fetch_add(1, Ordering::Relaxed) + 1;
    if !game.native_window() {
        return;
    }
    if std::mem::take(&mut client().close) {
        game.native_mod_screen_close(ID);
    }
    let tries = std::mem::take(&mut client().open_tries);
    if tries > 0 && !game.native_mod_screen_open(ID) {
        client().open_tries = tries - 1;
    }
    if !game.native_on(ID) || game.player.is_none() {
        // `resetInput`: out of the world, nothing is held.
        let mut c = client();
        c.active = false;
        c.toggled = false;
        c.key_down = false;
        c.sel = None;
        c.sent = None;
        c.tried = None;
        drop(c);
        *lock(&HOST) = PlayerState::default();
        return;
    }
    ensure_loaded(game);
    let cfg = cfg();
    {
        let mut c = client();
        let want = match cfg.key_mode {
            KeyMode::Hold => c.key_down,
            KeyMode::Toggle => c.toggled,
        };
        if want != c.active && c.last_change.is_none_or(|t| t.elapsed() >= PACKET_DELAY) {
            c.active = want;
            c.last_change = Some(Instant::now());
            if !want {
                c.sel = None;
            }
        }
    }
    send(game);
    select(game, &cfg, frame);
}

/// `LiteminerSelection.refresh`: the blocks the key would take, looked at.
fn select(game: &Game<'_>, cfg: &Config, frame: u64) {
    if !client().active {
        return;
    }
    let Some(hit) = game.pick() else {
        client().sel = None;
        return;
    };
    let state = game.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
    let tool = game.held().map(|t| (t.item.clone(), t.damage));
    let shape = client().shape;
    let key = (hit.pos, hit.face, shape, state.0, tool.clone(), CFG_GEN.load(Ordering::Relaxed));
    // Kept while nothing it hangs on changes, and worked out again every tenth of a second
    // or so for blocks changed around it (a third of a second for the largest).
    let old = client().sel.take();
    if let Some(s) = old.as_ref() {
        let every = if s.blocks.len() > 512 { 20 } else { 6 };
        if s.key == key && frame.saturating_sub(s.made) < every {
            client().sel = old;
            return;
        }
    }
    let held = game.held().cloned();
    let look = Look::new(game, cfg, held.as_ref());
    let yaw = game.player.as_ref().map_or(0.0, |p| p.yaw);
    let blocks = walk(&look, cfg, shape, hit.pos, hit.face, yaw);
    client().sel = Some(Selection { key, blocks, made: frame });
}

pub(crate) fn visuals(game: &Game<'_>, rims: &mut Vec<NativeBlockRim>) {
    if !game.native_window() || !game.native_on(ID) {
        return;
    }
    let cfg = cfg();
    if !cfg.show_highlights {
        return;
    }
    let c = client();
    if !c.active {
        return;
    }
    let Some(sel) = c.sel.as_ref() else { return };
    let argb = 0xFF00_0000 | cfg.highlight_foreground_line_color;
    for &pos in sel.blocks.iter().take(8) {
        rims.push(NativeBlockRim { pos, argb, width: 2.0 });
    }
}

/// `HUD.onRenderHUD`: the count and the shape, by the crosshair.
pub(crate) fn hud(game: &Game<'_>, batch: &mut HudBatch) {
    if !game.native_window() || !game.native_on(ID) || game.player.is_none() {
        return;
    }
    let cfg = cfg();
    if !cfg.show_hud {
        return;
    }
    let (count, shape) = {
        let c = client();
        if !c.active {
            return;
        }
        (c.sel.as_ref().map_or(0, |s| s.blocks.len()), c.shape)
    };
    let status = match count {
        0 => String::new(),
        _ if too_hungry(game, &cfg) => "§6Too hungry to mine!".to_owned(),
        1 => "1 Block".to_owned(),
        n => format!("{n} Blocks"),
    };
    let scale = cfg.hud_scale;
    let cx = (batch.width / 2.0 / scale).floor() + (5.0 / scale).floor();
    let cy = (batch.height / 2.0 / scale).floor() + (-10.0 / scale).floor();
    let start = batch.vertices.len();
    for (i, line) in [status, SHAPES[shape].to_owned()].iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let t = if line.contains('§') { Text::legacy(line) } else { Text::literal(line.clone()) };
        batch.text_styled(&game.gui.atlas, &t, cx, cy + i as f32 * 10.0, packed(0xFFFF_FFFF), true);
    }
    if (scale - 1.0).abs() > 1e-3 {
        batch.scale_since(start, 0.0, 0.0, scale, scale);
    }
}

// ---------------------------------------------------------------- commands

pub(crate) fn commands(commands: &mut Vec<&'static str>) {
    commands.push("liteminer");
}

/// `/liteminer`, `/liteminer config` and `/liteminer shape set <index>`.
pub(crate) fn command(game: &mut Game<'_>, command: &str, ran: &mut bool) {
    if *ran || !game.native_on(ID) {
        return;
    }
    let words: Vec<&str> = command.split_whitespace().collect();
    if words.first() != Some(&"liteminer") {
        return;
    }
    *ran = true;
    ensure_loaded(game);
    match words.as_slice() {
        ["liteminer"] | ["liteminer", "config"] => open(),
        ["liteminer", "shape", "set", n] => match n.parse::<i64>() {
            Ok(i) if i >= 0 => {
                let s = shape_index(i);
                {
                    let mut c = client();
                    c.shape = s;
                    c.sel = None;
                }
                send(game);
                game.native_chat(&format!("[LiteMiner] Shape: {}", SHAPES[s]));
            }
            _ => game.native_chat("[LiteMiner] The index is a whole number from 0."),
        },
        _ => game.native_chat("[LiteMiner] /liteminer, /liteminer config, /liteminer shape set <index>"),
    }
}

// ---------------------------------------------------------------- Options > Mods

fn on_off(b: bool) -> &'static str {
    if b { "ON" } else { "OFF" }
}

pub(crate) fn settings_page(game: &mut Game<'_>, mod_id: &str, out: &mut Vec<String>) {
    if mod_id != ID {
        return;
    }
    ensure_loaded(game);
    let c = cfg();
    out.extend([
        "Liteminer Configuration...".to_owned(),
        format!("Key Mode: {}", if c.key_mode == KeyMode::Hold { "Hold" } else { "Toggle" }),
        format!("Show HUD: {}", on_off(c.show_hud)),
        format!("Show Block Highlights: {}", on_off(c.show_highlights)),
    ]);
}

pub(crate) fn settings_click(game: &mut Game<'_>, mod_id: &str, button: usize) {
    if mod_id != ID {
        return;
    }
    ensure_loaded(game);
    match button {
        0 => {
            if !game.native_mod_screen_open(ID) {
                open();
            }
        }
        1 => edit(|c| c.key_mode = if c.key_mode == KeyMode::Hold { KeyMode::Toggle } else { KeyMode::Hold }),
        2 => edit(|c| c.show_hud = !c.show_hud),
        3 => edit(|c| c.show_highlights = !c.show_highlights),
        _ => {}
    }
    CFG_GEN.fetch_add(1, Ordering::Relaxed);
}

fn open() {
    let mut c = client();
    c.drag = None;
    c.open_tries = 20;
}

// ---------------------------------------------------------------- the configuration screen

fn argb(rgb: u32, alpha: f32) -> u32 {
    ((alpha.clamp(0.0, 1.0) * 255.0).round() as u32) << 24 | (rgb & 0xFF_FFFF)
}
fn packed(c: u32) -> u32 {
    rgba((c >> 16) as u8, (c >> 8) as u8, c as u8, (c >> 24) as u8)
}
fn fill(g: &Game<'_>, b: &mut HudBatch, r: [f32; 4], color: u32) {
    if r[2] > 0.0 && r[3] > 0.0 && (color >> 24) > 0 {
        g.native_fill(b, r, color);
    }
}
fn text_w(g: &Game<'_>, s: &str) -> f32 {
    g.gui.atlas.font.width_text(&Text::literal(s.to_owned()))
}
fn label(g: &Game<'_>, b: &mut HudBatch, s: &str, x: f32, y: f32, color: u32, shadow: bool) {
    b.text_styled(&g.gui.atlas, &Text::literal(s.to_owned()), x, y, packed(color), shadow);
}
fn inside(p: [f32; 2], r: [f32; 4]) -> bool {
    p[0] >= r[0] && p[0] < r[0] + r[2] && p[1] >= r[1] && p[1] < r[1] + r[3]
}
fn bevel(g: &Game<'_>, b: &mut HudBatch, r: [f32; 4]) {
    let [x, y, w, h] = r;
    fill(g, b, [x + 1.0, y + 1.0, w - 2.0, h - 2.0], argb(0xC6C6C6, 1.0));
    fill(g, b, [x + 1.0, y + 1.0, w - 3.0, 2.0], argb(0xFFFFFF, 1.0));
    fill(g, b, [x + 1.0, y + 1.0, 2.0, h - 3.0], argb(0xFFFFFF, 1.0));
    fill(g, b, [x + 3.0, y + h - 3.0, w - 4.0, 2.0], argb(0x555555, 1.0));
    fill(g, b, [x + w - 3.0, y + 3.0, 2.0, h - 4.0], argb(0x555555, 1.0));
    fill(g, b, [x + 1.0, y, w - 2.0, 1.0], argb(0x000000, 1.0));
    fill(g, b, [x + 1.0, y + h - 1.0, w - 2.0, 1.0], argb(0x000000, 1.0));
    fill(g, b, [x, y + 1.0, 1.0, h - 2.0], argb(0x000000, 1.0));
    fill(g, b, [x + w - 1.0, y + 1.0, 1.0, h - 2.0], argb(0x000000, 1.0));
}
fn button(g: &Game<'_>, b: &mut HudBatch, r: [f32; 4], hover: bool, active: bool) {
    let sprite = if !active {
        "gui/sprites/widget/button_disabled"
    } else if hover {
        "gui/sprites/widget/button_highlighted"
    } else {
        "gui/sprites/widget/button"
    };
    if g.gui.atlas.sprites.contains_key(sprite) {
        horizon_client::ui::nine_slice(b, &g.gui.atlas, sprite, r[0], r[1], r[2], r[3], u32::MAX);
    } else {
        fill(
            g,
            b,
            r,
            argb(
                if !active {
                    0x3A3A3A
                } else if hover {
                    0x8A8AA0
                } else {
                    0x6E6E6E
                },
                1.0,
            ),
        );
    }
}
/// A vanilla slider (with `value` in 0..1) or, without one, a button, labelled.
fn widget(g: &Game<'_>, b: &mut HudBatch, r: [f32; 4], text: &str, value: Option<f32>, hover: bool, active: bool) {
    match value {
        Some(v) => {
            let bg = "gui/sprites/widget/slider";
            if g.gui.atlas.sprites.contains_key(bg) {
                horizon_client::ui::nine_slice(b, &g.gui.atlas, bg, r[0], r[1], r[2], r[3], u32::MAX);
            } else {
                fill(g, b, r, argb(0x000000, 1.0));
            }
            let hx = r[0] + (r[2] - 8.0) * v.clamp(0.0, 1.0);
            let handle =
                if hover { "gui/sprites/widget/slider_handle_highlighted" } else { "gui/sprites/widget/slider_handle" };
            if g.gui.atlas.sprites.contains_key(handle) {
                horizon_client::ui::nine_slice(b, &g.gui.atlas, handle, hx, r[1], 8.0, r[3], u32::MAX);
            } else {
                button(g, b, [hx, r[1], 8.0, r[3]], hover, true);
            }
        }
        None => button(g, b, r, hover, active),
    }
    let tw = text_w(g, text);
    let x = r[0] + ((r[2] - tw) / 2.0).max(2.0);
    let start = b.vertices.len();
    label(g, b, text, x, r[1] + (r[3] - 8.0) / 2.0, argb(if active { 0xFFFFFF } else { 0xA0A0A0 }, 1.0), true);
    if tw > r[2] - 4.0 {
        b.clip_since(start, r[0] + 2.0, r[1], r[0] + r[2] - 2.0, r[1] + r[3]);
    }
}

/// What a click on the screen does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Act {
    Tab(usize),
    PreventToolBreaking,
    RequireCorrectTool,
    BlockBreakLimit,
    DropMode,
    HarvestEnabled,
    HarvestModifier,
    FoodEnabled,
    FoodAmount,
    ZeroHunger,
    GrownCrops,
    Deepslate,
    KeyMode,
    ShowHud,
    HudScale,
    ShowHighlights,
    Foreground,
    SeeThrough,
    Reset,
    Done,
}

const PW: f32 = 340.0;
const PH: f32 = 226.0;
/// The steps the block break limit's slider stops at (the file takes any of 1 to 2048).
const LIMIT_STEPS: [u32; 31] = [
    1, 2, 3, 4, 5, 6, 8, 10, 12, 16, 20, 24, 32, 40, 48, 64, 80, 96, 128, 160, 192, 256, 320, 384, 512, 640, 768, 1024,
    1280, 1536, 2048,
];

type Hits = Vec<([f32; 4], Act)>;

fn layout(w: f32, h: f32, tab: usize) -> (f32, f32, Hits) {
    let x = ((w - PW) / 2.0).round();
    let y = ((h - PH) / 2.0).round().max(2.0);
    let mut hits: Hits = Vec::new();
    let tw = (PW - 24.0) / 2.0;
    hits.push(([x + 10.0, y + 20.0, tw, 20.0], Act::Tab(0)));
    hits.push(([x + 14.0 + tw, y + 20.0, tw, 20.0], Act::Tab(1)));
    let rows: &[[Act; 2]] = if tab == 0 {
        &[
            [Act::PreventToolBreaking, Act::RequireCorrectTool],
            [Act::BlockBreakLimit, Act::DropMode],
            [Act::HarvestEnabled, Act::HarvestModifier],
            [Act::FoodEnabled, Act::FoodAmount],
            [Act::ZeroHunger, Act::GrownCrops],
            [Act::Deepslate, Act::Reset],
        ]
    } else {
        &[
            [Act::KeyMode, Act::ShowHud],
            [Act::HudScale, Act::ShowHighlights],
            [Act::Foreground, Act::SeeThrough],
            [Act::Reset, Act::Reset],
        ]
    };
    let mut row = y + 48.0;
    for pair in rows {
        if pair[0] == pair[1] {
            hits.push(([x + 10.0 + (tw + 4.0) / 2.0, row, tw, 20.0], pair[0]));
        } else {
            hits.push(([x + 10.0, row, tw, 20.0], pair[0]));
            hits.push(([x + 14.0 + tw, row, tw, 20.0], pair[1]));
        }
        row += 24.0;
    }
    hits.push(([x + (PW - 150.0) / 2.0, y + PH - 28.0, 150.0, 20.0], Act::Done));
    (x, y, hits)
}

fn slider_of(c: &Config, a: Act) -> Option<(f32, String)> {
    Some(match a {
        Act::BlockBreakLimit => {
            let i = LIMIT_STEPS.iter().position(|s| *s >= c.block_break_limit).unwrap_or(LIMIT_STEPS.len() - 1);
            (i as f32 / (LIMIT_STEPS.len() - 1) as f32, format!("Block Break Limit: {}", c.block_break_limit))
        }
        Act::HarvestModifier => (
            (c.harvest_time_per_block_modifier - 1.0) / 9.0,
            format!("Harvest Time Modifier: {:.1}", c.harvest_time_per_block_modifier),
        ),
        Act::FoodAmount => (c.food_exhaustion, format!("Exhaustion Amount: {:.2}", c.food_exhaustion)),
        Act::HudScale => ((c.hud_scale - 0.5) / 1.5, format!("HUD Scale: {:.2}", c.hud_scale)),
        _ => return None,
    })
}

fn set_slider(c: &mut Config, a: Act, r: [f32; 4], x: f32) {
    let v = ((x - r[0] - 4.0) / (r[2] - 8.0)).clamp(0.0, 1.0);
    match a {
        Act::BlockBreakLimit => {
            c.block_break_limit = LIMIT_STEPS[(v * (LIMIT_STEPS.len() - 1) as f32).round() as usize]
        }
        Act::HarvestModifier => c.harvest_time_per_block_modifier = ((1.0 + v * 9.0) * 10.0).round() / 10.0,
        Act::FoodAmount => c.food_exhaustion = (v * 100.0).round() / 100.0,
        Act::HudScale => c.hud_scale = ((0.5 + v * 1.5) * 20.0).round() / 20.0,
        _ => {}
    }
}

fn text_of(c: &Config, a: Act, tab: usize) -> String {
    let b = |name: &str, v: bool| format!("{name}: {}", on_off(v));
    match a {
        Act::Tab(0) => "Liteminer".into(),
        Act::Tab(_) => "Liteminer Client".into(),
        Act::PreventToolBreaking => b("Prevent Tool Breaking", c.prevent_tool_breaking),
        Act::RequireCorrectTool => b("Require Correct Tool", c.require_correct_tool_enabled),
        Act::DropMode => {
            format!("Drops: {}", if c.drop_mode == DropMode::Together { "Together" } else { "At each block" })
        }
        Act::HarvestEnabled => b("Harvest Time Scaling", c.harvest_time_per_block_modifier_enabled),
        Act::FoodEnabled => b("Food Exhaustion", c.food_exhaustion_enabled),
        Act::ZeroHunger => b("Mine at Zero Hunger", c.allow_vein_mining_at_zero_hunger),
        Act::GrownCrops => b("Distinguish Crops", c.distinguish_grown_crops),
        Act::Deepslate => b("Match Deepslate Ores", c.match_deepslate_ore_variants),
        Act::KeyMode => format!("Key Mode: {}", if c.key_mode == KeyMode::Hold { "Hold" } else { "Toggle" }),
        Act::ShowHud => b("Show HUD", c.show_hud),
        Act::ShowHighlights => b("Show Highlights", c.show_highlights),
        Act::Foreground => format!("Foreground: {}", wool_name(c.highlight_foreground_line_color)),
        Act::SeeThrough => format!("See-through: {}", wool_name(c.highlight_see_through_line_color)),
        Act::Reset => {
            if tab == 0 {
                "Reset Liteminer".into()
            } else {
                "Reset Client".into()
            }
        }
        Act::Done => "Done".into(),
        _ => String::new(),
    }
}

/// Liteminer's tooltips.
fn tip_of(a: Act) -> &'static str {
    match a {
        Act::Tab(0) => {
            "Gameplay settings for vein mining limits, safety checks, drops, harvest cost, and block matching. They apply to the worlds you host."
        }
        Act::Tab(_) => {
            "Client settings control local input behavior, on-screen HUD text, and block highlight rendering."
        }
        Act::PreventToolBreaking => "Stops vein mining when the durability of the tool gets to 1.",
        Act::RequireCorrectTool => "If enabled, blocks can only be broken if holding the correct tool.",
        Act::BlockBreakLimit => "Upper limit to how many blocks can be broken at a time.",
        Act::DropMode => {
            "Together drops everything where you broke the first block. At each block drops items and experience where each block was."
        }
        Act::HarvestEnabled => "Increases the time it takes to break a block when vein mining.",
        Act::HarvestModifier => {
            "Modifier used to scale up the time it needs to mine a block. Higher values mean longer times to mine."
        }
        Act::FoodEnabled => "Causes your hunger to go down per block mined.",
        Act::FoodAmount => "How much food exhaustion to cause per block broken.",
        Act::ZeroHunger => "Allows vein mining when the hunger bar is empty.",
        Act::GrownCrops => {
            "If enabled, shapeless vein mining of crops will only select crops that are grown (or only select crops that are still growing)."
        }
        Act::Deepslate => {
            "If enabled, shapeless vein mining will treat <name>_ore and deepslate_<name>_ore as the same block."
        }
        Act::KeyMode => {
            "In Hold mode you must keep holding the key to mine. In toggle mode you have to hit the key to toggle."
        }
        Act::ShowHud => "Enables the text showing how many blocks you have selected and your mining shape.",
        Act::HudScale => "Changes how big the HUD elements are.",
        Act::ShowHighlights => "Show visual highlights around blocks that will be mined.",
        Act::Foreground => "Color for the main highlight lines (wool colors).",
        Act::SeeThrough => "Color for the see-through highlight lines (wool colors).",
        Act::Reset => "Back to Liteminer's defaults for this tab.",
        Act::Done => "",
    }
}

fn mine(game: &Game<'_>) -> bool {
    game.native_mod_screen() == Some(ID)
}

pub(crate) fn screen_draw(game: &Game<'_>, b: &mut HudBatch, screen: &NativeScreenView<'_>) {
    if !mine(game) {
        return;
    }
    ensure_loaded(game);
    let m = screen.mouse;
    let tab = {
        let mut c = client();
        c.size = [b.width, b.height];
        c.tab
    };
    let c = cfg();
    let (x, y, hits) = layout(b.width, b.height, tab);
    fill(game, b, [0.0, 0.0, b.width, b.height], argb(0x000000, 0.45));
    bevel(game, b, [x, y, PW, PH]);
    let title = "Liteminer Configuration";
    label(game, b, title, x + (PW - text_w(game, title)) / 2.0, y + 7.0, argb(0x404040, 1.0), false);
    for (r, act) in &hits {
        let hover = inside(m, *r);
        match act {
            Act::Tab(i) => widget(game, b, *r, &text_of(&c, *act, tab), None, hover, *i != tab),
            Act::Foreground | Act::SeeThrough => {
                widget(game, b, *r, &text_of(&c, *act, tab), None, hover, true);
                let col = if *act == Act::Foreground {
                    c.highlight_foreground_line_color
                } else {
                    c.highlight_see_through_line_color
                };
                fill(game, b, [r[0] + 4.0, r[1] + 5.0, 10.0, 10.0], argb(0x000000, 1.0));
                fill(game, b, [r[0] + 5.0, r[1] + 6.0, 8.0, 8.0], argb(col, 1.0));
            }
            a => match slider_of(&c, *a) {
                Some((v, text)) => widget(game, b, *r, &text, Some(v), hover, true),
                None => widget(game, b, *r, &text_of(&c, *a, tab), None, hover, true),
            },
        }
    }
    if let Some((_, act)) = hits.iter().find(|(r, _)| inside(m, *r)) {
        let tip = tip_of(*act);
        if !tip.is_empty() {
            tooltip(game, b, tip, m);
        }
    }
}

/// A tooltip, wrapped at 200 pixels.
fn tooltip(g: &Game<'_>, b: &mut HudBatch, text: &str, m: [f32; 2]) {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split(' ') {
        let next = if cur.is_empty() { word.to_owned() } else { format!("{cur} {word}") };
        if text_w(g, &next) > 200.0 && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_owned();
        } else {
            cur = next;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    let w = lines.iter().map(|l| text_w(g, l)).fold(0.0f32, f32::max) + 8.0;
    let h = lines.len() as f32 * 10.0 + 6.0;
    let x = (m[0] + 10.0).min(b.width - w - 2.0).max(2.0);
    let y = (m[1] - 14.0).min(b.height - h - 2.0).max(2.0);
    fill(g, b, [x, y, w, h], argb(0x100010, 0.94));
    fill(g, b, [x, y, w, 1.0], argb(0x5000FF, 0.6));
    fill(g, b, [x, y + h - 1.0, w, 1.0], argb(0x28007F, 0.6));
    for (i, l) in lines.iter().enumerate() {
        label(g, b, l, x + 4.0, y + 4.0 + i as f32 * 10.0, argb(0xFFFFFF, 1.0), true);
    }
}

pub(crate) fn screen_click(game: &Game<'_>, screen: &NativeScreenView<'_>, button: i32, canceled: &mut bool) {
    if !mine(game) {
        return;
    }
    *canceled = true;
    if button != 0 && button != 1 {
        return;
    }
    ensure_loaded(game);
    let m = screen.mouse;
    let (size, tab) = {
        let c = client();
        (c.size, c.tab)
    };
    let (_, _, hits) = layout(size[0], size[1], tab);
    let Some((r, act)) = hits.iter().find(|(r, _)| inside(m, *r)).copied() else { return };
    let back = button == 1;
    match act {
        Act::Tab(i) => {
            client().tab = i;
            return;
        }
        Act::Done => {
            client().close = true;
            return;
        }
        a if slider_of(&cfg(), a).is_some() => {
            edit(|c| set_slider(c, a, r, m[0]));
            client().drag = Some(a);
        }
        a => edit(|c| match a {
            Act::PreventToolBreaking => c.prevent_tool_breaking = !c.prevent_tool_breaking,
            Act::RequireCorrectTool => c.require_correct_tool_enabled = !c.require_correct_tool_enabled,
            Act::DropMode => {
                c.drop_mode = if c.drop_mode == DropMode::Together { DropMode::EachBlock } else { DropMode::Together }
            }
            Act::HarvestEnabled => {
                c.harvest_time_per_block_modifier_enabled = !c.harvest_time_per_block_modifier_enabled
            }
            Act::FoodEnabled => c.food_exhaustion_enabled = !c.food_exhaustion_enabled,
            Act::ZeroHunger => c.allow_vein_mining_at_zero_hunger = !c.allow_vein_mining_at_zero_hunger,
            Act::GrownCrops => c.distinguish_grown_crops = !c.distinguish_grown_crops,
            Act::Deepslate => c.match_deepslate_ore_variants = !c.match_deepslate_ore_variants,
            Act::KeyMode => c.key_mode = if c.key_mode == KeyMode::Hold { KeyMode::Toggle } else { KeyMode::Hold },
            Act::ShowHud => c.show_hud = !c.show_hud,
            Act::ShowHighlights => c.show_highlights = !c.show_highlights,
            Act::Foreground => c.highlight_foreground_line_color = wool_step(c.highlight_foreground_line_color, back),
            Act::SeeThrough => c.highlight_see_through_line_color = wool_step(c.highlight_see_through_line_color, back),
            Act::Reset => {
                if tab == 0 {
                    c.reset_common();
                } else {
                    c.reset_client();
                }
            }
            _ => {}
        }),
    }
    CFG_GEN.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn screen_drag(game: &Game<'_>, screen: &NativeScreenView<'_>, canceled: &mut bool) {
    if !mine(game) {
        return;
    }
    *canceled = true;
    let (drag, size, tab) = {
        let c = client();
        (c.drag, c.size, c.tab)
    };
    let Some(a) = drag else { return };
    let (_, _, hits) = layout(size[0], size[1], tab);
    if let Some((r, _)) = hits.iter().find(|(_, h)| *h == a) {
        let r = *r;
        edit(|c| set_slider(c, a, r, screen.mouse[0]));
        CFG_GEN.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn screen_release(game: &Game<'_>, canceled: &mut bool) {
    if !mine(game) {
        return;
    }
    *canceled = true;
    client().drag = None;
}

#[cfg(test)]
pub(crate) fn test_reset(dir: &std::path::Path, f: impl FnOnce(&mut Config)) {
    {
        let mut s = lock(&STORE);
        s.loaded = true;
        s.dir = Some(dir.to_path_buf());
        s.checked = Some(Instant::now());
        let mut c = Config::default();
        f(&mut c);
        s.cfg = Some(Arc::new(c.sanitized()));
    }
    *lock(&HOST) = PlayerState::default();
    *lock(&GUESTS) = None;
    lock(&COUNTED).clear();
    let mut c = client();
    c.active = false;
    c.shape = 0;
    c.key_down = false;
    c.toggled = false;
    c.last_change = None;
    c.sel = None;
    c.sent = None;
    c.tried = None;
    c.tab = 0;
    c.drag = None;
    c.close = false;
    c.open_tries = 0;
}
#[cfg(test)]
pub(crate) fn test_force_reload(game: &Game<'_>) {
    lock(&STORE).loaded = false;
    ensure_loaded(game);
}
#[cfg(test)]
pub(crate) fn test_set_host(s: PlayerState) {
    *lock(&HOST) = s;
}
#[cfg(test)]
pub(crate) fn test_hit(w: f32, h: f32, tab: usize, want: Act) -> [f32; 2] {
    let (_, _, hits) = layout(w, h, tab);
    let r = hits.iter().find(|(_, a)| *a == want).map(|(r, _)| *r).expect("a button");
    [r[0] + 6.0, r[1] + r[3] / 2.0]
}
