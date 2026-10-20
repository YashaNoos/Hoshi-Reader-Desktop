//! Linux build (0022): game controllers read with gilrs (evdev), on a thread of their own.
//!
//! WebKitGTK's Gamepad API only feeds a web view it considers focused, which does not
//! happen in Hoshi's window, and its controller library froze the app when a controller
//! reconnected. So the Linux build reads controllers here and sends their state to the
//! window ("controller-state") in the W3C standard layout; the frontend keeps the logic.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use gilrs::{Axis, Button, Gamepad, GamepadId, Gilrs};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Standard-layout order (Gamepad API button indices 0..=16).
const BUTTONS: [Button; 17] = [
    Button::South,
    Button::East,
    Button::West,
    Button::North,
    Button::LeftTrigger,
    Button::RightTrigger,
    Button::LeftTrigger2,
    Button::RightTrigger2,
    Button::Select,
    Button::Start,
    Button::LeftThumb,
    Button::RightThumb,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
    Button::Mode,
];

/// At most this often (stick movements send hundreds of events a second).
const MIN_INTERVAL: Duration = Duration::from_millis(8);

#[derive(Serialize, Clone, Default, PartialEq)]
pub struct ControllerState {
    name: Option<String>,
    buttons: Vec<f32>,
    /// Left X, left Y, right X, right Y; Y is positive downwards, as in the Gamepad API.
    axes: Vec<f32>,
}

static STATE: Mutex<Option<ControllerState>> = Mutex::new(None);
static STARTED: OnceLock<()> = OnceLock::new();

fn snapshot(gamepad: &Gamepad) -> ControllerState {
    let button = |button: Button| gamepad.button_data(button).map_or(0.0, |data| data.value());
    ControllerState {
        name: Some(gamepad.name().to_string()),
        buttons: BUTTONS.iter().map(|&b| button(b)).collect(),
        axes: vec![
            gamepad.value(Axis::LeftStickX),
            -gamepad.value(Axis::LeftStickY),
            gamepad.value(Axis::RightStickX),
            -gamepad.value(Axis::RightStickY),
        ],
    }
}

/// The controller in use: the one already chosen while it stays connected, else the first.
fn choose(gilrs: &Gilrs, active: Option<GamepadId>) -> Option<GamepadId> {
    active
        .filter(|&id| gilrs.connected_gamepad(id).is_some())
        .or_else(|| gilrs.gamepads().find(|(_, gamepad)| gamepad.is_connected()).map(|(id, _)| id))
}

fn publish(app: &AppHandle, gilrs: &Gilrs, active: Option<GamepadId>) -> ControllerState {
    let state = active.map_or_else(ControllerState::default, |id| snapshot(&gilrs.gamepad(id)));
    *STATE.lock().unwrap() = Some(state.clone());
    app.emit("controller-state", &state).ok();
    state
}

fn run(app: AppHandle) {
    let mut gilrs = match Gilrs::new() {
        Ok(gilrs) => gilrs,
        Err(gilrs::Error::NotImplemented(gilrs)) => gilrs,
        Err(error) => {
            eprintln!("controller: {error}");
            return;
        }
    };
    let mut active = choose(&gilrs, None);
    let mut sent = publish(&app, &gilrs, active);
    let mut last_emit = Instant::now();
    loop {
        // gilrs updates a controller's state as its events are taken from the queue.
        if gilrs.next_event_blocking(Some(Duration::from_secs(1))).is_none() {
            continue;
        }
        let wait = MIN_INTERVAL.saturating_sub(last_emit.elapsed());
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
        while gilrs.next_event().is_some() {}
        active = choose(&gilrs, active);
        let state = active.map_or_else(ControllerState::default, |id| snapshot(&gilrs.gamepad(id)));
        if state != sent {
            sent = publish(&app, &gilrs, active);
            last_emit = Instant::now();
        }
    }
}

/// Starts reading controllers (once) and returns the current state.
#[tauri::command]
pub fn controller_state(app: AppHandle) -> ControllerState {
    STARTED.get_or_init(|| {
        std::thread::Builder::new()
            .name("controller".into())
            .spawn(move || run(app))
            .ok();
    });
    STATE.lock().unwrap().clone().unwrap_or_default()
}
