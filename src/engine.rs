//! The platform independent core of mouseless: it receives key events and produces outputs
//! (key presses, mouse movements, ...).
//!
//! This is a port of the event handler chain of the original mouseless:
//! ComboHandler -> ModLayerHandler -> TapHoldHandler -> DefaultHandler -> OneShotHandler -> Executor
//!
//! Instead of goroutines and timers, the engine is driven by two entry points:
//! - [`Engine::handle_event`] for every physical key press or release
//! - [`Engine::on_timer`] which must be called at [`Engine::next_deadline`]
//!
//! Both return the list of outputs that have to be executed by the platform layer.

use crate::config::{Binding, Config, Layer, MouseButton, TapHoldBinding};
use crate::keys::{self, KEY_ESC, WILDCARD};
use crate::{debug, warn};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Event {
    pub code: u16,
    pub is_press: bool,
    pub time: Instant,
}

#[derive(Clone, Debug)]
struct EventBinding {
    event: Event,
    binding: Option<Binding>,
}

impl EventBinding {
    fn new(event: Event) -> Self {
        EventBinding { event, binding: None }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    Key { code: u16, down: bool },
    MouseMove { dx: i32, dy: i32 },
    /// Scroll by the given delta, where 120 is one wheel click (positive is up or right).
    Wheel { horizontal: bool, delta: i32 },
    Button { button: MouseButton, down: bool },
    Exec { command: String, env: Vec<(String, String)> },
}

pub struct Engine {
    config: Config,
    out: Vec<Output>,
    combo: ComboHandler,
    mod_layer: ModLayerHandler,
    tap_hold: TapHoldHandler,
    one_shot: OneShotHandler,
    executor: Executor,
    keyboard: VirtualKeyboard,
    mouse: VirtualMouse,
    /// set when the reload-config action was executed
    pub reload_requested: bool,
    /// set when the quit action was executed
    pub quit_requested: bool,
}

impl Engine {
    pub fn new(config: Config) -> Self {
        let mouse = VirtualMouse::new(&config);
        Engine {
            config,
            out: Vec::new(),
            combo: ComboHandler::default(),
            mod_layer: ModLayerHandler::default(),
            tap_hold: TapHoldHandler::default(),
            one_shot: OneShotHandler::default(),
            executor: Executor::default(),
            keyboard: VirtualKeyboard::default(),
            mouse,
            reload_requested: false,
            quit_requested: false,
        }
    }

    /// Applies a new config. Like in the original, this resets all handlers and the current layer,
    /// but keeps the state of the virtual keyboard and mouse.
    pub fn set_config(&mut self, config: Config) {
        self.mouse.set_config(&config);
        self.config = config;
        self.combo = ComboHandler::default();
        self.mod_layer = ModLayerHandler::default();
        self.tap_hold = TapHoldHandler::default();
        self.one_shot = OneShotHandler::default();
        self.executor = Executor::default();
    }

    /// Handles a physical key press or release.
    pub fn handle_event(&mut self, event: Event) -> Vec<Output> {
        self.combo_handle(EventBinding::new(event));
        std::mem::take(&mut self.out)
    }

    /// Handles an auto-repeat of a physical key that is held down.
    ///
    /// On Windows, keys pressed with SendInput are not repeated automatically, so the repeat events of
    /// the physical key are used to repeat the key that it is mapped to.
    pub fn handle_repeat(&mut self, code: u16) -> Vec<Output> {
        if !self.combo.queue.is_empty() || !self.tap_hold.queue.is_empty() {
            return Vec::new();
        }
        if let Some(codes) = self.keyboard.triggered_keys.get(&code) {
            if let Some(&c) = codes.iter().rev().find(|c| self.keyboard.is_pressed.contains(c)) {
                self.out.push(Output::Key { code: c, down: true });
            }
        }
        std::mem::take(&mut self.out)
    }

    /// Returns the next point in time at which [`Engine::on_timer`] has to be called.
    pub fn next_deadline(&self) -> Option<Instant> {
        [self.combo.deadline, self.tap_hold.deadline, self.mouse.next_deadline()]
            .into_iter()
            .flatten()
            .min()
    }

    /// Processes timeouts and mouse movement.
    pub fn on_timer(&mut self, now: Instant) -> Vec<Output> {
        // resolving a timeout may start a new one which is already expired (e.g. a second tap-hold key
        // that was pressed before the first one timed out), so repeat until nothing is expired
        loop {
            if self.combo.deadline.is_some_and(|d| d <= now) {
                debug!("ComboHandler: timed out");
                self.combo.state = ComboState::NoCombo;
                self.combo_resolved();
                self.combo_process();
            } else if self.tap_hold.deadline.is_some_and(|d| d <= now) {
                debug!("TapHoldHandler: tapHold timed out");
                self.tap_hold.state = TapHoldState::Hold;
                self.tap_hold_resolve();
                self.tap_hold_process();
            } else {
                break;
            }
        }
        self.mouse.on_timer(now, &mut self.out);
        std::mem::take(&mut self.out)
    }

    /// Releases all keys and buttons that are currently pressed, used on exit.
    pub fn release_all(&mut self) -> Vec<Output> {
        let keys: Vec<u16> = self.keyboard.is_pressed.iter().copied().collect();
        for k in keys {
            self.keyboard.release_key(k, &mut self.out);
        }
        let buttons: Vec<MouseButton> = self.mouse.is_button_pressed.drain().collect();
        for b in buttons {
            self.out.push(Output::Button { button: b, down: false });
        }
        std::mem::take(&mut self.out)
    }

    // ---- layer management ----

    fn current_layer(&self) -> &Layer {
        &self.config.layers[self.executor.current_layer]
    }

    fn get_layer(&self, name: &str) -> Option<usize> {
        self.config.layers.iter().position(|l| l.name == name)
    }

    // ---------------------------------------------------------------------------------------------
    // ComboHandler
    // ---------------------------------------------------------------------------------------------

    fn combo_handle(&mut self, eb: EventBinding) {
        self.combo.queue.push(eb);
        self.combo_process();
    }

    fn combo_process(&mut self) {
        while self.combo.pos < self.combo.queue.len() {
            self.combo_next();
        }
    }

    fn combo_next(&mut self) {
        let pos = self.combo.pos;
        let eb = &self.combo.queue[pos];
        let event = eb.event;
        debug!("ComboHandler: handling Event: {:?}", eb);

        let combo_bindings = if eb.binding.is_none() {
            self.current_layer().combo_bindings.get(&event.code).cloned()
        } else {
            None
        };

        if event.is_press {
            if let Some(cb) = combo_bindings {
                if self.combo.state != ComboState::Wait {
                    debug!("ComboHandler: waiting");
                    self.combo.state = ComboState::Wait;
                    self.combo.bindings = cb;
                    self.combo.deadline = Some(event.time + Duration::from_millis(self.config.combo_time_ms));
                }
            }
        } else if self.combo.state == ComboState::Wait && self.combo.queue[0].event.code == event.code {
            // don't execute combo if the first key is released
            self.combo.state = ComboState::NoCombo;
        }

        // if another key is pressed, check if it is a combo
        if self.combo.state == ComboState::Wait && event.code != self.combo.queue[0].event.code {
            if event.is_press {
                if let Some(b) = self.combo.bindings.get(&event.code).cloned() {
                    self.combo.queue[0].binding = Some(b);
                    // the second key is consumed now
                    self.combo.queue[pos].binding = Some(Binding::Nop);
                    self.combo.state = ComboState::Combo;
                } else {
                    self.combo.state = ComboState::NoCombo;
                }
            } else {
                self.combo.state = ComboState::NoCombo;
            }
        }

        match self.combo.state {
            ComboState::NoCombo | ComboState::Combo => self.combo_resolved(),
            ComboState::Idle => {
                let eb = self.combo.queue.remove(pos);
                self.mod_layer_handle(eb);
            }
            ComboState::Wait => self.combo.pos += 1,
        }
    }

    fn combo_resolved(&mut self) {
        self.combo.deadline = None;
        if self.combo.state == ComboState::NoCombo {
            debug!("ComboHandler: no combo");
        } else {
            debug!("ComboHandler: combo triggered");
        }
        self.combo.state = ComboState::Idle;
        self.combo.pos = 0;
        // the first key in the queue is the one that triggered the combo
        let first = self.combo.queue.remove(0);
        self.mod_layer_handle(first);
    }

    // ---------------------------------------------------------------------------------------------
    // ModLayerHandler
    // ---------------------------------------------------------------------------------------------

    fn mod_layer_handle(&mut self, mut eb: EventBinding) {
        debug!("ModLayerHandler: handling Event: {:?}", eb);
        let event = eb.event;

        let mod_layer = match eb.binding.as_ref().or_else(|| self.current_layer().bindings.get(&event.code)) {
            Some(Binding::ModLayer { mod_key, layer }) => Some((*mod_key, layer.clone())),
            _ => None,
        };
        let ml = &mut self.mod_layer;

        if event.is_press {
            if let Some((mod_key, layer_name)) = mod_layer {
                debug!("ModLayerHandler: pressing modifier");
                eb.binding = Some(Binding::Key(vec![mod_key]));
                // we don't allow two active mod bindings at the same time
                if ml.state == ModLayerState::Idle {
                    match self.config.layers.iter().position(|l| l.name == layer_name) {
                        None => warn!("ModLayerHandler: layer does not exist: {}", layer_name),
                        Some(idx) => {
                            ml.state = ModLayerState::ModActive;
                            ml.layer = idx;
                            ml.mod_key = mod_key;
                            ml.trigger_key = event.code;
                            ml.pressed_layer_keys.clear();
                            ml.original_layer = self.executor.current_layer;
                        }
                    }
                }
            } else if ml.state != ModLayerState::Idle && self.executor.current_layer == ml.original_layer {
                // if a mod binding is active and the pressed key is mapped in the corresponding layer,
                // insert the binding, unless the layer has changed in the meantime
                if let Some(binding) = self.config.layers[ml.layer].bindings.get(&event.code).cloned() {
                    ml.pressed_layer_keys.insert(event.code);
                    if ml.state == ModLayerState::ModActive {
                        ml.state = ModLayerState::LayerActive;
                        eb.binding = Some(Binding::Multi(vec![Binding::KeyRelease(ml.mod_key), binding]));
                    } else {
                        eb.binding = Some(binding);
                    }
                }
            }
        } else if ml.state != ModLayerState::Idle {
            // if no more mapped keys are pressed, press the mod key again
            if ml.pressed_layer_keys.remove(&event.code)
                && ml.state == ModLayerState::LayerActive
                && ml.pressed_layer_keys.is_empty()
            {
                ml.state = ModLayerState::ModActive;
                eb.binding = Some(Binding::KeyPress(ml.mod_key));
            }
            // done if the trigger key is released
            if ml.trigger_key == event.code {
                debug!("ModLayerHandler: done due to release of modifier");
                ml.state = ModLayerState::Idle;
                ml.pressed_layer_keys.clear();
            }
        }

        self.tap_hold_handle(eb);
    }

    // ---------------------------------------------------------------------------------------------
    // TapHoldHandler
    // ---------------------------------------------------------------------------------------------

    fn tap_hold_handle(&mut self, eb: EventBinding) {
        self.tap_hold.queue.push(eb);
        self.tap_hold_process();
    }

    fn tap_hold_process(&mut self) {
        while self.tap_hold.pos < self.tap_hold.queue.len() {
            self.tap_hold_next();
        }
    }

    fn tap_hold_next(&mut self) {
        let pos = self.tap_hold.pos;
        let eb = &self.tap_hold.queue[pos];
        let event = eb.event;
        debug!("TapHoldHandler: handling Event: {:?}", eb);

        let tap_hold_binding = match eb.binding.as_ref().or_else(|| self.current_layer().bindings.get(&event.code)) {
            Some(Binding::TapHold(t)) => Some(t.clone()),
            _ => None,
        };
        let th = &mut self.tap_hold;

        if event.is_press {
            if let Some(binding) = tap_hold_binding {
                if th.state != TapHoldState::Wait {
                    debug!("TapHoldHandler: activating holdBack");
                    th.state = TapHoldState::Wait;
                    // remember all pressed keys
                    th.hold_back_start_is_pressed = th.is_pressed.clone();
                    // the timeout is relative to the time of the key press
                    if binding.timeout_ms > 0 {
                        th.deadline = Some(event.time + Duration::from_millis(binding.timeout_ms));
                    }
                    th.binding = Some(binding);
                    // if the key has been pressed recently within quickTapTime, activate the tap binding
                    if let Some(&last) = th.last_pressed.get(&event.code) {
                        if event.time < last + Duration::from_millis(self.config.quick_tap_time_ms) {
                            debug!("TapHoldHandler: quick tap detected");
                            th.state = TapHoldState::Tap;
                        }
                    }
                }
            }
        } else if th.state == TapHoldState::Wait && th.binding.is_some() && th.queue[0].event.code == event.code {
            // execute tap binding if the tap-hold key is released
            th.state = TapHoldState::Tap;
        }

        if th.state == TapHoldState::Wait && event.code != th.queue[0].event.code {
            let binding = th.binding.as_ref().expect("binding is set in wait state");
            if event.is_press {
                // if TapOnNext and another key is pressed, activate hold
                if binding.tap_on_next {
                    th.state = TapHoldState::Hold;
                }
            } else if binding.tap_on_next_release && !th.hold_back_start_is_pressed.contains(&event.code) {
                // if TapOnNextRelease and another key is released that wasn't pressed before the tap key,
                // activate hold
                th.state = TapHoldState::Hold;
            }
        }

        match th.state {
            TapHoldState::Hold | TapHoldState::Tap => self.tap_hold_resolve(),
            TapHoldState::Idle => self.tap_hold_event_handled(0),
            TapHoldState::Wait => {
                // forward a key release where the press was before the tap hold started, to prevent
                // auto-repeat from kicking in, but we don't want and need this for modifiers
                let was_pressed = th.hold_back_start_is_pressed.contains(&event.code);
                if !event.is_press && was_pressed && !keys::is_modifier(event.code) {
                    debug!(
                        "TapHoldHandler: forwarding key release {} which was pressed before the tap hold started",
                        event.code
                    );
                    self.tap_hold_event_handled(pos);
                } else {
                    th.pos += 1;
                }
            }
        }
    }

    /// Must be called when a tap-hold binding has been resolved (state Tap or Hold).
    fn tap_hold_resolve(&mut self) {
        let th = &mut self.tap_hold;
        if th.state != TapHoldState::Tap && th.state != TapHoldState::Hold {
            debug!("TapHoldHandler: resolveTapHold called in state {:?}", th.state);
            return;
        }
        th.deadline = None;
        let binding = th.binding.take().expect("binding is set when resolving");
        // the first event in the queue is the one that triggered the tap-hold
        th.queue[0].binding = Some(if th.state == TapHoldState::Hold {
            debug!("TapHoldHandler: activated hold binding");
            *binding.hold
        } else {
            debug!("TapHoldHandler: activated tap binding");
            *binding.tap
        });
        th.state = TapHoldState::Idle;
        // process from the beginning of the queue
        th.pos = 0;
        self.tap_hold_event_handled(0);
    }

    /// Forwards the event at the given position and removes it from the queue.
    fn tap_hold_event_handled(&mut self, position: usize) {
        let th = &mut self.tap_hold;
        let eb = th.queue.remove(position);
        let event = eb.event;
        if event.is_press {
            th.is_pressed.insert(event.code);
            th.last_pressed.insert(event.code, event.time);
        } else {
            th.is_pressed.remove(&event.code);
            th.hold_back_start_is_pressed.remove(&event.code);
        }
        self.default_handle(eb);
    }

    // ---------------------------------------------------------------------------------------------
    // DefaultHandler
    // ---------------------------------------------------------------------------------------------

    fn default_handle(&mut self, mut eb: EventBinding) {
        debug!("DefaultHandler: handling Event: {:?}", eb);
        let event = eb.event;

        // resolve the binding if it is a press and not bound yet
        if event.is_press && eb.binding.is_none() {
            let layer = self.current_layer();
            let mut binding = layer.bindings.get(&event.code).cloned();

            // switch to the first layer on escape, if not mapped to something else
            if binding.is_none() && event.code == KEY_ESC && self.executor.current_layer != 0 {
                binding = Some(Binding::Layer(self.config.layers[0].name.clone()));
            }
            // use the wildcard binding if no binding is defined for the key
            if binding.is_none() {
                binding = layer.wildcard_binding.clone();
            }
            // if there is no wildcard either and pass through is enabled, insert a KeyBinding
            if binding.is_none() && layer.pass_through {
                binding = Some(Binding::Key(vec![event.code]));
            }
            eb.binding = binding;
        }

        self.one_shot_handle(eb);
    }

    // ---------------------------------------------------------------------------------------------
    // OneShotHandler
    // ---------------------------------------------------------------------------------------------

    fn one_shot_handle(&mut self, mut eb: EventBinding) {
        debug!("OneShotHandler: handling Event: {:?}", eb);
        let event = eb.event;
        let key_idx = self.one_shot.keys.iter().position(|k| k.code == event.code);

        if event.is_press {
            let one_shot_inner = match &eb.binding {
                Some(Binding::OneShot(inner)) => Some((**inner).clone()),
                _ => None,
            };

            if let Some(i) = key_idx {
                if one_shot_inner.is_some() || is_unbound_key(eb.binding.as_ref(), event.code) {
                    // an active one-shot key is pressed again, its binding is still active
                    let key = &mut self.one_shot.keys[i];
                    key.state = match key.state {
                        OneShotState::Armed => OneShotState::ArmedHeld,
                        OneShotState::Locked => OneShotState::LockedHeld,
                        s => s,
                    };
                    key.other_key_pressed = false;
                    return;
                }
                // the key is bound to another action now (e.g. the hold action of a tap-hold), so release
                // the one-shot
                self.one_shot_release_key(event.code);
            }

            // presses mapped to nop (e.g. the second key of a combo) are ignored
            if matches!(eb.binding, Some(Binding::Nop)) {
                self.executor_handle(eb);
                return;
            }

            for k in &mut self.one_shot.keys {
                k.other_key_pressed = true;
            }

            if let Some(inner) = one_shot_inner {
                debug!("OneShotHandler: one-shot key pressed");
                self.one_shot.keys.push(OneShotKey {
                    code: event.code,
                    state: OneShotState::Held,
                    other_key_pressed: false,
                });
                eb.binding = Some(inner);
                self.executor_handle(eb);
                return;
            }

            let is_modifier = is_modifier_binding(eb.binding.as_ref());
            self.executor_handle(eb);
            // modifiers don't consume armed one-shot keys, so that they can be combined
            if !is_modifier {
                self.one_shot_release_armed_keys();
            }
            return;
        }

        // key release
        if let Some(i) = key_idx {
            let key = &mut self.one_shot.keys[i];
            match key.state {
                OneShotState::Held if !key.other_key_pressed => {
                    debug!("OneShotHandler: one-shot key tapped, arming it");
                    key.state = OneShotState::Armed;
                    return;
                }
                OneShotState::ArmedHeld if !key.other_key_pressed => {
                    debug!("OneShotHandler: one-shot key tapped twice, locking it");
                    key.state = OneShotState::Locked;
                    return;
                }
                _ => {}
            }
            self.one_shot.keys.remove(i);
        }
        self.executor_handle(eb);
    }

    fn one_shot_release_armed_keys(&mut self) {
        let armed: Vec<u16> = self
            .one_shot
            .keys
            .iter()
            .filter(|k| k.state == OneShotState::Armed)
            .map(|k| k.code)
            .collect();
        for code in armed {
            self.one_shot_release_key(code);
        }
    }

    /// Deactivates the binding of the given one-shot key by sending its key release.
    fn one_shot_release_key(&mut self, code: u16) {
        debug!("OneShotHandler: releasing one-shot key {}", code);
        self.one_shot.keys.retain(|k| k.code != code);
        self.executor_handle(EventBinding::new(Event { code, is_press: false, time: Instant::now() }));
    }

    // ---------------------------------------------------------------------------------------------
    // Executor
    // ---------------------------------------------------------------------------------------------

    fn executor_handle(&mut self, eb: EventBinding) {
        if !eb.event.is_press {
            self.key_released(eb.event.code);
        }
        if let Some(binding) = eb.binding {
            self.execute_binding(&binding, eb.event.code, eb.event.time);
        }
    }

    fn execute_binding(&mut self, binding: &Binding, cause: u16, now: Instant) {
        debug!("Executing {:?}", binding);
        match binding {
            Binding::Multi(bindings) => {
                for b in bindings {
                    self.execute_binding(b, cause, now);
                }
            }
            Binding::Speed(s) => self.mouse.add_speed_factor(cause, *s, now),
            Binding::Scroll { x, y } => self.mouse.change_scroll_speed(cause, *x, *y, now),
            Binding::Move { x, y } => self.mouse.change_move_speed(cause, *x, *y, now),
            Binding::Button { button, repeat_interval_ms } => {
                if *repeat_interval_ms > 0 {
                    self.mouse.button_repeat(cause, *button, Duration::from_millis(*repeat_interval_ms), now, &mut self.out);
                } else {
                    self.mouse.button_press(cause, *button, &mut self.out);
                }
            }
            Binding::Key(combo) => {
                // replace any wildcard with the key that was pressed
                let keys: Vec<u16> = combo.iter().map(|&k| if k == WILDCARD { cause } else { k }).collect();
                self.keyboard.press_keys(cause, &keys, &mut self.out);
            }
            Binding::KeyPress(k) => self.keyboard.press_key(*k, &mut self.out),
            Binding::KeyRelease(k) => self.keyboard.release_key(*k, &mut self.out),
            Binding::Layer(name) => {
                // deactivate any toggled layers
                self.executor.toggle_layer_keys.clear();
                self.executor.toggle_layer_previous.clear();
                if let Some(idx) = self.get_layer(name) {
                    self.go_to_layer(idx);
                }
            }
            Binding::ToggleLayer(name) => {
                if let Some(idx) = self.get_layer(name) {
                    self.executor.toggle_layer_keys.push(cause);
                    self.executor.toggle_layer_previous.push(self.executor.current_layer);
                    self.go_to_layer(idx);
                }
            }
            Binding::ReloadConfig => self.reload_requested = true,
            Binding::Quit => self.quit_requested = true,
            Binding::Exec(cmd) => self.execute_command_with_key(cmd.clone(), cause),
            Binding::ExecPressRelease { press, release } => {
                self.execute_command_with_key(press.clone(), cause);
                self.executor.exec_press_release.insert(cause, release.clone());
            }
            Binding::Nop | Binding::TapHold(_) | Binding::ModLayer { .. } | Binding::OneShot(_) => {}
        }
    }

    fn key_released(&mut self, code: u16) {
        // go back to the previous layer when a toggle-layer key is released
        if let Some(i) = self.executor.toggle_layer_keys.iter().position(|&k| k == code) {
            let previous = self.executor.toggle_layer_previous[i];
            // all layers that have been toggled after the current one are removed as well
            self.executor.toggle_layer_keys.truncate(i);
            self.executor.toggle_layer_previous.truncate(i);
            self.go_to_layer(previous);
        }
        if let Some(cmd) = self.executor.exec_press_release.remove(&code) {
            self.execute_command_with_key(cmd, code);
        }
        self.keyboard.original_key_up(code, &mut self.out);
        self.mouse.original_key_up(code, &mut self.out);
    }

    fn go_to_layer(&mut self, idx: usize) {
        if let Some(cmd) = self.current_layer().exit_command.clone() {
            self.execute_command(cmd, Vec::new());
        }
        debug!("Switching to layer {}", self.config.layers[idx].name);
        self.executor.current_layer = idx;
        if let Some(cmd) = self.current_layer().enter_command.clone() {
            self.execute_command(cmd, Vec::new());
        }
    }

    fn execute_command_with_key(&mut self, command: String, cause: u16) {
        let env = vec![
            ("key".to_string(), keys::key_name_or_unknown(cause).to_string()),
            ("key_code".to_string(), cause.to_string()),
        ];
        self.execute_command(command, env);
    }

    fn execute_command(&mut self, command: String, env: Vec<(String, String)>) {
        self.out.push(Output::Exec { command, env });
    }
}

/// Returns true if the given binding presses only modifier keys.
fn is_modifier_binding(binding: Option<&Binding>) -> bool {
    match binding {
        Some(Binding::Key(combo)) => combo.iter().all(|&c| keys::is_modifier(c)),
        _ => false,
    }
}

/// Returns true if the given binding of a key press does not define an own action for the key, e.g. if
/// the one-shot key is pressed again while its one-shot layer is active, which does not map the key.
fn is_unbound_key(binding: Option<&Binding>, code: u16) -> bool {
    match binding {
        None | Some(Binding::Nop) => true,
        Some(Binding::Key(combo)) => combo.as_slice() == [code] || combo.contains(&WILDCARD),
        _ => false,
    }
}

// -------------------------------------------------------------------------------------------------
// handler states
// -------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Default)]
enum ComboState {
    #[default]
    Idle,
    Wait,
    NoCombo,
    Combo,
}

#[derive(Default)]
struct ComboHandler {
    queue: Vec<EventBinding>,
    pos: usize,
    state: ComboState,
    deadline: Option<Instant>,
    bindings: HashMap<u16, Binding>,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
enum ModLayerState {
    #[default]
    Idle,
    ModActive,
    LayerActive,
}

#[derive(Default)]
struct ModLayerHandler {
    state: ModLayerState,
    mod_key: u16,
    trigger_key: u16,
    layer: usize,
    original_layer: usize,
    pressed_layer_keys: HashSet<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
enum TapHoldState {
    #[default]
    Idle,
    Wait,
    Tap,
    Hold,
}

#[derive(Default)]
struct TapHoldHandler {
    queue: Vec<EventBinding>,
    pos: usize,
    is_pressed: HashSet<u16>,
    last_pressed: HashMap<u16, Instant>,
    state: TapHoldState,
    binding: Option<TapHoldBinding>,
    deadline: Option<Instant>,
    hold_back_start_is_pressed: HashSet<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum OneShotState {
    /// the one-shot key is pressed for the first time
    Held,
    /// the one-shot key has been tapped, so its binding is active until the next key press
    Armed,
    /// the one-shot key is pressed again while being armed
    ArmedHeld,
    /// the one-shot key has been tapped twice, so its binding is active until it is tapped again
    Locked,
    /// the one-shot key is pressed again while being locked
    LockedHeld,
}

struct OneShotKey {
    code: u16,
    state: OneShotState,
    other_key_pressed: bool,
}

#[derive(Default)]
struct OneShotHandler {
    keys: Vec<OneShotKey>,
}

#[derive(Default)]
struct Executor {
    current_layer: usize,
    // remember all keys that toggled a layer, and from which layer they came from
    toggle_layer_keys: Vec<u16>,
    toggle_layer_previous: Vec<usize>,
    exec_press_release: HashMap<u16, String>,
}

// -------------------------------------------------------------------------------------------------
// virtual keyboard
// -------------------------------------------------------------------------------------------------

#[derive(Default)]
struct VirtualKeyboard {
    is_pressed: HashSet<u16>,
    pressed_modifiers: HashSet<u16>,
    triggered_keys: HashMap<u16, Vec<u16>>,
}

impl VirtualKeyboard {
    /// Presses the given keys and releases them automatically when the given trigger key is released.
    fn press_keys(&mut self, trigger: u16, codes: &[u16], out: &mut Vec<Output>) {
        self.triggered_keys.entry(trigger).or_default().extend_from_slice(codes);
        // release previous modifiers
        let mods: Vec<u16> = self.pressed_modifiers.iter().copied().collect();
        for c in mods {
            self.release_key(c, out);
        }
        // a combo consisting only of modifiers (e.g. leftmeta+leftctrl) is meant to be held while pressing
        // other keys, so its keys must not be released by subsequent combos
        let only_modifiers = codes.iter().all(|&c| keys::is_modifier(c));
        for (i, &c) in codes.iter().enumerate() {
            self.press_key(c, out);
            if i < codes.len() - 1 && !only_modifiers {
                self.pressed_modifiers.insert(c);
            }
        }
    }

    fn press_key(&mut self, code: u16, out: &mut Vec<Output>) {
        debug!("Keyboard: pressing {} ({})", keys::key_name_or_unknown(code), code);
        out.push(Output::Key { code, down: true });
        self.is_pressed.insert(code);
    }

    fn release_key(&mut self, code: u16, out: &mut Vec<Output>) {
        debug!("Keyboard: releasing {} ({})", keys::key_name_or_unknown(code), code);
        out.push(Output::Key { code, down: false });
        self.is_pressed.remove(&code);
        self.pressed_modifiers.remove(&code);
    }

    fn original_key_up(&mut self, code: u16, out: &mut Vec<Output>) {
        if let Some(codes) = self.triggered_keys.remove(&code) {
            for c in codes {
                if self.is_pressed.contains(&c) {
                    self.release_key(c, out);
                }
            }
        }
    }
}

// -------------------------------------------------------------------------------------------------
// virtual mouse
// -------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Default, Debug)]
struct Vector {
    x: f64,
    y: f64,
}

struct ButtonRepeat {
    interval: Duration,
    next: Instant,
}

struct VirtualMouse {
    loop_interval: Duration,
    base_mouse_speed: f64,
    base_scroll_speed: f64,
    start_mouse_speed: f64,
    acceleration_time: f64,
    deceleration_time: f64,
    acceleration_curve: f64,
    deceleration_curve: f64,

    is_button_pressed: HashSet<MouseButton>,
    // a single key may press multiple buttons (e.g. via multi)
    buttons_by_keys: HashMap<u16, Vec<MouseButton>>,
    repeats_by_keys: HashMap<(u16, MouseButton), ButtonRepeat>,
    move_by_keys: HashMap<u16, Vector>,
    scroll_by_keys: HashMap<u16, Vector>,
    speed_by_keys: HashMap<u16, f64>,

    velocity: Vector,
    move_fraction: Vector,
    scroll_fraction: Vector,

    /// when the mouse loop runs next, None if it is idle
    next_tick: Option<Instant>,
    last_update: Instant,
}

impl VirtualMouse {
    fn new(config: &Config) -> Self {
        let mut m = VirtualMouse {
            loop_interval: Duration::from_millis(20),
            base_mouse_speed: 0.0,
            base_scroll_speed: 0.0,
            start_mouse_speed: 0.0,
            acceleration_time: 0.0,
            deceleration_time: 0.0,
            acceleration_curve: 1.0,
            deceleration_curve: 1.0,
            is_button_pressed: HashSet::new(),
            buttons_by_keys: HashMap::new(),
            repeats_by_keys: HashMap::new(),
            move_by_keys: HashMap::new(),
            scroll_by_keys: HashMap::new(),
            speed_by_keys: HashMap::new(),
            velocity: Vector::default(),
            move_fraction: Vector::default(),
            scroll_fraction: Vector::default(),
            next_tick: None,
            last_update: Instant::now(),
        };
        m.set_config(config);
        m
    }

    fn set_config(&mut self, c: &Config) {
        self.loop_interval = Duration::from_millis(c.mouse_loop_interval_ms);
        self.base_mouse_speed = c.base_mouse_speed;
        self.base_scroll_speed = c.base_scroll_speed;
        self.start_mouse_speed = c.start_mouse_speed;
        self.acceleration_time = c.mouse_acceleration_time;
        self.deceleration_time = c.mouse_deceleration_time;
        self.acceleration_curve = c.mouse_acceleration_curve;
        self.deceleration_curve = c.mouse_deceleration_curve;
    }

    fn next_deadline(&self) -> Option<Instant> {
        let repeat = self.repeats_by_keys.values().map(|r| r.next).min();
        [self.next_tick, repeat].into_iter().flatten().min()
    }

    fn button_press(&mut self, key: u16, button: MouseButton, out: &mut Vec<Output>) {
        self.buttons_by_keys.entry(key).or_default().push(button);
        self.is_button_pressed.insert(button);
        debug!("Mouse: pressing {:?}", button);
        out.push(Output::Button { button, down: true });
    }

    /// Clicks the given button immediately and then repeatedly with the given interval until the key is
    /// released.
    fn button_repeat(&mut self, key: u16, button: MouseButton, interval: Duration, now: Instant, out: &mut Vec<Output>) {
        self.repeats_by_keys.insert((key, button), ButtonRepeat { interval, next: now + interval });
        self.click(button, out);
    }

    /// Presses and releases the given button, unless it is currently held down by another key.
    fn click(&mut self, button: MouseButton, out: &mut Vec<Output>) {
        if self.is_button_pressed.contains(&button) {
            return;
        }
        debug!("Mouse: clicking {:?}", button);
        out.push(Output::Button { button, down: true });
        out.push(Output::Button { button, down: false });
    }

    fn change_move_speed(&mut self, key: u16, x: f64, y: f64, now: Instant) {
        self.move_by_keys.insert(key, Vector { x, y });
        self.mouse_move_change(now);
    }

    fn change_scroll_speed(&mut self, key: u16, x: f64, y: f64, now: Instant) {
        self.scroll_by_keys.insert(key, Vector { x, y });
        self.mouse_move_change(now);
    }

    fn add_speed_factor(&mut self, key: u16, factor: f64, now: Instant) {
        self.speed_by_keys.insert(key, factor);
        self.mouse_move_change(now);
    }

    fn original_key_up(&mut self, code: u16, out: &mut Vec<Output>) {
        self.move_by_keys.remove(&code);
        self.scroll_by_keys.remove(&code);
        self.speed_by_keys.remove(&code);
        self.repeats_by_keys.retain(|(k, _), _| *k != code);
        if let Some(buttons) = self.buttons_by_keys.remove(&code) {
            for b in buttons {
                if self.is_button_pressed.remove(&b) {
                    debug!("Mouse: releasing {:?}", b);
                    out.push(Output::Button { button: b, down: false });
                }
            }
        }
    }

    /// Starts the mouse loop if it is not running.
    fn mouse_move_change(&mut self, now: Instant) {
        if self.next_tick.is_none() {
            self.next_tick = Some(now);
            // set last_update to the past so that the mouse starts moving immediately
            self.last_update = now.checked_sub(self.loop_interval).unwrap_or(now);
        }
    }

    fn on_timer(&mut self, now: Instant, out: &mut Vec<Output>) {
        // repeated clicks
        let mut clicks = Vec::new();
        for ((_, button), r) in self.repeats_by_keys.iter_mut() {
            if r.next <= now {
                clicks.push(*button);
                r.next += r.interval;
                if r.next <= now {
                    r.next = now + r.interval;
                }
            }
        }
        for b in clicks {
            self.click(b, out);
        }

        // movement and scrolling
        if self.next_tick.is_some_and(|t| t <= now) {
            let duration = now.saturating_duration_since(self.last_update);
            self.last_update = now;
            self.move_and_scroll(duration, now, out);
        }
    }

    fn move_and_scroll(&mut self, update_duration: Duration, now: Instant, out: &mut Vec<Output>) {
        let mut mv = Vector::default();
        let mut scroll = Vector::default();
        for d in self.move_by_keys.values() {
            mv.x += d.x;
            mv.y += d.y;
        }
        for d in self.scroll_by_keys.values() {
            scroll.x += d.x;
            scroll.y += d.y;
        }
        let speed_factor: f64 = self.speed_by_keys.values().product();

        if !self.move_by_keys.is_empty() || !self.scroll_by_keys.is_empty() || self.is_moving() {
            let tick_time = update_duration.as_secs_f64();
            let move_speed = self.base_mouse_speed * tick_time;
            let scroll_speed = self.base_scroll_speed * tick_time;
            let acceleration_step = tick_time * 1000.0 / self.acceleration_time;
            let deceleration_step = tick_time * 1000.0 / self.deceleration_time;
            self.scroll(scroll.x * scroll_speed * speed_factor, scroll.y * scroll_speed * speed_factor, out);
            self.do_move(
                mv.x * move_speed,
                mv.y * move_speed,
                self.start_mouse_speed * tick_time,
                self.base_mouse_speed * tick_time,
                acceleration_step,
                deceleration_step,
                speed_factor,
                out,
            );
            self.next_tick = Some(now + self.loop_interval);
        } else {
            self.next_tick = None;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn do_move(
        &mut self,
        x: f64,
        y: f64,
        start_speed: f64,
        max_speed: f64,
        acceleration_step: f64,
        deceleration_step: f64,
        speed_factor: f64,
        out: &mut Vec<Output>,
    ) {
        let (ac, dc) = (self.acceleration_curve, self.deceleration_curve);
        self.velocity.x = move_towards(self.velocity.x, x, max_speed, start_speed, ac, acceleration_step, dc, deceleration_step);
        self.velocity.y = move_towards(self.velocity.y, y, max_speed, start_speed, ac, acceleration_step, dc, deceleration_step);
        self.move_fraction.x += self.velocity.x * speed_factor;
        self.move_fraction.y += self.velocity.y * speed_factor;
        // move only the integer part
        let dx = self.move_fraction.x as i32;
        let dy = self.move_fraction.y as i32;
        self.move_fraction.x -= dx as f64;
        self.move_fraction.y -= dy as f64;
        if dx != 0 || dy != 0 {
            out.push(Output::MouseMove { dx, dy });
        }
    }

    fn scroll(&mut self, x: f64, y: f64, out: &mut Vec<Output>) {
        // Windows supports high-resolution scrolling, where one wheel click corresponds to 120 steps
        let (xs, ys) = scrolling_steps(&mut self.scroll_fraction, x, y, 120);
        if xs != 0 {
            out.push(Output::Wheel { horizontal: true, delta: xs });
        }
        if ys != 0 {
            out.push(Output::Wheel { horizontal: false, delta: -ys });
        }
    }

    fn is_moving(&self) -> bool {
        self.velocity.x != 0.0 || self.velocity.y != 0.0
    }
}

#[allow(clippy::too_many_arguments)]
fn move_towards(
    current: f64,
    target: f64,
    max: f64,
    start: f64,
    acceleration_curve: f64,
    acceleration_step: f64,
    deceleration_curve: f64,
    deceleration_step: f64,
) -> f64 {
    if target < 0.0 || (target == 0.0 && current < 0.0) {
        return -move_towards(-current, -target, max, start, acceleration_curve, acceleration_step, deceleration_curve, deceleration_step);
    }
    let mut current = current;
    if current <= 0.0 && target > 0.0 {
        current = start;
    }
    if current < target {
        let t = (current / max).powf(1.0 / acceleration_curve) + acceleration_step;
        target.min(target * t.powf(acceleration_curve))
    } else {
        let t = (current / max).powf(1.0 / deceleration_curve) - deceleration_step;
        if t <= 0.0 || t.is_nan() {
            return target;
        }
        target.max(max * t.powf(deceleration_curve))
    }
}

/// Calculates discrete scroll steps from fractional input, the remaining fraction is kept in
/// `fraction`. `step_count` is the number of steps for x=1.0 or y=1.0.
fn scrolling_steps(fraction: &mut Vector, x: f64, y: f64, step_count: i32) -> (i32, i32) {
    // when the direction changes, start at 0 again
    if x > 0.0 {
        fraction.x = fraction.x.max(0.0);
    } else if x < 0.0 {
        fraction.x = fraction.x.min(0.0);
    }
    if y > 0.0 {
        fraction.y = fraction.y.max(0.0);
    } else if y < 0.0 {
        fraction.y = fraction.y.min(0.0);
    }
    fraction.x += x;
    fraction.y += y;
    let xs = (fraction.x * step_count as f64) as i32;
    let ys = (fraction.y * step_count as f64) as i32;
    fraction.x -= xs as f64 / step_count as f64;
    fraction.y -= ys as f64 / step_count as f64;
    (xs, ys)
}

#[cfg(test)]
mod tests;
