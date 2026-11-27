use super::{Game, Screen, lan::LanHost};
use horizon_protocol::packets::{configuration::CustomPayload, play::Serverbound as S};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex, atomic::{AtomicU32, AtomicU8, Ordering}};
use std::time::{Duration, Instant};
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::keyboard::PhysicalKey;

pub(super) const SCREEN_CHANNEL: &str = "horizon:java_screen";
pub(super) const SCREEN_INPUT_CHANNEL: &str = "horizon:java_screen_input";
pub(super) const KEYS_CHANNEL: &str = "horizon:java_keys";
pub(super) const CONFIG_MODS_CHANNEL: &str = "horizon:java_config_mods";
pub(super) const HUD_CHANNEL: &str = "horizon:java_hud";
pub(super) const OVERLAY_CHANNEL: &str = "horizon:java_overlay";
const CLIENT_WINDOW: i32 = -1;
const OVERLAY_WINDOW: i32 = -3;
const SCREEN_NONE: u8 = 0;
const SCREEN_DEVICE: u8 = 1;
const SCREEN_OWN_CLIENT: u8 = 2;
const SCREEN_SERVER: u8 = 3;
const JOIN_AFTER: u32 = 40;
const HUD_FRAME_INTERVAL: Duration = Duration::from_millis(50);
const OVERLAY_MOVE_INTERVAL: Duration = Duration::from_millis(20);

struct Device {
    joined: Option<String>,
    mods: Vec<String>,
    own: bool,
    uuid: String,
    inventory: u64,
}

static DEVICE: Mutex<Device> = Mutex::new(Device {
    joined: None,
    mods: Vec::new(),
    own: false,
    uuid: String::new(),
    inventory: 0,
});
static WAITED: AtomicU32 = AtomicU32::new(0);
static SCREEN_OWNER: AtomicU8 = AtomicU8::new(SCREEN_NONE);
static OVERLAY_OWNER: AtomicU8 = AtomicU8::new(SCREEN_NONE);
static GUI_SIZE: Mutex<(i32, i32)> = Mutex::new((0, 0));
static DEVICE_KEYS: LazyLock<Mutex<HashSet<i32>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
static SERVER_KEYS: LazyLock<Mutex<HashSet<i32>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
static DEVICE_DOWN: LazyLock<Mutex<HashMap<i32, (i32, i32)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
struct GuestDeviceClient {
    mods: HashSet<String>,
    own: bool,
}

static DEVICE_GUESTS: LazyLock<Mutex<std::collections::HashMap<String, GuestDeviceClient>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));
static ANNOUNCED_GUESTS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static SERVER_DOWN: LazyLock<Mutex<HashMap<i32, (i32, i32)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static OWN_PLAYER_SENT: Mutex<Option<Option<u128>>> = Mutex::new(None);
static HUD_FRAME_AT: Mutex<Option<Instant>> = Mutex::new(None);
static POINTER: Mutex<DevicePointer> = Mutex::new(DevicePointer {
    position: [0.0, 0.0],
    drag_button: None,
});

struct DevicePointer {
    position: [f32; 2],
    drag_button: Option<i32>,
}

#[derive(Default)]
struct DeviceOverlay {
    open: bool,
    owner: u8,
    size: Option<(i32, i32)>,
    mouse: Option<(i32, i32)>,
    mouse_at: Option<Instant>,
    pressed: Option<i32>,
}

static DEVICE_OVERLAY: LazyLock<Mutex<DeviceOverlay>> =
    LazyLock::new(|| Mutex::new(DeviceOverlay::default()));

pub(super) fn own_client() -> bool {
    let device = DEVICE.lock().unwrap_or_else(|error| error.into_inner());
    device.own && device.joined.is_some()
}

pub(super) fn screen_open() -> bool {
    SCREEN_OWNER.load(Ordering::Relaxed) != SCREEN_NONE
}

fn set_screen_owner(owner: u8) {
    SCREEN_OWNER.store(owner, Ordering::Relaxed);
    super::java_gui::set_device_screen_open(owner != SCREEN_NONE);
    if owner == SCREEN_NONE {
        let mut pointer = POINTER.lock().unwrap_or_else(|error| error.into_inner());
        pointer.drag_button = None;
    }
}

pub(super) fn gui_size(width: i32, height: i32) {
    *GUI_SIZE.lock().unwrap_or_else(|error| error.into_inner()) = (width.max(0), height.max(0));
}

fn uuid_string(uuid: u128) -> String {
    horizon_protocol::crypto::uuid_string(uuid)
}

fn host_port(address: &str) -> (String, u16) {
    if let Some(rest) = address.strip_prefix('[')
        && let Some((host, tail)) = rest.split_once(']')
    {
        let port = tail.strip_prefix(':').and_then(|value| value.parse().ok());
        return (host.to_owned(), port.unwrap_or(25565));
    }
    match address.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => {
            (host.to_owned(), port.parse().unwrap_or(25565))
        }
        _ => (address.to_owned(), 25565),
    }
}


pub(super) fn sdl_key(key: winit::keyboard::KeyCode) -> Option<i32> {
    use winit::keyboard::KeyCode as K;
    let letters = [
        K::KeyA, K::KeyB, K::KeyC, K::KeyD, K::KeyE, K::KeyF, K::KeyG, K::KeyH,
        K::KeyI, K::KeyJ, K::KeyK, K::KeyL, K::KeyM, K::KeyN, K::KeyO, K::KeyP,
        K::KeyQ, K::KeyR, K::KeyS, K::KeyT, K::KeyU, K::KeyV, K::KeyW, K::KeyX,
        K::KeyY, K::KeyZ,
    ];
    let digits = [
        K::Digit1, K::Digit2, K::Digit3, K::Digit4, K::Digit5,
        K::Digit6, K::Digit7, K::Digit8, K::Digit9, K::Digit0,
    ];
    let numpad = [
        K::Numpad1, K::Numpad2, K::Numpad3, K::Numpad4, K::Numpad5,
        K::Numpad6, K::Numpad7, K::Numpad8, K::Numpad9, K::Numpad0,
    ];
    let functions = [
        K::F1, K::F2, K::F3, K::F4, K::F5, K::F6,
        K::F7, K::F8, K::F9, K::F10, K::F11, K::F12,
    ];
    let extended_functions = [
        K::F13, K::F14, K::F15, K::F16, K::F17, K::F18,
        K::F19, K::F20, K::F21, K::F22, K::F23, K::F24,
    ];
    let at = |keys: &[K], base: i32| keys.iter().position(|value| *value == key).map(|index| base + index as i32);
    if let Some(code) = at(&letters, 4)
        .or_else(|| at(&digits, 30))
        .or_else(|| at(&numpad, 89))
        .or_else(|| at(&functions, 58))
        .or_else(|| at(&extended_functions, 104))
    {
        return Some(code);
    }
    Some(match key {
        K::Enter => 40,
        K::Escape => 41,
        K::Backspace => 42,
        K::Tab => 43,
        K::Space => 44,
        K::Minus => 45,
        K::Equal => 46,
        K::BracketLeft => 47,
        K::BracketRight => 48,
        K::Backslash => 49,
        K::Semicolon => 51,
        K::Quote => 52,
        K::Backquote => 53,
        K::Comma => 54,
        K::Period => 55,
        K::Slash => 56,
        K::CapsLock => 57,
        K::PrintScreen => 70,
        K::ScrollLock => 71,
        K::Pause => 72,
        K::Insert => 73,
        K::Home => 74,
        K::PageUp => 75,
        K::Delete => 76,
        K::End => 77,
        K::PageDown => 78,
        K::ArrowRight => 79,
        K::ArrowLeft => 80,
        K::ArrowDown => 81,
        K::ArrowUp => 82,
        K::NumLock => 83,
        K::NumpadDivide => 84,
        K::NumpadMultiply => 85,
        K::NumpadSubtract => 86,
        K::NumpadAdd => 87,
        K::NumpadEnter => 88,
        K::NumpadDecimal => 99,
        K::IntlBackslash => 100,
        K::ContextMenu => 101,
        K::Power => 102,
        K::NumpadEqual => 103,
        K::NumpadComma => 133,
        K::IntlRo => 135,
        K::KanaMode => 136,
        K::IntlYen => 137,
        K::Convert => 138,
        K::NonConvert => 139,
        K::Lang1 => 144,
        K::Lang2 => 145,
        K::Lang3 | K::Katakana => 146,
        K::Lang4 | K::Hiragana => 147,
        K::Lang5 => 148,
        K::Help => 117,
        K::Select => 119,
        K::Again => 121,
        K::Undo => 122,
        K::Cut => 123,
        K::Copy => 124,
        K::Paste => 125,
        K::Find => 126,
        K::AudioVolumeMute => 127,
        K::AudioVolumeUp => 128,
        K::AudioVolumeDown => 129,
        K::NumpadBackspace => 187,
        K::NumpadParenLeft => 182,
        K::NumpadParenRight => 183,
        K::NumpadHash => 204,
        K::NumpadMemoryStore => 208,
        K::NumpadMemoryRecall => 209,
        K::NumpadMemoryClear => 210,
        K::NumpadMemoryAdd => 211,
        K::NumpadMemorySubtract => 212,
        K::Abort => 155,
        K::NumpadClear => 216,
        K::NumpadClearEntry => 217,
        K::BrowserSearch => 280,
        K::BrowserHome => 281,
        K::BrowserBack => 282,
        K::BrowserForward => 283,
        K::BrowserStop => 284,
        K::BrowserRefresh => 285,
        K::BrowserFavorites => 286,
        K::Sleep => 258,
        K::WakeUp => 259,
        K::MediaTrackNext => 267,
        K::MediaTrackPrevious => 268,
        K::MediaStop => 269,
        K::Eject => 270,
        K::MediaPlayPause => 271,
        K::MediaSelect => 272,
        K::Open => 274,
        K::Props => 279,
        K::ControlLeft => 224,
        K::ShiftLeft => 225,
        K::AltLeft => 226,
        K::SuperLeft => 227,
        K::ControlRight => 228,
        K::ShiftRight => 229,
        K::AltRight => 230,
        K::SuperRight => 231,
        _ => return None,
    })
}

pub(super) fn sdl_keycode(key: winit::keyboard::KeyCode, scancode: i32) -> i32 {
    use winit::keyboard::KeyCode as K;
    let name = format!("{key:?}");
    let text = if key == K::Space {
        Some(" ".to_owned())
    } else {
        name.strip_prefix("Key")
            .or_else(|| name.strip_prefix("Digit"))
            .filter(|value| value.len() == 1)
            .map(str::to_lowercase)
    };
    if let Some(text) = text {
        return text.chars().next().map_or(scancode, |character| character as i32);
    }
    match key {
        K::Enter => 13,
        K::Escape => 27,
        K::Backspace => 8,
        K::Tab => 9,
        K::Delete => 127,
        K::Minus => '-' as i32,
        K::Equal => '=' as i32,
        K::BracketLeft => '[' as i32,
        K::BracketRight => ']' as i32,
        K::Backslash => '\\' as i32,
        K::Semicolon => ';' as i32,
        K::Quote => '\'' as i32,
        K::Backquote => '`' as i32,
        K::Comma => ',' as i32,
        K::Period => '.' as i32,
        K::Slash => '/' as i32,
        _ => scancode | (1 << 30),
    }
}

pub(super) fn sdl_button(button: i32) -> i32 {
    match button {
        0 => 1,
        1 => 3,
        2 => 2,
        3 => 4,
        4 => 5,
        value => value + 1,
    }
}

pub(super) fn screen_button(button: winit::event::MouseButton) -> i32 {
    sdl_button(super::glfw_button(button))
}

pub(super) fn sdl_modifiers(modifiers: winit::keyboard::ModifiersState) -> i32 {
    let mut bits = 0;
    if modifiers.shift_key() {
        bits |= 3;
    }
    if modifiers.control_key() {
        bits |= 192;
    }
    if modifiers.alt_key() {
        bits |= 768;
    }
    if modifiers.super_key() {
        bits |= 3072;
    }
    bits
}

fn set_device_keys(keys: Vec<i32>) {
    *DEVICE_KEYS.lock().unwrap_or_else(|error| error.into_inner()) = keys.into_iter().collect();
}

pub(super) fn key(key: i32, keycode: i32, mods: i32, action: u8) {
    if action > 2 {
        return;
    }
    let has_device = {
        let device = DEVICE.lock().unwrap_or_else(|error| error.into_inner());
        !device.mods.is_empty() && device.joined.is_some()
    };
    if !has_device {
        return;
    }
    let bound = {
        let keys = DEVICE_KEYS.lock().unwrap_or_else(|error| error.into_inner());
        keys.contains(&-1) || keys.contains(&key)
    };
    let should_send = {
        let mut pressed = DEVICE_DOWN.lock().unwrap_or_else(|error| error.into_inner());
        match action {
            0 => pressed.remove(&key).is_some(),
            1 if bound => {
                pressed.insert(key, (keycode, mods));
                true
            }
            2 => {
                if let Some(input) = pressed.get_mut(&key) {
                    *input = (keycode, mods);
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    };
    if !should_send {
        return;
    }
    if let Err(error) = horizon_jvm::device::key(key, keycode, mods, action) {
        log::warn!("Java mods: a key to the device client failed: {error}");
    }
}

pub(super) fn remote_key(game: &mut Game<'_>, key: i32, keycode: i32, mods: i32, action: u8) {
    if action > 2 || game.remote.is_none() || (game.screen != Screen::Playing && action != 0) {
        return;
    }
    let bound = {
        let keys = SERVER_KEYS.lock().unwrap_or_else(|error| error.into_inner());
        keys.contains(&-1) || keys.contains(&key)
    };
    let should_send = {
        let mut down = SERVER_DOWN.lock().unwrap_or_else(|error| error.into_inner());
        match action {
            0 => down.remove(&key).is_some(),
            1 if bound => {
                down.insert(key, (keycode, mods));
                true
            }
            2 => {
                if let Some(input) = down.get_mut(&key) {
                    *input = (keycode, mods);
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    };
    if !should_send {
        return;
    }
    let input = json!({
        "window": CLIENT_WINDOW,
        "kind": "bind",
        "key": key,
        "keycode": keycode,
        "mods": mods,
        "action": action,
        "down": action != 0,
    });
    game.send(S::CustomPayload(CustomPayload {
        channel: SCREEN_INPUT_CHANNEL.into(),
        data: input.to_string().into_bytes(),
    }));
}

pub(super) fn device_mouse(button: i32, mods: i32, down: bool) {
    let number = button;
    let key = -100 - number;
    let bound = {
        let keys = DEVICE_KEYS.lock().unwrap_or_else(|error| error.into_inner());
        keys.contains(&key)
    };
    let should_send = {
        let mut pressed = DEVICE_DOWN.lock().unwrap_or_else(|error| error.into_inner());
        if down {
            if bound {
                pressed.insert(key, (0, mods));
                true
            } else {
                pressed.contains_key(&key)
            }
        } else {
            pressed.remove(&key).is_some()
        }
    };
    if !should_send {
        return;
    }
    if let Err(error) = horizon_jvm::device::mouse(number, mods, down) {
        log::warn!("Java mods: a mouse button to the device client failed: {error}");
    }
}

pub(super) fn device_scroll(horizontal: f64, vertical: f64, slot: i32) {
    if !DEVICE_KEYS.lock().unwrap_or_else(|error| error.into_inner()).contains(&-200) {
        return;
    }
    if let Err(error) = horizon_jvm::device::scroll(horizontal, vertical, slot) {
        log::warn!("Java mods: the mouse wheel to the device client failed: {error}");
    }
}

pub(super) fn remote_mouse(game: &mut Game<'_>, button: i32, mods: i32, down: bool) {
    if game.remote.is_none() || (game.screen != Screen::Playing && down) {
        return;
    }
    let number = button;
    let key = -100 - number;
    let bound = SERVER_KEYS.lock().unwrap_or_else(|error| error.into_inner()).contains(&key);
    let should_send = {
        let mut pressed = SERVER_DOWN.lock().unwrap_or_else(|error| error.into_inner());
        if down {
            if bound {
                pressed.insert(key, (0, mods));
                true
            } else {
                false
            }
        } else {
            pressed.remove(&key).is_some()
        }
    };
    if !should_send {
        return;
    }
    let input = json!({
        "window": CLIENT_WINDOW,
        "kind": "bind",
        "mouse": number,
        "mods": mods,
        "down": down,
    });
    game.send(S::CustomPayload(CustomPayload {
        channel: SCREEN_INPUT_CHANNEL.into(),
        data: input.to_string().into_bytes(),
    }));
}

pub(super) fn release_inputs(game: &mut Game<'_>) {
    let device_pressed = {
        let mut pressed = DEVICE_DOWN.lock().unwrap_or_else(|error| error.into_inner());
        std::mem::take(&mut *pressed)
    };
    for (key, (keycode, mods)) in device_pressed {
        if key >= 0 {
            if let Err(error) = horizon_jvm::device::key(key, keycode, mods, 0) {
                log::warn!("Java mods: a device key release failed: {error}");
            }
        } else if (-105..=-101).contains(&key) {
            if let Err(error) = horizon_jvm::device::mouse(-100 - key, mods, false) {
                log::warn!("Java mods: a device mouse release failed: {error}");
            }
        }
    }
    if game.remote.is_none() {
        SERVER_DOWN.lock().unwrap_or_else(|error| error.into_inner()).clear();
        return;
    }
    let pressed = {
        let mut down = SERVER_DOWN.lock().unwrap_or_else(|error| error.into_inner());
        std::mem::take(&mut *down)
    };
    for (key, (keycode, mods)) in pressed {
        let input = if (-105..=-101).contains(&key) {
            json!({
                "window": CLIENT_WINDOW,
                "kind": "bind",
                "mouse": -100 - key,
                "mods": mods,
                "down": false,
            })
        } else if key >= 0 {
            json!({
                "window": CLIENT_WINDOW,
                "kind": "bind",
                "key": key,
                "keycode": keycode,
                "mods": mods,
                "action": 0,
                "down": false,
            })
        } else {
            continue;
        };
        game.send(S::CustomPayload(CustomPayload {
            channel: SCREEN_INPUT_CHANNEL.into(),
            data: input.to_string().into_bytes(),
        }));
    }
}

pub(super) fn remote_scroll(game: &mut Game<'_>, horizontal: f64, vertical: f64, slot: i32) {
    if game.remote.is_none()
        || game.screen != Screen::Playing
        || !SERVER_KEYS.lock().unwrap_or_else(|error| error.into_inner()).contains(&-200)
    {
        return;
    }
    let input = json!({
        "window": CLIENT_WINDOW,
        "kind": "bind",
        "x": horizontal,
        "scroll": vertical,
        "slot": slot,
    });
    game.send(S::CustomPayload(CustomPayload {
        channel: SCREEN_INPUT_CHANNEL.into(),
        data: input.to_string().into_bytes(),
    }));
}

pub(super) fn takes(channel: &str) -> bool {
    let device = DEVICE.lock().unwrap_or_else(|error| error.into_inner());
    if device.joined.is_none() || device.mods.is_empty() {
        return false;
    }
    if device.own {
        return !channel.starts_with("minecraft:") && !channel.starts_with("horizon:");
    }
    horizon_jvm::device::is_device_channel(channel, &device.mods)
}

pub(super) fn payload(channel: &str, data: &[u8]) {
    if let Err(error) = horizon_jvm::device::payload(channel, data) {
        log::warn!("Java mods: device payload {channel} failed: {error}");
    }
}

pub(super) fn remote_payload(game: &mut Game<'_>, channel: &str, data: &[u8]) -> bool {
    match channel {
        SCREEN_CHANNEL => {
            let Ok(frame) = serde_json::from_slice::<Value>(data) else {
                return true;
            };
            if frame.get("close").and_then(Value::as_bool) == Some(true)
                || frame.get("open").and_then(Value::as_bool) == Some(false)
            {
                set_screen_owner(SCREEN_NONE);
                return true;
            }
            if frame.get("open").and_then(Value::as_bool) == Some(true) {
                set_screen_owner(SCREEN_SERVER);
            }
            if let Some(operations) = frame.get("ops") {
                super::java_gui::set_device_screen_frame(&operations.to_string());
            }
            true
        }
        HUD_CHANNEL => {
            super::java_gui::set_device_hud_frame(&String::from_utf8_lossy(data));
            true
        }
        OVERLAY_CHANNEL => {
            OVERLAY_OWNER.store(SCREEN_SERVER, Ordering::Relaxed);
            super::java_gui::set_device_overlay_frame(&String::from_utf8_lossy(data));
            true
        }
        KEYS_CHANNEL => {
            if let Ok(keys) = serde_json::from_slice::<Vec<i32>>(data) {
                *SERVER_KEYS.lock().unwrap_or_else(|error| error.into_inner()) = keys.into_iter().collect();
            }
            true
        }
        CONFIG_MODS_CHANNEL => {
            if let Ok(mods) = serde_json::from_slice::<Vec<(String, String)>>(data) {
                game.settings.mods.java_configs = mods;
            }
            true
        }
        _ if takes(channel) => {
            payload(channel, data);
            true
        }
        _ => false,
    }
}

pub(super) fn reset_server_state() {
    DEVICE_GUESTS.lock().unwrap_or_else(|error| error.into_inner()).clear();
    ANNOUNCED_GUESTS.lock().unwrap_or_else(|error| error.into_inner()).clear();
    SERVER_KEYS.lock().unwrap_or_else(|error| error.into_inner()).clear();
    SERVER_DOWN.lock().unwrap_or_else(|error| error.into_inner()).clear();
    *OWN_PLAYER_SENT.lock().unwrap_or_else(|error| error.into_inner()) = None;
}

fn reset_device_client_state() {
    WAITED.store(0, Ordering::Relaxed);
    set_screen_owner(SCREEN_NONE);
    super::java_gui::reset_device();
    *DEVICE_KEYS.lock().unwrap_or_else(|error| error.into_inner()) = HashSet::new();
    DEVICE_DOWN.lock().unwrap_or_else(|error| error.into_inner()).clear();
    OVERLAY_OWNER.store(SCREEN_NONE, Ordering::Relaxed);
    *DEVICE_OVERLAY.lock().unwrap_or_else(|error| error.into_inner()) = DeviceOverlay::default();
    *HUD_FRAME_AT.lock().unwrap_or_else(|error| error.into_inner()) = None;
    *POINTER.lock().unwrap_or_else(|error| error.into_inner()) = DevicePointer {
        position: [0.0, 0.0],
        drag_button: None,
    };
    let should_leave = {
        let mut device = DEVICE.lock().unwrap_or_else(|error| error.into_inner());
        let should_leave = device.joined.take().is_some();
        device.own = false;
        device.inventory = 0;
        device.uuid.clear();
        device.mods.clear();
        should_leave
    };
    if should_leave
        && let Err(error) = horizon_jvm::device::leave()
    {
        log::warn!("Java mods: device client leave failed: {error}");
    }
}

pub(super) fn left() {
    reset_server_state();
    reset_device_client_state();
}

pub(super) fn guest_payload(host: &LanHost, conn: u32, channel: &str, body: &[u8]) -> Option<bool> {
    let guests = host
        .hub
        .guests
        .iter()
        .map(|guest| uuid_string(guest.uuid))
        .collect::<HashSet<_>>();
    let uuid = host
        .hub
        .guests
        .iter()
        .find(|guest| guest.conn == conn)
        .map(|guest| uuid_string(guest.uuid))?;
    let mut device_clients = DEVICE_GUESTS.lock().unwrap_or_else(|error| error.into_inner());
    device_clients.retain(|id, _| guests.contains(id));
    if channel == horizon_jvm::device::RUNS_OWN_CHANNEL {
        let mut mods = String::from_utf8_lossy(body)
            .split(',')
            .map(str::trim)
            .filter(|mod_id| !mod_id.is_empty())
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        let own = mods.remove("*");
        device_clients.insert(uuid.clone(), GuestDeviceClient { mods, own });
        drop(device_clients);
        if let Err(error) = horizon_jvm::device::runs_their_own(&uuid) {
            log::warn!("Java mods: the guest's device client registration failed: {error}");
        }
        return Some(true);
    }
    let device_channel = device_clients
        .get(&uuid)
        .is_some_and(|client| channel.split_once(':').is_some_and(|(namespace, _)| client.mods.contains(namespace)));
    if device_channel {
        drop(device_clients);
        if let Err(error) = horizon_jvm::device::from_game(&uuid, channel, body) {
            log::warn!("Java mods: the guest's device client payload {channel} failed: {error}");
        }
        return Some(true);
    }
    None
}

pub(super) fn guest_owns_client(uuid: &str) -> bool {
    DEVICE_GUESTS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(uuid)
        .is_some_and(|client| client.own)
}

pub(super) fn own_player(owner: Option<u128>) {
    let mut told = OWN_PLAYER_SENT.lock().unwrap_or_else(|error| error.into_inner());
    if *told == Some(owner) || !horizon_jvm::events::ready() {
        return;
    }
    let uuid = owner.map(uuid_string);
    if horizon_jvm::device::own_player(uuid.as_deref()).is_ok() {
        *told = Some(owner);
    }
}

pub(super) fn send_guest_join_state(host: &LanHost, conn: u32) {
    let keys = horizon_jvm::events::bound_keys().unwrap_or_default();
    let configs = horizon_jvm::events::config_mods().unwrap_or_default();
    host.hub.send_to(
        conn,
        &horizon_protocol::packets::play::Clientbound::CustomPayload(CustomPayload {
            channel: KEYS_CHANNEL.into(),
            data: serde_json::to_vec(&keys).unwrap_or_default(),
        }),
    );
    host.hub.send_to(
        conn,
        &horizon_protocol::packets::play::Clientbound::CustomPayload(CustomPayload {
            channel: CONFIG_MODS_CHANNEL.into(),
            data: serde_json::to_vec(&configs).unwrap_or_default(),
        }),
    );
    host.hub.send_to(
        conn,
        &horizon_protocol::packets::play::Clientbound::CustomPayload(CustomPayload {
            channel: OVERLAY_CHANNEL.into(),
            data: b"[]".to_vec(),
        }),
    );
}

pub(super) fn sync_guest_join_state(host: &LanHost) {
    static SYNC_TICKS: AtomicU32 = AtomicU32::new(0);
    if SYNC_TICKS.fetch_add(1, Ordering::Relaxed).is_multiple_of(100) {
        ANNOUNCED_GUESTS.lock().unwrap_or_else(|error| error.into_inner()).clear();
    }
    let connected = host.hub.online_players()
        .map(|guest| (uuid_string(guest.uuid), guest.conn))
        .collect::<Vec<_>>();
    let connected_ids = connected.iter().map(|(uuid, _)| uuid.clone()).collect::<HashSet<_>>();
    DEVICE_GUESTS.lock().unwrap_or_else(|error| error.into_inner())
        .retain(|uuid, _| connected_ids.contains(uuid));
    let mut announced = ANNOUNCED_GUESTS.lock().unwrap_or_else(|error| error.into_inner());
    announced.retain(|uuid| connected_ids.contains(uuid));
    for (uuid, conn) in connected {
        if guest_owns_client(&uuid) || !announced.insert(uuid) {
            continue;
        }
        send_guest_join_state(host, conn);
    }
}

impl Game<'_> {
    pub(super) fn tick_device_client(&mut self) {
        if self.integrated.is_some() || !horizon_jvm::events::enabled() {
            return;
        }
        let Some(remote) = self.remote.as_ref() else {
            return;
        };
        if !remote.joined || self.player.is_none() {
            return;
        }
        let join_key = format!("{}|{:x}", remote.session.address, remote.profile());
        let joined = DEVICE.lock().unwrap_or_else(|error| error.into_inner()).joined.clone();
        if joined.as_deref().is_some_and(|value| value != join_key.as_str()) {
            reset_device_client_state();
        }
        if joined.as_deref() != Some(join_key.as_str()) {
            if WAITED.fetch_add(1, Ordering::Relaxed) < JOIN_AFTER {
                return;
            }
            let (host, port) = host_port(&remote.session.address);
            let uuid = uuid_string(remote.profile());
            let name = remote.profile_name.clone();
            let local_ui = !super::java_gui::overlay_enabled() && !self.options.split_guest;
            let bridge_server = remote.mod_net;
            match horizon_jvm::device::join(&host, port, &uuid, &name, local_ui) {
                Ok(None) => return,
                Ok(Some(mut mods)) => {
                    let mut device = DEVICE.lock().unwrap_or_else(|error| error.into_inner());
                    device.joined = Some(join_key);
                    device.own = mods.first().is_some_and(|mod_id| mod_id == "*");
                    if device.own {
                        mods.remove(0);
                    }
                    device.uuid = uuid;
                    device.mods = mods;
                    let own = device.own;
                    let mod_ids = device.mods.join(",");
                    let data = if own {
                        format!("*{separator}{mod_ids}", separator = if mod_ids.is_empty() { "" } else { "," }).into_bytes()
                    } else {
                        mod_ids.into_bytes()
                    };
                    let no_mods = device.mods.is_empty();
                    drop(device);
                    if bridge_server || !local_ui {
                        self.send(S::CustomPayload(CustomPayload {
                            channel: horizon_jvm::device::RUNS_OWN_CHANNEL.into(),
                            data,
                        }));
                    }
                    if own {
                        self.own_client_greet();
                    } else {
                        match horizon_jvm::device::bound_keys() {
                            Ok(keys) => set_device_keys(keys),
                            Err(error) => log::warn!("Java mods: device client key bindings could not be read: {error}"),
                        }
                    }
                    if no_mods {
                        return;
                    }
                }
                Err(error) => {
                    log::warn!("Java mods: device client join failed: {error}");
                    WAITED.store(JOIN_AFTER.saturating_sub(20), Ordering::Relaxed);
                    return;
                }
            }
        }
        let device = DEVICE.lock().unwrap_or_else(|error| error.into_inner());
        let has_mods = !device.mods.is_empty();
        let own = device.own;
        drop(device);
        if !own && self.ticks.is_multiple_of(100)
            && let Ok(keys) = horizon_jvm::device::bound_keys()
        {
            set_device_keys(keys);
        }
        if !has_mods {
            return;
        }
        if own {
            self.tick_own_client();
            return;
        }
        let Some(player) = self.player.as_ref() else {
            return;
        };
        let players: Vec<Value> = self
            .remote_player_names()
            .into_iter()
            .filter_map(|(id, name)| {
                let uuid = self.remote_player_uuid(id)?;
                let mob = self.mobs.iter().find(|mob| mob.id == id)?;
                Some(json!({
                    "u": uuid_string(uuid),
                    "n": name,
                    "x": mob.position.x,
                    "y": mob.position.y,
                    "z": mob.position.z,
                    "yaw": mob.yaw,
                }))
            })
            .collect();
        let (width, height) = *GUI_SIZE.lock().unwrap_or_else(|error| error.into_inner());
        let state = json!({
            "x": player.position.x,
            "y": player.position.y,
            "z": player.position.z,
            "yaw": player.yaw,
            "pitch": player.pitch,
            "players": players,
            "w": width,
            "h": height,
        });
        match horizon_jvm::device::tick(&state.to_string()) {
            Ok(shown) => self.show_device(&shown),
            Err(error) => log::warn!("Java mods: device client tick failed: {error}"),
        }
        for (channel, data) in horizon_jvm::device::take_out() {
            self.send(S::CustomPayload(CustomPayload { channel, data }));
        }
    }

    fn own_client_greet(&mut self) {
        OVERLAY_OWNER.store(SCREEN_OWN_CLIENT, Ordering::Relaxed);
        super::java_gui::set_device_overlay_frame("[]");
        match horizon_jvm::events::bound_keys() {
            Ok(keys) => set_device_keys(keys),
            Err(error) => log::warn!("Java mods: own client key bindings could not be read: {error}"),
        }
        self.settings.mods.java_configs = horizon_jvm::events::config_mods().unwrap_or_else(|error| {
            log::warn!("Java mods: own client config screens could not be read: {error}");
            Vec::new()
        });
    }

    fn own_client_inventory(&self) -> Option<Vec<String>> {
        use std::hash::{Hash, Hasher};
        let slots: Vec<String> = (0..horizon_jvm::events::INVENTORY_SLOTS)
            .map(|slot| {
                let stack = match slot {
                    0..=35 => self.inventory.get(slot).cloned().flatten(),
                    36..=39 => self.armor[39 - slot].clone(),
                    _ => self.offhand.clone(),
                };
                stack.map_or_else(String::new, |stack| {
                    horizon_client::snbt::to_snbt(&horizon_items::item_nbt::item_to_nbt(&stack))
                })
            })
            .collect();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        slots.hash(&mut hasher);
        let hash = hasher.finish();
        let mut device = DEVICE.lock().unwrap_or_else(|error| error.into_inner());
        (std::mem::replace(&mut device.inventory, hash) != hash).then_some(slots)
    }

    fn tick_own_client(&mut self) {
        if self.ticks.is_multiple_of(100)
            && let Ok(keys) = horizon_jvm::events::bound_keys()
        {
            set_device_keys(keys);
        }
        let Some(player) = self.player.as_ref() else {
            return;
        };
        let mode = match self.mode {
            horizon_client::command::args::GameMode::Survival => 0,
            horizon_client::command::args::GameMode::Creative => 1,
            horizon_client::command::args::GameMode::Adventure => 2,
            horizon_client::command::args::GameMode::Spectator => 3,
        };
        let sneaking = self.keys.contains(&winit::keyboard::KeyCode::ShiftLeft);
        let (width, height) = *GUI_SIZE.lock().unwrap_or_else(|error| error.into_inner());
        let mut state = json!({
            "tick": self.ticks,
            "dim": self.dim.id(),
            "x": player.position.x,
            "y": player.position.y,
            "z": player.position.z,
            "yaw": player.yaw,
            "pitch": player.pitch,
            "mode": mode,
            "health": player.health.health(),
            "food": player.health.food.food,
            "sat": player.health.food.saturation,
            "perm": horizon_jvm::events::player_flags(0, sneaking, self.selected as u8, 0),
            "w": width,
            "h": height,
        });
        if let Some(inventory) = self.own_client_inventory() {
            state["inv"] = json!(inventory);
        }
        let state = state.to_string();
        if let Err(error) = super::java_gameplay::with_java_world(self, || {
            horizon_jvm::device::tick(&state)
        }) {
            log::warn!("Java mods: own client tick failed: {error}");
        }
        self.own_client_shows();
    }

    pub(super) fn draw_own_client_hud_frame(&mut self) {
        if !own_client() || self.ui.hide_gui || self.screen != Screen::Playing {
            return;
        }
        let now = Instant::now();
        {
            let mut frame_at = HUD_FRAME_AT.lock().unwrap_or_else(|error| error.into_inner());
            if frame_at.is_some_and(|next| now < next) {
                return;
            }
            *frame_at = Some(now + HUD_FRAME_INTERVAL);
        }
        let uuid = DEVICE.lock().unwrap_or_else(|error| error.into_inner()).uuid.clone();
        let look = [self.yaw, self.pitch];
        if let Err(error) = super::java_gameplay::with_java_world(self, || {
            horizon_jvm::events::client_hud_frame(&uuid, look)
        }) {
            log::warn!("Java mods: own client HUD frame failed: {error}");
        }
        self.own_client_shows();
    }

    fn own_client_input(&mut self, input: &Value) {
        let uuid = DEVICE.lock().unwrap_or_else(|error| error.into_inner()).uuid.clone();
        let input = input.to_string();
        if let Err(error) = super::java_gameplay::with_java_world(self, || {
            horizon_jvm::events::screen_input(&uuid, &input)
        }) {
            log::warn!("Java mods: own client input failed: {error}");
        }
        self.own_client_shows();
    }

    fn own_client_shows(&mut self) {
        for action in horizon_jvm::events::take_actions() {
            match action {
                horizon_jvm::events::Action::Tell { text, .. } => {
                    self.system_message(super::java_gameplay::java_text(&text));
                }
                horizon_jvm::events::Action::Overlay { text, .. } => {
                    self.native_client_action_bar(super::java_gameplay::java_text(&text), true);
                }
                horizon_jvm::events::Action::HudFrame { frame, .. } => {
                    super::java_gui::set_device_hud_frame(&frame);
                }
                horizon_jvm::events::Action::OverlayFrame { frame, .. } => {
                    OVERLAY_OWNER.store(SCREEN_OWN_CLIENT, Ordering::Relaxed);
                    super::java_gui::set_device_overlay_frame(&frame);
                }
                horizon_jvm::events::Action::ClientScreen { open, .. } => {
                    if open {
                        set_screen_owner(SCREEN_OWN_CLIENT);
                    } else if SCREEN_OWNER.load(Ordering::Relaxed) == SCREEN_OWN_CLIENT {
                        set_screen_owner(SCREEN_NONE);
                    }
                }
                horizon_jvm::events::Action::ScreenFrame { frame, .. }
                    if SCREEN_OWNER.load(Ordering::Relaxed) == SCREEN_OWN_CLIENT =>
                {
                    super::java_gui::set_device_screen_frame(&frame);
                }
                _ => {}
            }
        }
        self.drain_device_out();
    }

    fn drain_device_out(&mut self) {
        for (channel, data) in horizon_jvm::device::take_out() {
            self.send(S::CustomPayload(CustomPayload { channel, data }));
        }
    }

    fn show_device(&mut self, shown: &str) {
        let Ok(shown) = serde_json::from_str::<Value>(shown) else {
            return;
        };
        if shown.get("close").and_then(Value::as_bool) == Some(true)
            && SCREEN_OWNER.load(Ordering::Relaxed) == SCREEN_DEVICE
        {
            set_screen_owner(SCREEN_NONE);
        }
        if let Some(Value::Array(ops)) = shown.get("hud") {
            super::java_gui::set_device_hud_frame(&Value::Array(ops.clone()).to_string());
        }
        if let Some(Value::Array(ops)) = shown.get("screen") {
            set_screen_owner(SCREEN_DEVICE);
            super::java_gui::set_device_screen_frame(&Value::Array(ops.clone()).to_string());
        }
    }

    pub(super) fn device_screen_input(&mut self, input: &Value) {
        let mut input = input.clone();
        input["window"] = json!(CLIENT_WINDOW);
        match SCREEN_OWNER.load(Ordering::Relaxed) {
            SCREEN_DEVICE => match horizon_jvm::device::input(&input.to_string()) {
                Ok(shown) => self.show_device(&shown),
                Err(error) => log::warn!("Java mods: device screen input failed: {error}"),
            },
            SCREEN_OWN_CLIENT => self.own_client_input(&input),
            SCREEN_SERVER => {
                self.send(S::CustomPayload(CustomPayload {
                    channel: SCREEN_INPUT_CHANNEL.into(),
                    data: input.to_string().into_bytes(),
                }));
            }
            _ => return,
        }
        if input.get("kind").and_then(Value::as_str) == Some("close")
            && SCREEN_OWNER.load(Ordering::Relaxed) != SCREEN_SERVER
        {
            set_screen_owner(SCREEN_NONE);
        }
    }

    fn send_device_overlay_input(&mut self, owner: u8, mut input: Value) {
        input["window"] = json!(OVERLAY_WINDOW);
        if owner == SCREEN_SERVER {
            self.send(S::CustomPayload(CustomPayload {
                channel: SCREEN_INPUT_CHANNEL.into(),
                data: input.to_string().into_bytes(),
            }));
        } else if owner == SCREEN_OWN_CLIENT {
            self.own_client_input(&input);
        }
    }

    pub(super) fn device_overlay_frame(
        &mut self,
        width: i32,
        height: i32,
        inventory_visible: bool,
    ) {
        let owner = OVERLAY_OWNER.load(Ordering::Relaxed);
        let visible = inventory_visible
            && !screen_open()
            && super::java_gui::device_overlay_open()
            && (owner == SCREEN_SERVER || owner == SCREEN_OWN_CLIENT)
            && (self.remote.is_some() || own_client());
        let size = (width.max(0), height.max(0));
        let mouse = (self.cursor.0 as i32, self.cursor.1 as i32);
        let mut message = None;
        let mut close = None;
        {
            let mut overlay = DEVICE_OVERLAY.lock().unwrap_or_else(|error| error.into_inner());
            if !visible {
                if overlay.open {
                    close = Some((overlay.owner, overlay.pressed.take()));
                }
                *overlay = DeviceOverlay::default();
            } else if !overlay.open || overlay.owner != owner {
                if overlay.open {
                    close = Some((overlay.owner, overlay.pressed.take()));
                }
                overlay.open = true;
                overlay.owner = owner;
                overlay.size = Some(size);
                overlay.mouse = Some(mouse);
                overlay.mouse_at = Some(Instant::now());
                message = Some(json!({
                    "kind":"open", "width":size.0, "height":size.1,
                    "x":mouse.0, "y":mouse.1,
                }));
            } else if overlay.size != Some(size) {
                overlay.size = Some(size);
                overlay.mouse = Some(mouse);
                overlay.mouse_at = Some(Instant::now());
                message = Some(json!({
                    "kind":"size", "width":size.0, "height":size.1,
                    "x":mouse.0, "y":mouse.1,
                }));
            } else {
                let now = Instant::now();
                let due = overlay.mouse != Some(mouse)
                    && overlay.mouse_at.is_none_or(|at| now.duration_since(at) >= OVERLAY_MOVE_INTERVAL);
                if due {
                    overlay.mouse = Some(mouse);
                    overlay.mouse_at = Some(now);
                    message = Some(json!({"kind":"move", "x":mouse.0, "y":mouse.1}));
                }
            }
        }
        if let Some((owner, pressed)) = close {
            if let Some(button) = pressed {
                self.send_device_overlay_input(owner, json!({
                    "kind":"release", "x":mouse.0, "y":mouse.1,
                    "button":button, "mods":sdl_modifiers(self.modifiers),
                }));
            }
            self.send_device_overlay_input(owner, json!({"kind":"close"}));
        }
        if let Some(message) = message {
            self.send_device_overlay_input(owner, message);
        }
    }

    fn inventory_screen_key(&self, key: winit::keyboard::KeyCode) -> bool {
        use winit::keyboard::KeyCode as K;
        key == K::Escape
            || matches!(
                self.settings.keys.translate(key),
                Some(Ok(K::KeyE
                    | K::KeyQ
                    | K::KeyF
                    | K::Digit1
                    | K::Digit2
                    | K::Digit3
                    | K::Digit4
                    | K::Digit5
                    | K::Digit6
                    | K::Digit7
                    | K::Digit8
                    | K::Digit9))
            )
    }

    pub(super) fn device_overlay_event(&mut self, event: &WindowEvent) -> bool {
        if screen_open()
            || !matches!(self.screen, Screen::Inventory | Screen::Creative)
            || !super::java_gui::device_overlay_open()
        {
            return false;
        }
        let owner = OVERLAY_OWNER.load(Ordering::Relaxed);
        if owner != SCREEN_SERVER && owner != SCREEN_OWN_CLIENT {
            return false;
        }
        let typing = super::java_gui::device_overlay_typing();
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(renderer) = &self.renderer {
                    let batch = self.interface_batch([renderer.config.width, renderer.config.height]);
                    self.cursor = self.interface_point([position.x as f32, position.y as f32], batch.scale);
                }
                let mouse = (self.cursor.0 as i32, self.cursor.1 as i32);
                let mut overlay = DEVICE_OVERLAY.lock().unwrap_or_else(|error| error.into_inner());
                let dx = mouse.0 - overlay.mouse.unwrap_or(mouse).0;
                let dy = mouse.1 - overlay.mouse.unwrap_or(mouse).1;
                if let Some(button) = overlay.pressed {
                    overlay.mouse = Some(mouse);
                    drop(overlay);
                    self.send_device_overlay_input(owner, json!({
                        "kind":"drag", "x":mouse.0, "y":mouse.1,
                        "button":button, "dx":dx, "dy":dy,
                    }));
                    return true;
                }
                false
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let number = screen_button(*button);
                let down = *state == ElementState::Pressed;
                let outside = self.carried.is_none() && self.inventory_target().1;
                let mut overlay = DEVICE_OVERLAY.lock().unwrap_or_else(|error| error.into_inner());
                let take = if down {
                    if outside {
                        overlay.pressed = Some(number);
                    }
                    outside
                } else {
                    let take = overlay.pressed == Some(number);
                    if take {
                        overlay.pressed = None;
                    }
                    take
                };
                drop(overlay);
                if take {
                    self.send_device_overlay_input(owner, json!({
                        "kind": if down { "press" } else { "release" },
                        "x":self.cursor.0, "y":self.cursor.1, "button":number,
                        "mods":sdl_modifiers(self.modifiers), "horizon":true,
                    }));
                }
                take
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if !self.inventory_target().1 {
                    return false;
                }
                let (horizontal, vertical) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (f64::from(*x), f64::from(*y)),
                    MouseScrollDelta::PixelDelta(position) => (position.x / 30.0, position.y / 30.0),
                };
                self.send_device_overlay_input(owner, json!({
                    "kind":"scroll", "x":self.cursor.0, "y":self.cursor.1,
                    "h":horizontal, "v":vertical, "horizon":true,
                }));
                true
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(key) = event.physical_key else {
                    return false;
                };
                let takes = if typing {
                    key != winit::keyboard::KeyCode::Escape
                } else {
                    !self.inventory_screen_key(key) && !self.recipe_search_focused()
                };
                if !takes {
                    return false;
                }
                if event.state == ElementState::Pressed
                    && let Some(code) = sdl_key(key)
                {
                    let mut input = json!({
                        "kind":"key", "key":code, "keycode":sdl_keycode(key, code),
                        "mods":sdl_modifiers(self.modifiers),
                    });
                    if let Some(index) = self.inventory_target().0
                        .and_then(|index| Self::menu_slot_index("inventory", index))
                    {
                        input["slot"] = json!(index);
                    }
                    self.send_device_overlay_input(owner, input);
                    if let Some(text) = event.text.as_deref() {
                        for character in text.chars().filter(|character| !character.is_control()) {
                            self.send_device_overlay_input(owner, json!({
                                "kind":"char", "char":character as u32,
                            }));
                        }
                    }
                }
                true
            }
            WindowEvent::Ime(winit::event::Ime::Commit(text)) if typing => {
                for character in text.chars().filter(|character| !character.is_control()) {
                    self.send_device_overlay_input(owner, json!({
                        "kind":"char", "char":character as u32,
                    }));
                }
                true
            }
            WindowEvent::Focused(false) => {
                let pressed = DEVICE_OVERLAY.lock().unwrap_or_else(|error| error.into_inner()).pressed.take();
                if let Some(button) = pressed {
                    self.send_device_overlay_input(owner, json!({
                        "kind":"release", "x":self.cursor.0, "y":self.cursor.1,
                        "button":button, "mods":sdl_modifiers(self.modifiers),
                    }));
                }
                false
            }
            _ => false,
        }
    }

    pub(super) fn device_screen_event(&mut self, event: &WindowEvent) -> bool {
        if !screen_open() {
            return false;
        }
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(renderer) = &self.renderer {
                    let batch = self.interface_batch([renderer.config.width, renderer.config.height]);
                    self.cursor = self.interface_point([position.x as f32, position.y as f32], batch.scale);
                }
                let x = self.cursor.0;
                let y = self.cursor.1;
                let input = {
                    let mut pointer = POINTER.lock().unwrap_or_else(|error| error.into_inner());
                    let dx = x - pointer.position[0];
                    let dy = y - pointer.position[1];
                    pointer.position = [x, y];
                    if let Some(button) = pointer.drag_button {
                        json!({"kind":"drag", "x":x, "y":y, "button":button, "dx":dx, "dy":dy})
                    } else {
                        json!({"kind":"move", "x":x, "y":y})
                    }
                };
                self.device_screen_input(&input);
                true
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button = screen_button(*button);
                let down = *state == ElementState::Pressed;
                {
                    let mut pointer = POINTER.lock().unwrap_or_else(|error| error.into_inner());
                    if down {
                        pointer.drag_button = Some(button);
                    } else if pointer.drag_button == Some(button) {
                        pointer.drag_button = None;
                    }
                    pointer.position = [self.cursor.0, self.cursor.1];
                }
                self.device_screen_input(&json!({
                    "kind": if down { "press" } else { "release" },
                    "x": self.cursor.0,
                    "y": self.cursor.1,
                    "button": button,
                    "mods": sdl_modifiers(self.modifiers),
                }));
                true
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (horizontal, vertical) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (f64::from(*x), f64::from(*y)),
                    MouseScrollDelta::PixelDelta(position) => (position.x / 30.0, position.y / 30.0),
                };
                self.device_screen_input(&json!({
                    "kind":"scroll",
                    "x":self.cursor.0,
                    "y":self.cursor.1,
                    "h":horizontal,
                    "v":vertical,
                }));
                true
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(key) = event.physical_key else {
                    return false;
                };
                let owner = SCREEN_OWNER.load(Ordering::Relaxed);
                let takes = if owner == SCREEN_SERVER {
                    key != winit::keyboard::KeyCode::Escape
                        && (super::java_gui::device_screen_typing()
                            || self.settings.keys.translate(key)
                                != Some(Ok(winit::keyboard::KeyCode::KeyE)))
                        && !self.recipe_search_focused()
                } else {
                    true
                };
                if !takes {
                    return false;
                }
                if event.state == ElementState::Pressed
                    && let Some(code) = sdl_key(key)
                {
                    self.device_screen_input(&json!({
                        "kind":"key",
                        "key":code,
                        "keycode":sdl_keycode(key, code),
                        "mods":sdl_modifiers(self.modifiers),
                    }));
                    if let Some(text) = event.text.as_deref() {
                        for character in text.chars().filter(|character| !character.is_control()) {
                            self.device_screen_input(&json!({"kind":"char", "char":character as u32}));
                        }
                    }
                }
                true
            }
            WindowEvent::Ime(winit::event::Ime::Commit(text)) => {
                let owner = SCREEN_OWNER.load(Ordering::Relaxed);
                if owner == SCREEN_SERVER && !super::java_gui::device_screen_typing() {
                    return false;
                }
                for character in text.chars().filter(|character| !character.is_control()) {
                    self.device_screen_input(&json!({"kind":"char", "char":character as u32}));
                }
                true
            }
            WindowEvent::Focused(false) => {
                let button = POINTER.lock().unwrap_or_else(|error| error.into_inner()).drag_button.take();
                if let Some(button) = button {
                    self.device_screen_input(&json!({
                        "kind":"release",
                        "x":self.cursor.0,
                        "y":self.cursor.1,
                        "button":button,
                        "mods":0,
                    }));
                }
                false
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{host_port, sdl_key, screen_button, uuid_string};
    use winit::{event::MouseButton, keyboard::KeyCode};

    #[test]
    fn addresses_split_into_host_and_port() {
        assert_eq!(host_port("localhost:25599"), ("localhost".into(), 25599));
        assert_eq!(host_port("example.org"), ("example.org".into(), 25565));
        assert_eq!(host_port("[::1]:25570"), ("::1".into(), 25570));
        assert_eq!(host_port("::1"), ("::1".into(), 25565));
    }

    #[test]
    fn uuids_keep_the_protocol_string_form() {
        assert_eq!(uuid_string(0), "00000000-0000-0000-0000-000000000000");
    }

    #[test]
    fn keyboard_and_mouse_inputs_use_sdl_codes() {
        assert_eq!(sdl_key(KeyCode::KeyA), Some(4));
        assert_eq!(sdl_key(KeyCode::Escape), Some(41));
        assert_eq!(sdl_key(KeyCode::F12), Some(69));
        assert_eq!(sdl_key(KeyCode::F13), Some(104));
        assert_eq!(sdl_key(KeyCode::F24), Some(115));
        assert_eq!(sdl_key(KeyCode::PrintScreen), Some(70));
        assert_eq!(sdl_key(KeyCode::BrowserBack), Some(282));
        assert_eq!(sdl_key(KeyCode::NumpadEqual), Some(103));
        assert_eq!(super::sdl_keycode(KeyCode::KeyA, 4), 97);
        assert_eq!(super::sdl_keycode(KeyCode::F12, 69), 0x4000_0000 | 69);
        assert_eq!(screen_button(MouseButton::Left), 1);
        assert_eq!(screen_button(MouseButton::Middle), 2);
        assert_eq!(screen_button(MouseButton::Right), 3);
    }
}
