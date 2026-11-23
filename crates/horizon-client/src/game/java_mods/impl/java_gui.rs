use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use horizon_client_render::hud::HudBatch;
use serde_json::Value;

const MAX_FRAME_BYTES: usize = 28 * 1024 * 1024;
const MAX_FRAME_OPS: usize = 4096;
const MAX_DYNAMIC_IMAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_FRAME_IMAGE_BYTES: usize = 24 * 1024 * 1024;

#[derive(Clone)]
struct DynamicImage {
    width: u32,
    height: u32,
    rgba: Arc<Vec<u8>>,
}

#[derive(Clone, Default)]
struct Frame {
    operations: Vec<Value>,
    images: HashMap<String, DynamicImage>,
}

#[derive(Clone, Default)]
struct UiState {
    local_uuid: Option<String>,
    screen_open: bool,
    menu_open: bool,
    client_screen_open: bool,
    screen: Option<Arc<Frame>>,
    hud: Option<Arc<Frame>>,
    overlay: Option<Arc<Frame>>,
    sent_size: Option<[u32; 2]>,
    dynamic_images: HashMap<String, DynamicImage>,
    drag_button: Option<i32>,
    pointer: [f32; 2],
}

impl UiState {
    fn set_menu_open(&mut self, open: bool) {
        if open && !self.screen_open {
            self.screen = None;
            self.sent_size = None;
            self.drag_button = None;
        }
        self.menu_open = open;
        self.screen_open = self.menu_open || self.client_screen_open;
        if !self.screen_open {
            self.screen = None;
            self.sent_size = None;
            self.drag_button = None;
        }
    }

    fn set_screen_frame(&mut self, frame: Frame) {
        self.screen = Some(Arc::new(frame));
        self.screen_open = self.menu_open || self.client_screen_open;
    }

    fn set_client_screen_open(&mut self, open: bool) {
        self.client_screen_open = open;
        self.screen_open = self.menu_open || self.client_screen_open;
        self.screen = None;
        self.sent_size = None;
        self.drag_button = None;
    }

    fn clear_screen(&mut self) {
        self.menu_open = false;
        self.client_screen_open = false;
        self.screen_open = false;
        self.screen = None;
        self.sent_size = None;
        self.drag_button = None;
    }
}

static UI: Mutex<UiState> = Mutex::new(UiState {
    local_uuid: None,
    screen_open: false,
    menu_open: false,
    client_screen_open: false,
    screen: None,
    hud: None,
    overlay: None,
    sent_size: None,
    dynamic_images: HashMap::new(),
    drag_button: None,
    pointer: [0.0, 0.0],
});

pub(in crate::game) fn reset() {
    *UI.lock().unwrap_or_else(|e| e.into_inner()) = UiState::default();
}

pub(in crate::game) fn set_local_uuid(uuid: &str) {
    let mut ui = UI.lock().unwrap_or_else(|e| e.into_inner());
    if ui.local_uuid.as_deref() != Some(uuid) {
        ui.local_uuid = Some(uuid.to_owned());
        ui.clear_screen();
        ui.hud = None;
        ui.overlay = None;
        ui.sent_size = None;
        ui.dynamic_images.clear();
        ui.drag_button = None;
    }
}

pub(in crate::game) fn set_screen_open(open: bool) {
    UI.lock()
        .unwrap_or_else(|e| e.into_inner())
        .set_client_screen_open(open);
}

pub(in crate::game) fn set_menu_open(open: bool) {
    UI.lock()
        .unwrap_or_else(|e| e.into_inner())
        .set_menu_open(open);
}

pub(in crate::game) fn clear_screen() {
    UI.lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear_screen();
}

pub(in crate::game) fn screen_open() -> bool {
    UI.lock().unwrap_or_else(|e| e.into_inner()).screen_open
}

pub(in crate::game) fn set_screen_frame(json: &str) {
    set_frame(json, |ui, frame| ui.set_screen_frame(frame));
}

pub(in crate::game) fn set_hud_frame(json: &str) {
    set_frame(json, |ui, frame| ui.hud = Some(Arc::new(frame)));
}

pub(in crate::game) fn set_overlay_frame(json: &str) {
    set_frame(json, |ui, frame| {
        if frame.operations.is_empty() {
            ui.overlay = None;
        } else {
            ui.overlay = Some(Arc::new(frame));
        }
    });
}

fn set_frame(json: &str, store: impl FnOnce(&mut UiState, Frame)) {
    if json.len() > MAX_FRAME_BYTES {
        log::warn!("Java GUI: rejected a frame larger than {MAX_FRAME_BYTES} bytes");
        return;
    }
    let parsed: Value = match serde_json::from_str(json) {
        Ok(value) => value,
        Err(error) => {
            log::warn!("Java GUI: invalid frame JSON: {error}");
            return;
        }
    };
    let Some(operations) = parsed.as_array() else {
        log::warn!("Java GUI: frame root is not an operation array");
        return;
    };
    if operations.len() > MAX_FRAME_OPS {
        log::warn!("Java GUI: rejected a frame with {} operations", operations.len());
        return;
    }
    let mut frame = Frame::default();
    let mut frame_image_bytes = 0usize;
    for operation in operations {
        let Some(kind) = operation.get("op").and_then(Value::as_str) else { continue };
        if kind == "image" {
            let Some(name) = operation.get("name").and_then(Value::as_str) else { continue };
            let Some(width) = operation.get("width").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()) else { continue };
            let Some(height) = operation.get("height").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()) else { continue };
            let Some(encoded) = operation.get("rgba").and_then(Value::as_str) else { continue };
            if width == 0 || height == 0 || width > 2048 || height > 2048 { continue; }
            let expected = width as usize * height as usize * 4;
            if expected > MAX_DYNAMIC_IMAGE_BYTES || frame_image_bytes.saturating_add(expected) > MAX_FRAME_IMAGE_BYTES { continue; }
            let Ok(rgba) = STANDARD.decode(encoded) else { continue };
            if rgba.len() != expected { continue; }
            frame_image_bytes = frame_image_bytes.saturating_add(expected);
            frame.images.insert(name.to_owned(), DynamicImage { width, height, rgba: Arc::new(rgba) });
            continue;
        }
        frame.operations.push(operation.clone());
    }
    let mut ui = UI.lock().unwrap_or_else(|e| e.into_inner());
    for (name, image) in frame.images.drain() {
        ui.dynamic_images.insert(name, image);
    }
    let mut total = ui.dynamic_images.values().map(|image| image.rgba.len()).sum::<usize>();
    while total > 32 * 1024 * 1024 || ui.dynamic_images.len() > 128 {
        let Some(name) = ui.dynamic_images.keys().next().cloned() else { break };
        if let Some(old) = ui.dynamic_images.remove(&name) { total = total.saturating_sub(old.rgba.len()); }
    }
    frame.images = ui.dynamic_images.clone();
    store(&mut ui, frame);
}

pub(in crate::game) fn draw_hud(game: &mut super::Game, batch: &mut HudBatch) {
    let hud = UI.lock().unwrap_or_else(|e| e.into_inner()).hud.clone();
    if let Some(frame) = hud.as_deref() {
        draw_frame(game, batch, frame);
    }
}

pub(in crate::game) fn draw_screens(game: &mut super::Game, batch: &mut HudBatch, inventory_visible: bool) {
    let (screen, overlay, uuid, screen_open, sent_size) = {
        let ui = UI.lock().unwrap_or_else(|e| e.into_inner());
        (ui.screen.clone(), ui.overlay.clone(), ui.local_uuid.clone(), ui.screen_open, ui.sent_size)
    };
    if screen_open {
        if let Some(frame) = screen.as_deref() {
            draw_frame(game, batch, frame);
        }
        if let Some(uuid) = uuid.as_deref() {
            let now = [batch.width.max(1.0) as u32, batch.height.max(1.0) as u32];
            if sent_size != Some(now) {
                send_size(uuid, now);
            }
        }
    }
    if inventory_visible {
        if let Some(frame) = overlay.as_deref() {
            draw_frame(game, batch, frame);
        }
    }
}

fn send_size(uuid: &str, size: [u32; 2]) {
    let json = format!(r#"{{"kind":"size","width":{},"height":{}}}"#, size[0], size[1]);
    if let Err(error) = horizon_jvm::events::screen_input(uuid, &json) {
        log::debug!("Java GUI: couldn't sync screen size: {error}");
    }
    UI.lock().unwrap_or_else(|e| e.into_inner()).sent_size = Some(size);
}

pub(in crate::game) fn input(json: &str) -> bool {
    let uuid = UI.lock().unwrap_or_else(|e| e.into_inner()).local_uuid.clone();
    let Some(uuid) = uuid else { return false };
    if let Err(error) = horizon_jvm::events::screen_input(&uuid, json) {
        log::warn!("Java GUI: input callback failed: {error}");
        return false;
    }
    true
}

pub(in crate::game) fn key_code(code: winit::keyboard::KeyCode) -> Option<i32> {
    use winit::keyboard::KeyCode as K;
    Some(match code {
        K::KeyA => 65, K::KeyB => 66, K::KeyC => 67, K::KeyD => 68, K::KeyE => 69,
        K::KeyF => 70, K::KeyG => 71, K::KeyH => 72, K::KeyI => 73, K::KeyJ => 74,
        K::KeyK => 75, K::KeyL => 76, K::KeyM => 77, K::KeyN => 78, K::KeyO => 79,
        K::KeyP => 80, K::KeyQ => 81, K::KeyR => 82, K::KeyS => 83, K::KeyT => 84,
        K::KeyU => 85, K::KeyV => 86, K::KeyW => 87, K::KeyX => 88, K::KeyY => 89, K::KeyZ => 90,
        K::Digit0 => 48, K::Digit1 => 49, K::Digit2 => 50, K::Digit3 => 51, K::Digit4 => 52,
        K::Digit5 => 53, K::Digit6 => 54, K::Digit7 => 55, K::Digit8 => 56, K::Digit9 => 57,
        K::Space => 32, K::Apostrophe => 39, K::Comma => 44, K::Minus => 45, K::Period => 46,
        K::Slash => 47, K::Semicolon => 59, K::Equal => 61, K::BracketLeft => 91,
        K::Backslash => 92, K::BracketRight => 93, K::Backquote => 96,
        K::Escape => 256, K::Enter => 257, K::Tab => 258, K::Backspace => 259,
        K::Insert => 260, K::Delete => 261, K::ArrowRight => 262, K::ArrowLeft => 263,
        K::ArrowDown => 264, K::ArrowUp => 265, K::PageUp => 266, K::PageDown => 267,
        K::Home => 268, K::End => 269, K::CapsLock => 280, K::ScrollLock => 281,
        K::NumLock => 282, K::PrintScreen => 283, K::Pause => 284,
        K::F1 => 290, K::F2 => 291, K::F3 => 292, K::F4 => 293, K::F5 => 294,
        K::F6 => 295, K::F7 => 296, K::F8 => 297, K::F9 => 298, K::F10 => 299,
        K::F11 => 300, K::F12 => 301, K::F13 => 302, K::F14 => 303, K::F15 => 304,
        K::F16 => 305, K::F17 => 306, K::F18 => 307, K::F19 => 308, K::F20 => 309,
        K::F21 => 310, K::F22 => 311, K::F23 => 312, K::F24 => 313, K::F25 => 314,
        K::Numpad0 => 320, K::Numpad1 => 321, K::Numpad2 => 322, K::Numpad3 => 323,
        K::Numpad4 => 324, K::Numpad5 => 325, K::Numpad6 => 326, K::Numpad7 => 327,
        K::Numpad8 => 328, K::Numpad9 => 329, K::NumpadDecimal => 330,
        K::NumpadDivide => 331, K::NumpadMultiply => 332, K::NumpadSubtract => 333,
        K::NumpadAdd => 334, K::NumpadEnter => 335, K::NumpadEqual => 336,
        K::ShiftLeft => 340, K::ControlLeft => 341, K::AltLeft => 342, K::SuperLeft => 343,
        K::ShiftRight => 344, K::ControlRight => 345, K::AltRight => 346, K::SuperRight => 347,
        K::ContextMenu => 348,
        _ => return None,
    })
}

pub(in crate::game) fn mouse_input(x: f32, y: f32, button: i32, mods: u32, pressed: bool) -> bool {
    let kind = if pressed { "press" } else { "release" };
    let sent = input(&format!(r#"{{"kind":"{kind}","x":{x},"y":{y},"button":{button},"mods":{mods}}}"#));
    let mut ui = UI.lock().unwrap_or_else(|e| e.into_inner());
    ui.pointer = [x, y];
    if pressed { ui.drag_button = Some(button); }
    else if ui.drag_button == Some(button) { ui.drag_button = None; }
    sent
}

pub(in crate::game) fn mouse_move(x: f32, y: f32) -> bool {
    let (button, dx, dy) = {
        let mut ui = UI.lock().unwrap_or_else(|e| e.into_inner());
        let dx = x - ui.pointer[0];
        let dy = y - ui.pointer[1];
        ui.pointer = [x, y];
        (ui.drag_button, dx, dy)
    };
    if let Some(button) = button {
        input(&format!(r#"{{"kind":"drag","x":{x},"y":{y},"button":{button},"dx":{dx},"dy":{dy}}}"#))
    } else {
        input(&format!(r#"{{"kind":"move","x":{x},"y":{y}}}"#))
    }
}

pub(in crate::game) fn mouse_scroll(x: f32, y: f32, horizontal: f64, vertical: f64) -> bool {
    input(&format!(r#"{{"kind":"scroll","x":{x},"y":{y},"h":{horizontal},"v":{vertical}}}"#))
}

pub(in crate::game) fn keyboard_input(key: i32, mods: u32, pressed: bool) -> bool {
    input(&format!(r#"{{"kind":"key","key":{key},"keycode":{key},"mods":{mods},"down":{pressed}}}"#))
}

pub(in crate::game) fn character_input(character: char) -> bool {
    input(&format!(r#"{{"kind":"char","char":{}}}"#, character as u32))
}

fn draw_frame(game: &mut super::Game, batch: &mut HudBatch, frame: &Frame) {
    let mut scissors: Vec<[f32; 4]> = Vec::new();
    for operation in &frame.operations {
        let Some(kind) = operation.get("op").and_then(Value::as_str) else { continue };
        if kind == "scissor" {
            if let Some(mut next) = rect(operation).map(|rect| transformed_rect(rect, operation.get("pose"))) {
                if let Some(parent) = scissors.last().copied() {
                    next = [next[0].max(parent[0]), next[1].max(parent[1]), next[2].min(parent[2]), next[3].min(parent[3])];
                    if next[2] < next[0] { next[2] = next[0]; }
                    if next[3] < next[1] { next[3] = next[1]; }
                }
                scissors.push(next);
            }
            continue;
        }
        if kind == "unscissor" {
            scissors.pop();
            continue;
        }
        let start = batch.vertices.len();
        draw_operation(game, batch, operation, frame);
        transform_since(batch, start, operation.get("pose"));
        if let Some([x0, y0, x1, y1]) = scissors.last().copied() {
            batch.clip_since(start, x0, y0, x1, y1);
        }
    }
}

fn draw_operation(game: &mut super::Game, batch: &mut HudBatch, op: &Value, frame: &Frame) {
    let kind = op.get("op").and_then(Value::as_str).unwrap_or("");
    match kind {
        "fill" => {
            if let Some([x0, y0, x1, y1]) = rect(op) {
                batch.fill(&game.gui.atlas, x0, y0, x1 - x0, y1 - y0, color(op.get("color")));
            }
        }
        "gradient" => {
            if let Some([x0, y0, x1, y1]) = rect(op) {
                batch.fill_gradient(&game.gui.atlas, (x0, y0, x1 - x0, y1 - y0), color(op.get("top")), color(op.get("bottom")));
            }
        }
        "text" => {
            let text = op.get("text").and_then(Value::as_str)
                .or_else(|| op.get("json").and_then(Value::as_str).map(|v| v));
            if let (Some(text), Some(x), Some(y)) = (text, number(op.get("x")), number(op.get("y"))) {
                let text = if op.get("json").and_then(Value::as_str).is_some() {
                    component_text(text)
                } else { text.to_owned() };
                let shadow = op.get("shadow").and_then(Value::as_bool).unwrap_or(false);
                batch.text(&game.gui.atlas, &text, x, y, color(op.get("color")), shadow);
            }
        }
        "sprite" => {
            let sprite = op.get("sprite").and_then(Value::as_str);
            if let (Some(sprite), Some([x0, y0, x1, y1])) = (sprite, rect(op)) {
                if let Some(mut sprite_rect) = game.gui.atlas.inline_sprite("minecraft:gui", sprite) {
                    let u0 = number(op.get("u0")).unwrap_or(0.0).clamp(0.0, 1.0);
                    let u1 = number(op.get("u1")).unwrap_or(1.0).clamp(0.0, 1.0);
                    let v0 = number(op.get("v0")).unwrap_or(0.0).clamp(0.0, 1.0);
                    let v1 = number(op.get("v1")).unwrap_or(1.0).clamp(0.0, 1.0);
                    let du = sprite_rect.u1 - sprite_rect.u0;
                    let dv = sprite_rect.v1 - sprite_rect.v0;
                    let base_u = sprite_rect.u0;
                    let base_v = sprite_rect.v0;
                    sprite_rect.u0 = base_u + du * u0;
                    sprite_rect.u1 = base_u + du * u1;
                    sprite_rect.v0 = base_v + dv * v0;
                    sprite_rect.v1 = base_v + dv * v1;
                    batch.rect_uv(x0, y0, x1 - x0, y1 - y0, sprite_rect, color_with_alpha(op.get("color"), op.get("alpha")));
                }
            }
        }
        "blit" => {
            if let (Some(texture), Some([x0, y0, x1, y1])) = (op.get("texture").and_then(Value::as_str), rect(op)) {
                let uv = [
                    number(op.get("u0")).unwrap_or(0.0),
                    number(op.get("u1")).unwrap_or(1.0),
                    number(op.get("v0")).unwrap_or(0.0),
                    number(op.get("v1")).unwrap_or(1.0),
                ];
                let tint = color_with_alpha(op.get("color"), op.get("alpha"));
                if let Some(image) = frame.images.get(texture) {
                    batch.picture_rgba_region(&game.gui.atlas, texture, image.width, image.height, image.rgba.as_slice(), x0, y0, x1 - x0, y1 - y0, uv, tint);
                } else if let Some(name) = texture_name(texture) {
                    batch.picture_region(&game.gui.atlas, &name, x0, y0, x1 - x0, y1 - y0, uv, tint);
                }
            }
        }
        "item" | "decorations" | "item_tooltip" => {
            let Some(snbt) = op.get("item").and_then(Value::as_str) else { return };
            let Some(stack) = game.item_from_snbt(snbt) else { return };
            let (Some(x), Some(y)) = (number(op.get("x")), number(op.get("y"))) else { return };
            match kind {
                "item" => game.draw_item(batch, &stack, x, y),
                "decorations" => {
                    let count = op.get("count").and_then(Value::as_str).map(str::to_owned);
                    game.draw_item_decorations(batch, &stack, x, y, count);
                }
                "item_tooltip" => game.draw_stack_tooltip(batch, &stack, x, y),
                _ => {}
            }
        }
        "tooltip" => {
            let Some(lines) = op.get("lines").and_then(Value::as_array) else { return };
            let (Some(x), Some(y)) = (number(op.get("x")), number(op.get("y"))) else { return };
            let lines: Vec<(String, u32)> = lines.iter().map(|line| {
                let json = line.to_string();
                (component_text(&json), u32::MAX)
            }).collect();
            game.draw_tooltip_lines(batch, &lines, x, y);
        }
        "typing" | "image" => {}
        _ => {}
    }
}

fn transformed_rect(rect: [f32; 4], pose: Option<&Value>) -> [f32; 4] {
    let Some(values) = pose.and_then(Value::as_array) else { return rect };
    if values.len() != 6 { return rect; }
    let Some(matrix) = values.iter().map(|value| value.as_f64().filter(|n| n.is_finite()).map(|n| n.clamp(-100_000.0, 100_000.0) as f32)).collect::<Option<Vec<_>>>() else { return rect };
    let points = [
        [rect[0], rect[1]], [rect[2], rect[1]],
        [rect[0], rect[3]], [rect[2], rect[3]],
    ];
    let mut out = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
    for [x, y] in points {
        let x = matrix[0] * x + matrix[2] * y + matrix[4];
        let y = matrix[1] * x + matrix[3] * y + matrix[5];
        out[0] = out[0].min(x);
        out[1] = out[1].min(y);
        out[2] = out[2].max(x);
        out[3] = out[3].max(y);
    }
    out
}

fn transform_since(batch: &mut HudBatch, start: usize, pose: Option<&Value>) {
    let Some(values) = pose.and_then(Value::as_array) else { return };
    if values.len() != 6 { return; }
    let mut m = [0.0f32; 6];
    for (i, v) in values.iter().enumerate() {
        let Some(n) = v.as_f64().filter(|n| n.is_finite()) else { return };
        m[i] = n.clamp(-100_000.0, 100_000.0) as f32;
    }
    for vertex in &mut batch.vertices[start..] {
        let [x, y] = vertex.pos;
        vertex.pos = [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
    }
}

fn rect(op: &Value) -> Option<[f32; 4]> {
    let r = [number(op.get("x0"))?, number(op.get("y0"))?, number(op.get("x1"))?, number(op.get("y1"))?];
    r.iter().all(|v| v.is_finite() && v.abs() < 1_000_000.0).then_some(r)
}

fn number(value: Option<&Value>) -> Option<f32> {
    value.and_then(Value::as_f64).filter(|v| v.is_finite()).map(|v| v.clamp(-1_000_000.0, 1_000_000.0) as f32)
}

fn color(value: Option<&Value>) -> u32 {
    color_with_alpha(value, None)
}

fn color_with_alpha(value: Option<&Value>, alpha: Option<&Value>) -> u32 {
    let mut argb = value.and_then(Value::as_i64).map(|v| v as u32).unwrap_or(u32::MAX);
    if let Some(alpha) = alpha.and_then(Value::as_f64).filter(|v| v.is_finite()) {
        let factor = alpha.clamp(0.0, 1.0);
        let old = ((argb >> 24) & 0xff) as f64;
        argb = (argb & 0x00ff_ffff) | (((old * factor).round() as u32) << 24);
    }
    (argb & 0xff00_ff00) | ((argb & 0x00ff_0000) >> 16) | ((argb & 0x0000_00ff) << 16)
}

fn texture_name(texture: &str) -> Option<String> {
    let (namespace, path) = texture.split_once(':').unwrap_or(("minecraft", texture));
    let path = path.strip_prefix("textures/").unwrap_or(path);
    let path = path.strip_suffix(".png").unwrap_or(path);
    let namespace_valid = !namespace.is_empty()
        && namespace.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'.'));
    let path_valid = !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\0')
        && !path.contains('\\')
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
        && path.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'.' | b'/'));
    (namespace_valid && path_valid).then(|| format!("{namespace}:{path}"))
}

fn component_text(json: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(json) else { return json.to_owned() };
    fn style_codes(style: &Value) -> String {
        let mut out = String::new();
        if let Some(color) = style.get("color").and_then(Value::as_str) {
            let code = match color {
                "black" => Some('0'), "dark_blue" => Some('1'), "dark_green" => Some('2'),
                "dark_aqua" => Some('3'), "dark_red" => Some('4'), "dark_purple" => Some('5'),
                "gold" => Some('6'), "gray" => Some('7'), "dark_gray" => Some('8'),
                "blue" => Some('9'), "green" => Some('a'), "aqua" => Some('b'),
                "red" => Some('c'), "light_purple" => Some('d'), "yellow" => Some('e'),
                "white" => Some('f'), _ => None,
            };
            if let Some(code) = code { out.push('§'); out.push(code); }
            else if color.starts_with('#') && color.len() == 7 && color.is_ascii() {
                out.push_str("§x");
                for digit in color[1..].chars() { out.push('§'); out.push(digit); }
            }
        }
        for (key, code) in [("obfuscated", 'k'), ("bold", 'l'), ("strikethrough", 'm'), ("underlined", 'n'), ("italic", 'o')] {
            if style.get(key).and_then(Value::as_bool) == Some(true) { out.push('§'); out.push(code); }
        }
        out
    }
    fn append(value: &Value, out: &mut String, inherited: &str) {
        match value {
            Value::String(text) => {
                if !inherited.is_empty() { out.push_str(inherited); }
                out.push_str(text);
            }
            Value::Array(items) => for item in items { append(item, out, inherited); },
            Value::Object(map) => {
                let own = style_codes(value);
                let active = if own.is_empty() { inherited.to_owned() } else { format!("{inherited}{own}") };
                if let Some(text) = map.get("text") {
                    append(text, out, &active);
                } else if let Some(key) = map.get("translate").and_then(Value::as_str) {
                    let mut translated = horizon_client::lang::get_or(key, key);
                    if let Some(arguments) = map.get("with").and_then(Value::as_array) {
                        for argument in arguments {
                            let mut rendered = String::new();
                            append(argument, &mut rendered, &active);
                            if let Some(position) = translated.find("%s") {
                                translated.replace_range(position..position + 2, &rendered);
                            }
                        }
                    }
                    if !active.is_empty() { out.push_str(&active); }
                    out.push_str(&translated);
                } else if let Some(selector) = map.get("selector").and_then(Value::as_str) {
                    if !active.is_empty() { out.push_str(&active); }
                    out.push_str(selector);
                } else if let Some(keybind) = map.get("keybind").and_then(Value::as_str) {
                    if !active.is_empty() { out.push_str(&active); }
                    out.push_str(keybind);
                }
                if let Some(extra) = map.get("extra") { append(extra, out, &active); }
                if !active.is_empty() {
                    out.push_str("§r");
                    out.push_str(inherited);
                }
            }
            _ => {}
        }
    }
    let mut out = String::new();
    append(&value, &mut out, "");
    out
}


#[cfg(test)]
mod tests {
    use super::{Frame, UiState};
    use std::sync::Arc;

    #[test]
    fn closing_a_java_menu_releases_the_screen() {
        let mut ui = UiState {
            screen_open: true,
            menu_open: true,
            screen: Some(Arc::new(Frame::default())),
            sent_size: Some([800, 600]),
            drag_button: Some(0),
            ..UiState::default()
        };

        ui.set_menu_open(false);

        assert!(!ui.screen_open);
        assert!(ui.screen.is_none());
        assert!(ui.sent_size.is_none());
        assert!(ui.drag_button.is_none());
    }

    #[test]
    fn closing_a_menu_keeps_a_client_only_screen_open() {
        let mut ui = UiState {
            screen_open: true,
            menu_open: true,
            client_screen_open: true,
            screen: Some(Arc::new(Frame::default())),
            ..UiState::default()
        };

        ui.set_menu_open(false);

        assert!(ui.screen_open);
        assert!(ui.client_screen_open);
        assert!(ui.screen.is_some());
    }

    #[test]
    fn closing_a_client_screen_restores_its_open_menu() {
        let mut ui = UiState {
            screen_open: true,
            menu_open: true,
            client_screen_open: true,
            screen: Some(Arc::new(Frame::default())),
            sent_size: Some([800, 600]),
            ..UiState::default()
        };

        ui.set_client_screen_open(false);

        assert!(ui.screen_open);
        assert!(!ui.client_screen_open);
        assert!(ui.screen.is_none());
        assert!(ui.sent_size.is_none());
    }

    #[test]
    fn closing_a_client_screen_after_its_menu_closed_releases_input() {
        let mut ui = UiState {
            screen_open: true,
            client_screen_open: true,
            screen: Some(Arc::new(Frame::default())),
            ..UiState::default()
        };

        ui.set_client_screen_open(false);

        assert!(!ui.screen_open);
        assert!(ui.screen.is_none());
    }

    #[test]
    fn a_late_screen_frame_does_not_reopen_a_closed_screen() {
        let mut ui = UiState::default();

        ui.set_screen_frame(Frame::default());

        assert!(!ui.screen_open);
        assert!(ui.screen.is_some());
    }

    #[test]
    fn opening_a_new_menu_discards_a_stale_screen_frame() {
        let mut ui = UiState {
            screen: Some(Arc::new(Frame::default())),
            ..UiState::default()
        };

        ui.set_menu_open(true);

        assert!(ui.screen_open);
        assert!(ui.screen.is_none());
    }
}
