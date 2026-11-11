//! Key names and codes.
//!
//! Internally every key is identified by its Linux evdev key code, exactly like the original
//! (Linux) mouseless. This keeps config files compatible between both versions, and the codes are
//! based on the physical key position (PC scan codes), so they do not depend on the keyboard layout.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The wildcard key `_`, which matches any key that is not mapped in a layer.
pub const WILDCARD: u16 = 10000;

pub const KEY_ESC: u16 = 1;

/// All key names, in the order of preference for reverse lookups (code -> name).
/// The names and codes are the same as in the original mouseless.
const KEY_NAMES: &[(&str, u16)] = &[
    ("_", WILDCARD),
    ("reserved", 0),
    ("esc", 1),
    ("k1", 2),
    ("k2", 3),
    ("k3", 4),
    ("k4", 5),
    ("k5", 6),
    ("k6", 7),
    ("k7", 8),
    ("k8", 9),
    ("k9", 10),
    ("k0", 11),
    ("minus", 12),
    ("equal", 13),
    ("backspace", 14),
    ("tab", 15),
    ("q", 16),
    ("w", 17),
    ("e", 18),
    ("r", 19),
    ("t", 20),
    ("y", 21),
    ("u", 22),
    ("i", 23),
    ("o", 24),
    ("p", 25),
    ("leftbrace", 26),
    ("rightbrace", 27),
    ("enter", 28),
    ("leftctrl", 29),
    ("a", 30),
    ("s", 31),
    ("d", 32),
    ("f", 33),
    ("g", 34),
    ("h", 35),
    ("j", 36),
    ("k", 37),
    ("l", 38),
    ("semicolon", 39),
    ("apostrophe", 40),
    ("grave", 41),
    ("leftshift", 42),
    ("backslash", 43),
    ("z", 44),
    ("x", 45),
    ("c", 46),
    ("v", 47),
    ("b", 48),
    ("n", 49),
    ("m", 50),
    ("comma", 51),
    ("dot", 52),
    ("slash", 53),
    ("rightshift", 54),
    ("kpasterisk", 55),
    ("leftalt", 56),
    ("space", 57),
    ("capslock", 58),
    ("f1", 59),
    ("f2", 60),
    ("f3", 61),
    ("f4", 62),
    ("f5", 63),
    ("f6", 64),
    ("f7", 65),
    ("f8", 66),
    ("f9", 67),
    ("f10", 68),
    ("numlock", 69),
    ("scrolllock", 70),
    ("kp7", 71),
    ("kp8", 72),
    ("kp9", 73),
    ("kpminus", 74),
    ("kp4", 75),
    ("kp5", 76),
    ("kp6", 77),
    ("kpplus", 78),
    ("kp1", 79),
    ("kp2", 80),
    ("kp3", 81),
    ("kp0", 82),
    ("kpdot", 83),
    ("zenkakuhankaku", 85),
    ("102nd", 86),
    ("f11", 87),
    ("f12", 88),
    ("ro", 89),
    ("katakana", 90),
    ("hiragana", 91),
    ("henkan", 92),
    ("katakanahiragana", 93),
    ("muhenkan", 94),
    ("kpjpcomma", 95),
    ("kpenter", 96),
    ("rightctrl", 97),
    ("kpslash", 98),
    ("sysrq", 99),
    ("rightalt", 100),
    ("linefeed", 101),
    ("home", 102),
    ("up", 103),
    ("pageup", 104),
    ("left", 105),
    ("right", 106),
    ("end", 107),
    ("down", 108),
    ("pagedown", 109),
    ("insert", 110),
    ("delete", 111),
    ("macro", 112),
    ("mute", 113),
    ("volumedown", 114),
    ("volumeup", 115),
    ("power", 116),
    ("kpequal", 117),
    ("kpplusminus", 118),
    ("pause", 119),
    ("scale", 120),
    ("kpcomma", 121),
    ("hangeul", 122),
    ("hanguel", 122),
    ("hanja", 123),
    ("yen", 124),
    ("leftmeta", 125),
    ("rightmeta", 126),
    ("compose", 127),
    ("stop", 128),
    ("again", 129),
    ("props", 130),
    ("undo", 131),
    ("front", 132),
    ("copy", 133),
    ("open", 134),
    ("paste", 135),
    ("find", 136),
    ("cut", 137),
    ("help", 138),
    ("menu", 139),
    ("calc", 140),
    ("setup", 141),
    ("sleep", 142),
    ("wakeup", 143),
    ("file", 144),
    ("sendfile", 145),
    ("deletefile", 146),
    ("xfer", 147),
    ("prog1", 148),
    ("prog2", 149),
    ("www", 150),
    ("msdos", 151),
    ("coffee", 152),
    ("screenlock", 152),
    ("rotate_display", 153),
    ("direction", 153),
    ("cyclewindows", 154),
    ("mail", 155),
    ("bookmarks", 156),
    ("computer", 157),
    ("back", 158),
    ("forward", 159),
    ("closecd", 160),
    ("ejectcd", 161),
    ("ejectclosecd", 162),
    ("nextsong", 163),
    ("playpause", 164),
    ("previoussong", 165),
    ("stopcd", 166),
    ("record", 167),
    ("rewind", 168),
    ("phone", 169),
    ("iso", 170),
    ("config", 171),
    ("homepage", 172),
    ("refresh", 173),
    ("exit", 174),
    ("move", 175),
    ("edit", 176),
    ("scrollup", 177),
    ("scrolldown", 178),
    ("kpleftparen", 179),
    ("kprightparen", 180),
    ("new", 181),
    ("redo", 182),
    ("f13", 183),
    ("f14", 184),
    ("f15", 185),
    ("f16", 186),
    ("f17", 187),
    ("f18", 188),
    ("f19", 189),
    ("f20", 190),
    ("f21", 191),
    ("f22", 192),
    ("f23", 193),
    ("f24", 194),
    ("playcd", 200),
    ("pausecd", 201),
    ("prog3", 202),
    ("prog4", 203),
    ("dashboard", 204),
    ("suspend", 205),
    ("close", 206),
    ("play", 207),
    ("fastforward", 208),
    ("bassboost", 209),
    ("print", 210),
    ("hp", 211),
    ("camera", 212),
    ("sound", 213),
    ("question", 214),
    ("email", 215),
    ("chat", 216),
    ("search", 217),
    ("connect", 218),
    ("finance", 219),
    ("sport", 220),
    ("shop", 221),
    ("alterase", 222),
    ("cancel", 223),
    ("brightnessdown", 224),
    ("brightnessup", 225),
    // additional aliases with Windows naming, only used for parsing
    ("leftwin", 125),
    ("rightwin", 126),
    ("apps", 127),
    ("printscreen", 99),
];

const MODIFIERS: &[u16] = &[
    42,  // leftshift
    54,  // rightshift
    29,  // leftctrl
    97,  // rightctrl
    56,  // leftalt
    100, // rightalt
    125, // leftmeta
    126, // rightmeta
];

pub fn is_modifier(code: u16) -> bool {
    MODIFIERS.contains(&code)
}

fn name_to_code() -> &'static HashMap<&'static str, u16> {
    static MAP: OnceLock<HashMap<&'static str, u16>> = OnceLock::new();
    MAP.get_or_init(|| KEY_NAMES.iter().copied().collect())
}

fn code_to_name() -> &'static HashMap<u16, &'static str> {
    static MAP: OnceLock<HashMap<u16, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        for &(name, code) in KEY_NAMES {
            m.entry(code).or_insert(name);
        }
        m
    })
}

/// Parses a key, which can be either a key name or the key code itself.
pub fn parse_key(key: &str) -> Result<u16, String> {
    let key = key.trim();
    if let Some(&code) = name_to_code().get(key.to_lowercase().as_str()) {
        return Ok(code);
    }
    if let Ok(code) = key.parse::<u16>() {
        return Ok(code);
    }
    Err(format!("'{key}' is neither a key code nor a key name"))
}

pub fn key_name(code: u16) -> Option<&'static str> {
    code_to_name().get(&code).copied()
}

pub fn key_name_or_unknown(code: u16) -> &'static str {
    key_name(code).unwrap_or("unknown")
}

/// Prints all key names that can be used in the config.
pub fn print_key_names() {
    let mut by_code: Vec<(u16, Vec<&str>)> = Vec::new();
    for &(name, code) in KEY_NAMES {
        if code == WILDCARD {
            continue;
        }
        match by_code.iter_mut().find(|(c, _)| *c == code) {
            Some((_, names)) => names.push(name),
            None => by_code.push((code, vec![name])),
        }
    }
    by_code.sort_by_key(|(c, _)| *c);
    println!("code   name(s)");
    for (code, names) in by_code {
        let supported = if windows_key(code).is_some() { "" } else { "   (no Windows mapping)" };
        println!("{:<6} {}{}", code, names.join(", "), supported);
    }
    println!("_      wildcard: matches any key that is not mapped in the layer");
}

// ---------------------------------------------------------------------------------------------
// Windows mapping
// ---------------------------------------------------------------------------------------------

/// How a key is represented on Windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WinKey {
    /// The (set 1) scan code without the E0 prefix.
    pub scan: u16,
    /// Whether the scan code has the E0 prefix ("extended key").
    pub extended: bool,
    /// If not 0, the key is sent with this virtual key code instead of its scan code. This is used
    /// for keys like media keys or pause, which do not work reliably when sent as scan code.
    pub vk: u16,
}

const fn k(scan: u16, extended: bool, vk: u16) -> WinKey {
    WinKey { scan, extended, vk }
}

// virtual key codes used below
const VK_PAUSE: u32 = 0x13;
const VK_SNAPSHOT: u32 = 0x2C;
const VK_NUMLOCK: u32 = 0x90;
const VK_RSHIFT: u32 = 0xA1;

/// Mapping of evdev codes which are not simply "evdev code == non-extended scan code".
const SPECIAL_KEYS: &[(u16, WinKey)] = &[
    (69, k(0x45, true, VK_NUMLOCK as u16)), // numlock
    (119, k(0x45, false, VK_PAUSE as u16)), // pause
    (86, k(0x56, false, 0)),                // 102nd
    (87, k(0x57, false, 0)),                // f11
    (88, k(0x58, false, 0)),                // f12
    (89, k(0x73, false, 0)),                // ro
    (92, k(0x79, false, 0)),                // henkan
    (93, k(0x70, false, 0)),                // katakanahiragana
    (94, k(0x7B, false, 0)),                // muhenkan
    (117, k(0x59, false, 0)),               // kpequal
    (121, k(0x7E, false, 0)),               // kpcomma
    (122, k(0x72, false, 0)),               // hangeul
    (123, k(0x71, false, 0)),               // hanja
    (124, k(0x7D, false, 0)),               // yen
    (183, k(0x64, false, 0)),               // f13
    (184, k(0x65, false, 0)),               // f14
    (185, k(0x66, false, 0)),               // f15
    (186, k(0x67, false, 0)),               // f16
    (187, k(0x68, false, 0)),               // f17
    (188, k(0x69, false, 0)),               // f18
    (189, k(0x6A, false, 0)),               // f19
    (190, k(0x6B, false, 0)),               // f20
    (191, k(0x6C, false, 0)),               // f21
    (192, k(0x6D, false, 0)),               // f22
    (193, k(0x6E, false, 0)),               // f23
    (194, k(0x76, false, 0)),               // f24
    // extended keys (E0 prefix)
    (96, k(0x1C, true, 0)),             // kpenter
    (97, k(0x1D, true, 0)),             // rightctrl
    (98, k(0x35, true, 0)),             // kpslash
    (99, k(0x37, true, VK_SNAPSHOT as u16)), // sysrq / print screen
    (100, k(0x38, true, 0)),            // rightalt
    (102, k(0x47, true, 0)),            // home
    (103, k(0x48, true, 0)),            // up
    (104, k(0x49, true, 0)),            // pageup
    (105, k(0x4B, true, 0)),            // left
    (106, k(0x4D, true, 0)),            // right
    (107, k(0x4F, true, 0)),            // end
    (108, k(0x50, true, 0)),            // down
    (109, k(0x51, true, 0)),            // pagedown
    (110, k(0x52, true, 0)),            // insert
    (111, k(0x53, true, 0)),            // delete
    (125, k(0x5B, true, 0)),            // leftmeta
    (126, k(0x5C, true, 0)),            // rightmeta
    (127, k(0x5D, true, 0)),            // compose (application / menu key)
    (116, k(0x5E, true, 0)),            // power
    (143, k(0x63, true, 0)),            // wakeup
    // media and browser keys, sent as virtual keys
    (113, k(0x20, true, 0xAD)), // mute
    (114, k(0x2E, true, 0xAE)), // volumedown
    (115, k(0x30, true, 0xAF)), // volumeup
    (163, k(0x19, true, 0xB0)), // nextsong
    (165, k(0x10, true, 0xB1)), // previoussong
    (166, k(0x24, true, 0xB2)), // stopcd
    (164, k(0x22, true, 0xB3)), // playpause
    (155, k(0x6C, true, 0xB4)), // mail
    (157, k(0x6B, true, 0xB6)), // computer
    (140, k(0x21, true, 0xB7)), // calc
    (158, k(0x6A, true, 0xA6)), // back
    (159, k(0x69, true, 0xA7)), // forward
    (173, k(0x67, true, 0xA8)), // refresh
    (128, k(0x68, true, 0xA9)), // stop
    (217, k(0x65, true, 0xAA)), // search
    (156, k(0x66, true, 0xAB)), // bookmarks
    (172, k(0x32, true, 0xAC)), // homepage
    (142, k(0x5F, true, 0x5F)), // sleep
];

/// Returns how the given evdev key code is represented on Windows.
pub fn windows_key(code: u16) -> Option<WinKey> {
    if let Some((_, key)) = SPECIAL_KEYS.iter().find(|(c, _)| *c == code) {
        return Some(*key);
    }
    // for most keys, the evdev code is the same as the (non-extended) scan code
    if (1..=83).contains(&code) {
        return Some(k(code, false, 0));
    }
    None
}

/// Converts a key event from the Windows low-level keyboard hook to an evdev key code.
pub fn from_windows(vk: u32, scan: u32, extended: bool) -> Option<u16> {
    // keys that cannot be identified by their scan code
    match vk {
        VK_PAUSE => return Some(119),
        VK_NUMLOCK => return Some(69),
        VK_RSHIFT => return Some(54),
        VK_SNAPSHOT => return Some(99),
        _ => {}
    }
    if scan == 0x54 && !extended {
        // alt + print screen
        return Some(99);
    }
    let scan = scan as u16;
    let lookup = |ext: bool| -> Option<u16> {
        if !ext && (1..=83).contains(&scan) && scan != 69 {
            return Some(scan);
        }
        SPECIAL_KEYS
            .iter()
            .find(|(_, w)| w.scan == scan && w.extended == ext && scan != 0)
            .map(|(c, _)| *c)
    };
    if let Some(code) = lookup(extended) {
        return Some(code);
    }
    // some keys are reported with an unexpected extended flag, e.g. by some drivers
    if scan != 0 {
        if let Some(code) = lookup(!extended) {
            return Some(code);
        }
    }
    // keys without a scan code (e.g. media keys from vendor software): use the virtual key code
    SPECIAL_KEYS
        .iter()
        .find(|(_, w)| w.vk != 0 && w.vk as u32 == vk)
        .map(|(c, _)| *c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_mapped_keys() {
        for &(_, code) in KEY_NAMES {
            if let Some(w) = windows_key(code) {
                let vk = if w.vk != 0 { w.vk as u32 } else { 0xFFFF };
                assert_eq!(from_windows(vk, w.scan as u32, w.extended), Some(code), "code {code}");
            }
        }
    }

    #[test]
    fn parse_names() {
        assert_eq!(parse_key("a"), Ok(30));
        assert_eq!(parse_key(" LeftShift "), Ok(42));
        assert_eq!(parse_key("leftwin"), Ok(125));
        assert_eq!(parse_key("57"), Ok(57));
        assert!(parse_key("nokey").is_err());
        assert_eq!(key_name(125), Some("leftmeta"));
        assert_eq!(key_name(122), Some("hangeul"));
    }

    #[test]
    fn numpad_vs_navigation() {
        assert_eq!(from_windows(0x26, 0x48, true), Some(103)); // up
        assert_eq!(from_windows(0x68, 0x48, false), Some(72)); // kp8
        assert_eq!(from_windows(0x26, 0x48, false), Some(72)); // kp8 with numlock off
        assert_eq!(from_windows(0x0D, 0x1C, true), Some(96)); // kpenter
        assert_eq!(from_windows(0x0D, 0x1C, false), Some(28)); // enter
    }
}
