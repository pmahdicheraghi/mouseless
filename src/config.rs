//! Parsing of the config file, compatible with the config format of the original mouseless.

use crate::keys::{self, WILDCARD};
use crate::{debug, warn};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    /// the "back" side button (Windows only extension)
    X1,
    /// the "forward" side button (Windows only extension)
    X2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TapHoldBinding {
    pub tap: Box<Binding>,
    pub hold: Box<Binding>,
    pub timeout_ms: u64,
    pub tap_on_next: bool,
    pub tap_on_next_release: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Binding {
    Multi(Vec<Binding>),
    TapHold(TapHoldBinding),
    ModLayer { mod_key: u16, layer: String },
    OneShot(Box<Binding>),
    Layer(String),
    ToggleLayer(String),
    ReloadConfig,
    Move { x: f64, y: f64 },
    Scroll { x: f64, y: f64 },
    Speed(f64),
    Button { button: MouseButton, repeat_interval_ms: u64 },
    Exec(String),
    ExecPressRelease { press: String, release: String },
    Nop,
    Key(Vec<u16>),
    /// Exits mouseless (Windows only extension).
    Quit,
    // only used internally
    KeyPress(u16),
    KeyRelease(u16),
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    pub pass_through: bool,
    pub enter_command: Option<String>,
    pub exit_command: Option<String>,
    pub bindings: HashMap<u16, Binding>,
    pub combo_bindings: HashMap<u16, HashMap<u16, Binding>>,
    pub wildcard_binding: Option<Binding>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub devices: Vec<String>,
    pub start_command: String,
    pub mouse_loop_interval_ms: u64,
    pub quick_tap_time_ms: u64,
    pub combo_time_ms: u64,
    pub base_mouse_speed: f64,
    pub mouse_acceleration_curve: f64,
    pub mouse_acceleration_time: f64,
    pub mouse_deceleration_curve: f64,
    pub mouse_deceleration_time: f64,
    pub start_mouse_speed: f64,
    pub base_scroll_speed: f64,
    pub instance_name: String,
    pub layers: Vec<Layer>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawConfig {
    devices: Option<Vec<String>>,
    devices_exclude: Option<Vec<String>>,
    start_command: Option<String>,
    mouse_loop_interval: f64,
    base_mouse_speed: f64,
    start_mouse_speed: f64,
    mouse_acceleration_curve: f64,
    mouse_acceleration_time: f64,
    mouse_deceleration_curve: f64,
    mouse_deceleration_time: f64,
    base_scroll_speed: f64,
    quick_tap_time: f64,
    combo_time: f64,
    instance_name: Option<String>,
    layers: Option<Vec<RawLayer>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLayer {
    name: Option<String>,
    pass_through: Option<bool>,
    enter_command: Option<String>,
    exit_command: Option<String>,
    bindings: Option<serde_yaml::Mapping>,
}

const KNOWN_KEYS: &[&str] = &[
    "devices",
    "devicesExclude",
    "startCommand",
    "mouseLoopInterval",
    "baseMouseSpeed",
    "startMouseSpeed",
    "mouseAccelerationCurve",
    "mouseAccelerationTime",
    "mouseDecelerationCurve",
    "mouseDecelerationTime",
    "baseScrollSpeed",
    "quickTapTime",
    "comboTime",
    "instanceName",
    "layers",
];
const KNOWN_LAYER_KEYS: &[&str] = &["name", "passThrough", "enterCommand", "exitCommand", "bindings"];

pub fn read_config(path: &Path) -> Result<Config, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_config(&text)
}

pub fn parse_config(text: &str) -> Result<Config, String> {
    warn_unknown_keys(text);
    let raw: RawConfig = serde_yaml::from_str(text).map_err(|e| e.to_string())?;

    let mut config = Config {
        devices: raw.devices.unwrap_or_default(),
        start_command: raw.start_command.unwrap_or_default(),
        mouse_loop_interval_ms: if raw.mouse_loop_interval > 0.0 { raw.mouse_loop_interval as u64 } else { 20 },
        quick_tap_time_ms: raw.quick_tap_time.max(0.0) as u64,
        combo_time_ms: if raw.combo_time > 0.0 { raw.combo_time as u64 } else { 25 },
        base_mouse_speed: raw.base_mouse_speed,
        mouse_acceleration_curve: if raw.mouse_acceleration_curve > 0.0 { raw.mouse_acceleration_curve } else { 1.0 },
        mouse_acceleration_time: raw.mouse_acceleration_time,
        mouse_deceleration_curve: if raw.mouse_deceleration_curve > 0.0 { raw.mouse_deceleration_curve } else { 1.0 },
        mouse_deceleration_time: raw.mouse_deceleration_time,
        start_mouse_speed: raw.start_mouse_speed,
        base_scroll_speed: raw.base_scroll_speed,
        instance_name: raw.instance_name.unwrap_or_default(),
        layers: Vec::new(),
    };
    if let Some(excl) = raw.devices_exclude {
        config.devices.extend(excl);
    }

    let raw_layers = raw.layers.unwrap_or_default();
    if raw_layers.is_empty() {
        return Err("no layers defined".into());
    }
    for (i, l) in raw_layers.into_iter().enumerate() {
        let layer = parse_layer(l).map_err(|e| format!("failed to parse layer {i}: {e}"))?;
        config.layers.push(layer);
    }
    // check that all referenced layers exist
    for layer in &config.layers {
        let mut all = layer.bindings.values().collect::<Vec<_>>();
        all.extend(layer.combo_bindings.values().flat_map(|m| m.values()));
        all.extend(layer.wildcard_binding.iter());
        for b in all {
            check_layer_refs(b, &config.layers)
                .map_err(|e| format!("layer '{}': {e}", layer.name))?;
        }
    }

    debug!("config: {:?}", config);
    Ok(config)
}

fn check_layer_refs(b: &Binding, layers: &[Layer]) -> Result<(), String> {
    let check = |name: &str| {
        if layers.iter().any(|l| l.name == name) {
            Ok(())
        } else {
            Err(format!("unknown layer '{name}'"))
        }
    };
    match b {
        Binding::Layer(n) | Binding::ToggleLayer(n) => check(n),
        Binding::ModLayer { layer, .. } => check(layer),
        Binding::Multi(bs) => bs.iter().try_for_each(|b| check_layer_refs(b, layers)),
        Binding::TapHold(t) => {
            check_layer_refs(&t.tap, layers)?;
            check_layer_refs(&t.hold, layers)
        }
        Binding::OneShot(b) => check_layer_refs(b, layers),
        _ => Ok(()),
    }
}

fn warn_unknown_keys(text: &str) {
    let Ok(serde_yaml::Value::Mapping(root)) = serde_yaml::from_str::<serde_yaml::Value>(text) else {
        return;
    };
    for key in root.keys() {
        let k = scalar_to_string(key).unwrap_or_default();
        if !KNOWN_KEYS.contains(&k.as_str()) {
            warn!("problem in the config file: unknown option '{k}'");
        }
    }
    if let Some(serde_yaml::Value::Sequence(layers)) = root.get("layers") {
        for layer in layers {
            if let serde_yaml::Value::Mapping(m) = layer {
                for key in m.keys() {
                    let k = scalar_to_string(key).unwrap_or_default();
                    if !KNOWN_LAYER_KEYS.contains(&k.as_str()) {
                        warn!("problem in the config file: unknown layer option '{k}'");
                    }
                }
            }
        }
    }
}

fn scalar_to_string(v: &serde_yaml::Value) -> Option<String> {
    match v {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn parse_layer(raw: RawLayer) -> Result<Layer, String> {
    let name = raw.name.filter(|n| !n.is_empty()).ok_or("no name given")?;
    let mut layer = Layer {
        name,
        pass_through: raw.pass_through.unwrap_or(true),
        enter_command: raw.enter_command,
        exit_command: raw.exit_command,
        bindings: HashMap::new(),
        combo_bindings: HashMap::new(),
        wildcard_binding: None,
    };

    for (k, v) in raw.bindings.unwrap_or_default() {
        let key = scalar_to_string(&k).ok_or_else(|| format!("invalid key: {k:?}"))?;
        let bind = scalar_to_string(&v).unwrap_or_default();
        let codes = parse_key_combo(&key).map_err(|e| format!("failed to parse the key '{key}': {e}"))?;
        let binding = parse_binding(&bind).map_err(|e| format!("failed to parse the binding '{bind}': {e}"))?;
        match codes.as_slice() {
            [WILDCARD] => layer.wildcard_binding = Some(binding),
            [code] => {
                layer.bindings.insert(*code, binding);
            }
            [a, b] => {
                layer.combo_bindings.entry(*a).or_default().insert(*b, binding.clone());
                layer.combo_bindings.entry(*b).or_default().insert(*a, binding);
            }
            _ => return Err(format!("combos with more than 2 keys are not supported: '{key}'")),
        }
    }
    Ok(layer)
}

/// Parses a single binding of a layer.
pub fn parse_binding(raw: &str) -> Result<Binding, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("binding is empty".into());
    }
    let args: Vec<&str> = raw.split_whitespace().collect();
    let action = args[0];
    let args = &args[1..];
    let arg_string = raw[action.len()..].trim();

    let parse_f64 = |s: &str, which: &str| s.parse::<f64>().map_err(|_| format!("{which} argument must be a number"));

    let binding = match action {
        "multi" => {
            let meta: Vec<&str> = arg_string.split(';').collect();
            if meta.len() < 2 {
                return Err("action requires at least two meta arguments (separated by ;)".into());
            }
            let mut bindings = Vec::new();
            for m in meta {
                let b = parse_binding(m)?;
                if matches!(b, Binding::OneShot(_)) {
                    return Err("one-shot is not supported within multi".into());
                }
                bindings.push(b);
            }
            Binding::Multi(bindings)
        }
        "tap-hold" => Binding::TapHold(parse_tap_hold(arg_string, false, false)?),
        "tap-hold-next" => Binding::TapHold(parse_tap_hold(arg_string, true, false)?),
        "tap-hold-next-release" => Binding::TapHold(parse_tap_hold(arg_string, false, true)?),
        "mod-layer" => {
            if args.len() != 2 {
                return Err("action requires exactly two arguments".into());
            }
            Binding::ModLayer { mod_key: keys::parse_key(args[0])?, layer: args[1].to_string() }
        }
        "one-shot" => {
            if args.is_empty() {
                return Err("action requires an action as argument".into());
            }
            let b = parse_binding(arg_string)?;
            if matches!(b, Binding::OneShot(_) | Binding::TapHold(_) | Binding::ModLayer { .. }) {
                return Err("this action is not supported within one-shot".into());
            }
            Binding::OneShot(Box::new(b))
        }
        "layer" => {
            if args.len() != 1 {
                return Err("action requires exactly one argument".into());
            }
            Binding::Layer(args[0].to_string())
        }
        "toggle-layer" => {
            if args.len() != 1 {
                return Err("action requires exactly one argument".into());
            }
            Binding::ToggleLayer(args[0].to_string())
        }
        "reload-config" => {
            if !args.is_empty() {
                return Err("action requires zero arguments".into());
            }
            Binding::ReloadConfig
        }
        "quit" => {
            if !args.is_empty() {
                return Err("action requires zero arguments".into());
            }
            Binding::Quit
        }
        "move" => {
            if args.len() != 2 {
                return Err("action requires exactly two arguments".into());
            }
            Binding::Move { x: parse_f64(args[0], "first")?, y: parse_f64(args[1], "second")? }
        }
        "scroll" => {
            if args.len() != 1 {
                return Err("action requires exactly one argument".into());
            }
            let (x, y) = match args[0] {
                "up" => (0.0, -1.0),
                "down" => (0.0, 1.0),
                "left" => (-1.0, 0.0),
                "right" => (1.0, 0.0),
                _ => return Err("first argument must one of up, down, left or right".into()),
            };
            Binding::Scroll { x, y }
        }
        "speed" => {
            if args.len() != 1 {
                return Err("action requires exactly one argument".into());
            }
            Binding::Speed(parse_f64(args[0], "first")?)
        }
        "button" => {
            if args.is_empty() || args.len() > 2 {
                return Err("action requires one or two arguments".into());
            }
            let button = match args[0].to_lowercase().as_str() {
                "left" => MouseButton::Left,
                "middle" => MouseButton::Middle,
                "right" => MouseButton::Right,
                "x1" | "back" => MouseButton::X1,
                "x2" | "forward" => MouseButton::X2,
                _ => return Err(format!("unknown button '{}'", args[0])),
            };
            let mut repeat_interval_ms = 0;
            if args.len() == 2 {
                repeat_interval_ms = match args[1].parse::<u64>() {
                    Ok(v) if v > 0 => v,
                    _ => return Err("second argument must be a positive number".into()),
                };
            }
            Binding::Button { button, repeat_interval_ms }
        }
        "exec" => {
            if args.is_empty() {
                return Err("action requires at least one argument".into());
            }
            Binding::Exec(arg_string.to_string())
        }
        "exec-press-release" => {
            let cmds: Vec<&str> = arg_string.split(';').collect();
            if cmds.len() != 2 {
                return Err("exec-press-release requires exactly two commands (separated by ;)".into());
            }
            Binding::ExecPressRelease { press: cmds[0].trim().to_string(), release: cmds[1].trim().to_string() }
        }
        "nop" => {
            if !args.is_empty() {
                return Err("action does not take any argument".into());
            }
            Binding::Nop
        }
        _ => match parse_key_combo(raw) {
            Ok(combo) => Binding::Key(combo),
            Err(_) => return Err("neither a valid action nor a valid key sequence".into()),
        },
    };
    Ok(binding)
}

fn parse_tap_hold(arg_string: &str, tap_on_next: bool, tap_on_next_release: bool) -> Result<TapHoldBinding, String> {
    let meta: Vec<&str> = arg_string.split(';').collect();
    if meta.len() != 3 {
        return Err("action requires exactly 3 meta arguments (separated by ;)".into());
    }
    let tap = parse_binding(meta[0])?;
    let hold = parse_binding(meta[1])?;
    let t = meta[2].trim();
    let timeout_ms = t.parse::<u64>().map_err(|_| format!("third argument must be a number: {t}"))?;
    Ok(TapHoldBinding { tap: Box::new(tap), hold: Box::new(hold), timeout_ms, tap_on_next, tap_on_next_release })
}

/// Parses a key combination of the form key1+key2+...
pub fn parse_key_combo(raw: &str) -> Result<Vec<u16>, String> {
    raw.split('+').map(keys::parse_key).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_example() {
        let c = parse_config(include_str!("../examples/config_full.yaml")).unwrap();
        assert_eq!(c.layers.len(), 4);
        assert_eq!(c.layers[0].name, "initial");
        assert!(c.layers[1].pass_through);
        assert!(!c.layers[2].pass_through);
        assert!(c.layers[2].wildcard_binding.is_some());
        assert_eq!(c.combo_time_ms, 25);
        let tab = &c.layers[0].bindings[&15];
        match tab {
            Binding::TapHold(t) => {
                assert!(t.tap_on_next);
                assert_eq!(*t.tap, Binding::Key(vec![15]));
                assert_eq!(*t.hold, Binding::ToggleLayer("mouse".into()));
                assert_eq!(t.timeout_ms, 500);
            }
            _ => panic!("expected tap-hold"),
        }
        // f+d combo
        assert_eq!(c.layers[0].combo_bindings[&33][&32], Binding::Layer("mouse".into()));
        assert_eq!(c.layers[0].combo_bindings[&32][&33], Binding::Layer("mouse".into()));
    }

    #[test]
    fn parses_minimal_example() {
        parse_config(include_str!("../examples/config_minimal.yaml")).unwrap();
    }

    #[test]
    fn parses_bindings() {
        assert_eq!(parse_binding("leftshift+k1").unwrap(), Binding::Key(vec![42, 2]));
        assert_eq!(parse_binding("move  1  -0.5").unwrap(), Binding::Move { x: 1.0, y: -0.5 });
        assert_eq!(
            parse_binding("button left 200").unwrap(),
            Binding::Button { button: MouseButton::Left, repeat_interval_ms: 200 }
        );
        assert_eq!(parse_binding("exec notepad.exe foo").unwrap(), Binding::Exec("notepad.exe foo".into()));
        assert!(parse_binding("move 1").is_err());
        assert!(parse_binding("foo").is_err());
        assert!(parse_binding("one-shot tap-hold a ; b ; 100").is_err());
    }

    #[test]
    fn rejects_unknown_layer() {
        let err = parse_config("layers:\n- name: a\n  bindings:\n    x: layer nope\n").unwrap_err();
        assert!(err.contains("unknown layer"), "{err}");
    }
}
