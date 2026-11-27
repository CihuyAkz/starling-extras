use super::*;
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct JavaTitle {
    ops: Vec<Value>,
    images: java_gui::TitleImages,
    custom: bool,
    failed: bool,
    next_frame: Option<Instant>,
    last_cursor: Option<(f32, f32)>,
    last_activity: Option<Instant>,
    pause: bool,
    empty_since: Option<Instant>,
}

impl Game<'_> {
    pub(super) fn java_title_scripted_key(&mut self, key: KeyCode, action: &str) -> bool {
        if !self.java_title_available() {
            return false;
        }
        let Some(code) = sdl_key(key) else {
            return false;
        };
        let send = |game: &mut Self, action: i32| {
            game.java_title_exchange(json!({
                "kind": "key",
                "key": code,
                "keycode": sdl_keycode(key, code),
                "action": action,
                "mods": 0,
                "text": ""
            }))
        };
        let handled = send(
            self,
            if action == "up" {
                0
            } else if action == "repeat" {
                2
            } else {
                1
            },
        );
        if action == "tap" {
            send(self, 0);
        }
        !self.java_pause_available() || handled
    }

    fn java_title_available(&self) -> bool {
        (self.java_pause_available()
            || self
                .title
                .as_ref()
                .is_some_and(|title| matches!(title.mode, horizon_client::title::Mode::Main)))
            && self.screen != Screen::Options
            && !self.java_title.failed
            && horizon_jvm::events::ready()
    }

    fn java_pause_available(&self) -> bool {
        self.screen == Screen::Paused
            && self.pause_menu
            && self.player.is_some()
            && self.pauses_game()
            && self.is_paused()
            && (self.java_title.pause || !self.streaming.terrain_loading())
    }

    pub(super) fn close_java_pause_if_hidden(&mut self) {
        if self.java_title.pause && !self.java_pause_available() {
            let _ = horizon_jvm::events::title_ui(r#"{"kind":"close"}"#);
            self.java_title = JavaTitle::default();
        }
    }

    fn java_title_exchange(&mut self, mut input: Value) -> bool {
        if matches!(input["kind"].as_str(), Some("press" | "release" | "scroll" | "key")) {
            self.java_title.last_activity = Some(Instant::now());
            self.java_title.next_frame = None;
        }
        input["pause"] = json!(self.java_pause_available());
        let size = self
            .renderer
            .as_ref()
            .map_or([1280, 720], |renderer| [renderer.config.width, renderer.config.height]);
        let batch = HudBatch::new(size, self.settings.gui_scale_inputs());
        input["width"] = json!(batch.width as i32);
        input["height"] = json!(batch.height as i32);
        input["pixelWidth"] = json!(size[0]);
        input["pixelHeight"] = json!(size[1]);
        input["guiScale"] = json!(batch.scale as i32);
        input["x"] = json!(self.cursor.0);
        input["y"] = json!(self.cursor.1);
        match horizon_jvm::events::title_ui(&input.to_string()) {
            Ok((frame, images)) => {
                if frame.len() > 28 * 1024 * 1024 {
                    return false;
                }
                let Ok(mut frame) = serde_json::from_str::<Value>(&frame) else {
                    return false;
                };
                if let Some(Value::Array(ops)) = frame.get_mut("ops").map(Value::take) {
                    if !java_gui::title_ops_valid(&ops) {
                        return false;
                    }
                    if self.java_title.pause && ops.is_empty() && !self.java_title.ops.is_empty() {
                        self.java_title.empty_since.get_or_insert_with(Instant::now);
                    } else {
                        self.java_title.ops = ops;
                        self.java_title.empty_since = None;
                    }
                }
                if frame.get("screenChanged").and_then(Value::as_bool) == Some(true) {
                    self.java_title.ops.clear();
                    self.java_title.images.clear();
                    self.java_title.empty_since = None;
                    self.java_title.next_frame = None;
                }
                java_gui::store_title_images(&mut self.java_title.images, images);
                if let Some(custom) = frame.get("custom").and_then(Value::as_bool) {
                    self.java_title.custom = custom;
                }
                frame.get("taken").and_then(Value::as_bool).unwrap_or(false)
            }
            Err(error) => {
                log::warn!("Java mods: title-screen session failed: {error}");
                self.java_title = JavaTitle {
                    failed: true,
                    pause: self.java_pause_available(),
                    ..Default::default()
                };
                false
            }
        }
    }

    pub(super) fn draw_java_title(&mut self, batch: &mut HudBatch) {
        if !self.java_title_available() {
            return;
        }
        let pause = self.java_pause_available();
        if pause != self.java_title.pause {
            self.java_title = JavaTitle {
                pause,
                ..Default::default()
            };
        }
        let now = Instant::now();
        if self.java_title.last_cursor != Some(self.cursor) {
            self.java_title.last_cursor = Some(self.cursor);
            if self
                .java_title
                .last_activity
                .is_none_or(|at| now.duration_since(at) >= Duration::from_millis(150))
            {
                self.java_title.next_frame = None;
            }
            self.java_title.last_activity = Some(now);
        }
        let interactive = self.java_title.custom
            || self
                .java_title
                .last_activity
                .is_some_and(|at| now.duration_since(at) < Duration::from_millis(150));
        let interval = if pause && interactive {
            Duration::from_nanos(1_000_000_000 / 120)
        } else {
            Duration::from_millis(16)
        };
        let due = self.java_title.next_frame.unwrap_or(now);
        if now >= due {
            let next = due + interval;
            self.java_title.next_frame = Some(if next > now { next } else { now + interval });
            self.java_title_exchange(json!({
                "kind": "frame",
                "occupied": horizon_client::widgets::current_button_bounds()
            }));
        }
        if self.java_title.custom {
            batch.vertices.truncate(batch.blur_at as usize);
            batch.blends.retain_mut(|(range, _)| {
                range.end = range.end.min(batch.blur_at);
                range.start < range.end
            });
        }
        if self
            .java_title
            .empty_since
            .is_some_and(|since| now.saturating_duration_since(since) >= Duration::from_millis(120))
        {
            self.java_title.ops.clear();
            self.java_title.empty_since = None;
        }
        let ops = std::mem::take(&mut self.java_title.ops);
        let images = self.java_title.images.clone();
        java_gui::draw_title_ops(self, batch, &ops, &images);
        self.java_title.ops = ops;
    }

    pub(super) fn java_title_custom(&self) -> bool {
        self.java_title.custom && self.java_title_available()
    }

    pub(super) fn java_title_input(&mut self, event: &WindowEvent) -> bool {
        if !self.java_title_available() || java_gui::screen_open() {
            return false;
        }
        let input = match event {
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => 1,
                    MouseButton::Middle => 2,
                    MouseButton::Right => 3,
                    MouseButton::Back => 4,
                    MouseButton::Forward => 5,
                    _ => return self.java_title.custom,
                };
                let mods = i32::from(self.has_shift_down()) * 3
                    | i32::from(self.has_control_down()) * 192;
                json!({
                    "kind": if *state == ElementState::Pressed { "press" } else { "release" },
                    "button": button,
                    "mods": mods
                })
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (horizontal, vertical) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (f64::from(*x), f64::from(*y)),
                    MouseScrollDelta::PixelDelta(position) => (position.x / 40.0, position.y / 40.0),
                };
                json!({ "kind": "scroll", "h": horizontal, "v": vertical })
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(key) = event.physical_key else {
                    return true;
                };
                let Some(code) = sdl_key(key) else {
                    return true;
                };
                let mods = i32::from(self.has_shift_down()) * 3
                    | i32::from(self.has_control_down()) * 192;
                let action = if event.state == ElementState::Released {
                    0
                } else if event.repeat {
                    2
                } else {
                    1
                };
                json!({
                    "kind": "key",
                    "action": action,
                    "key": code,
                    "keycode": sdl_keycode(key, code),
                    "mods": mods,
                    "text": event.text.as_deref().unwrap_or("")
                })
            }
            _ => return false,
        };
        self.java_title_exchange(input)
    }
}

fn sdl_keycode(key: KeyCode, scancode: i32) -> i32 {
    use KeyCode as K;
    if let Some(text) = key_text(key) {
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
        _ => scancode | 1 << 30,
    }
}

fn key_text(key: KeyCode) -> Option<String> {
    if key == KeyCode::Space {
        return Some(" ".into());
    }
    let name = format!("{key:?}");
    let name = name
        .strip_prefix("Key")
        .or_else(|| name.strip_prefix("Digit"))
        .filter(|rest| rest.len() == 1)?;
    Some(name.to_lowercase())
}

fn sdl_key(key: KeyCode) -> Option<i32> {
    use KeyCode as K;
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
    let at = |keys: &[KeyCode], base: i32| {
        keys.iter().position(|candidate| *candidate == key).map(|index| base + index as i32)
    };
    if let Some(code) = at(&letters, 4)
        .or_else(|| at(&digits, 30))
        .or_else(|| at(&numpad, 89))
        .or_else(|| at(&functions, 58))
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
