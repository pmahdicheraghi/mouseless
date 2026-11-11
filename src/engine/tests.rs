//! End-to-end tests of the engine. Most scenarios are taken from the tests of the original mouseless,
//! but here the resulting outputs are checked instead of the internal bindings.
//!
//! Script syntax: `Pa` press a, `Ra` release a, `Ta` auto-repeat of a, `15` advance time by 15ms.
//! Output syntax: `+a` key a down, `-a` key a up, `+left!`/`-left!` mouse button, `exec:<cmd>`,
//! `move` for any mouse movement, `wheel` for any scrolling.

use super::*;
use crate::config::parse_config;
use crate::keys::{key_name, parse_key};

fn run_with(engine: &mut Engine, script: &str) -> String {
    let t0 = Instant::now();
    let mut t = 0u64;
    let mut outs = Vec::new();
    for tok in script.split_whitespace() {
        if let Ok(ms) = tok.parse::<u64>() {
            t += ms;
            outs.extend(engine.on_timer(t0 + Duration::from_millis(t)));
            continue;
        }
        let (kind, name) = tok.split_at(1);
        let code = parse_key(name).unwrap();
        let time = t0 + Duration::from_millis(t);
        match kind {
            "P" => outs.extend(engine.handle_event(Event { code, is_press: true, time })),
            "R" => outs.extend(engine.handle_event(Event { code, is_press: false, time })),
            "T" => outs.extend(engine.handle_repeat(code)),
            _ => panic!("invalid token {tok}"),
        }
    }
    let mut res: Vec<String> = Vec::new();
    for o in outs {
        let s = match o {
            Output::Key { code, down } => format!("{}{}", if down { "+" } else { "-" }, key_name(code).unwrap()),
            Output::Button { button, down } => {
                format!("{}{}!", if down { "+" } else { "-" }, format!("{button:?}").to_lowercase())
            }
            Output::Exec { command, .. } => format!("exec:{command}"),
            Output::MouseMove { .. } => "move".into(),
            Output::Wheel { .. } => "wheel".into(),
        };
        // collapse consecutive mouse outputs
        if (s == "move" || s == "wheel") && res.last() == Some(&s) {
            continue;
        }
        res.push(s);
    }
    res.join(" ")
}

fn check(config: &str, cases: &[(&str, &str)]) {
    for (script, expected) in cases {
        let mut engine = Engine::new(parse_config(config).unwrap());
        let got = run_with(&mut engine, script);
        assert_eq!(&got, expected, "script: {script}");
    }
}

#[test]
fn pass_through() {
    check(
        "layers:\n- name: 1\n  bindings:\n    a: b\n",
        &[("Pc Rc", "+c -c"), ("Pa Ra", "+b -b"), ("Pa Pc Ra Rc", "+b +c -b -c")],
    );
}

#[test]
fn tap_hold() {
    let config = r#"
layers:
- name: 1
  bindings:
    a: tap-hold a ; x ; 10
    b: tap-hold b ; toggle-layer 2 ; 10
    c: c
- name: 2
  bindings:
    c: m
"#;
    check(
        config,
        &[
            ("Pa Ra", "+a -a"),
            ("Pa 5 Ra", "+a -a"),
            ("Pa 15 Ra", "+x -x"),
            ("Pa 15 Ra Pa Ra", "+x -x +a -a"),
            ("Pa Pc Ra Rc", "+a +c -a -c"),
            ("Pa Pc Rc Ra", "+a +c -c -a"),
            ("Pa Pc Rc 15 Ra", "+x +c -c -x"),
            ("Pc Pa Rc 15 Ra", "+c -c +x -x"),
            ("Pb 5 Rb", "+b -b"),
            ("Pb 15 Pc Rc Rb Pc Rc", "+m -m +c -c"),
        ],
    );
}

#[test]
fn tap_hold_next() {
    let config = r#"
layers:
- name: 1
  bindings:
    a: tap-hold-next a ; x ; 10
    b: tap-hold b ; y ; 10
    c: c
"#;
    check(
        config,
        &[
            ("Pa Ra", "+a -a"),
            ("Pa 15 Ra", "+x -x"),
            ("Pa Pc Ra Rc", "+x +c -x -c"),
            ("Pc Pa Rc Ra", "+c -c +a -a"),
            ("Pa Pb Ra Rb", "+x -x +b -b"),
            ("Pb Pa 15 Rb Ra", "+y +x -y -x"),
        ],
    );
}

#[test]
fn tap_hold_next_release() {
    let config = r#"
layers:
- name: 1
  bindings:
    a: tap-hold-next-release a ; x ; 10
    c: c
"#;
    check(
        config,
        &[
            ("Pa Ra", "+a -a"),
            ("Pa Pc Ra Rc", "+a +c -a -c"),
            ("Pa Pc Rc Ra", "+x +c -c -x"),
            ("Pc Pa Rc Ra", "+c -c +a -a"),
        ],
    );
}

#[test]
fn quick_tap() {
    let config = r#"
quickTapTime: 10
layers:
- name: 1
  bindings:
    a: tap-hold a ; x ; 20
"#;
    check(
        config,
        &[
            ("Pa 5 Ra Pa 30 Ra", "+a -a +a -a"),
            ("Pa 15 Ra Pa 30 Ra", "+a -a +x -x"),
            // quick tap followed by holding auto-repeats the tap key
            ("Pa 5 Ra Pa Ta Ta Ra", "+a -a +a +a +a -a"),
        ],
    );
}

#[test]
fn combo() {
    let config = r#"
comboTime: 10
layers:
- name: 1
  bindings:
    a+b: x
    c: c
"#;
    check(
        config,
        &[
            ("Pa Ra", "+a -a"),
            ("Pa Ra Pb Rb", "+a -a +b -b"),
            ("Pa 15 Pb Ra Rb", "+a +b -a -b"),
            ("Pa Pc Pb Ra Rb Rc", "+a +c +b -a -b -c"),
            ("Pa Pb Ra Rb", "+x -x"),
            ("Pb Pa Rb Ra", "+x -x"),
        ],
    );
}

#[test]
fn one_shot() {
    let config = r#"
layers:
- name: 1
  bindings:
    a: one-shot leftshift
    b: one-shot toggle-layer 2
- name: 2
  bindings:
    c: x
"#;
    check(
        config,
        &[
            ("Pc Rc", "+c -c"),
            ("Pa Ra Pc Rc", "+leftshift +c -leftshift -c"),
            ("Pa Ra Pc Rc Pd Rd", "+leftshift +c -leftshift -c +d -d"),
            ("Pa Pc Rc Ra", "+leftshift +c -c -leftshift"),
            ("Pa Ra Pa Ra Pc Rc Pd Rd Pa Ra", "+leftshift +c -c +d -d -leftshift"),
            ("Pb Rb Pc Rc Pc Rc", "+x -x +c -c"),
        ],
    );
}

#[test]
fn layers_and_escape() {
    let config = r#"
layers:
- name: initial
  bindings:
    f: layer mouse
    t: toggle-layer arrows
- name: mouse
  bindings:
    q: layer initial
    i: up
- name: arrows
  passThrough: false
  bindings:
    e: up
    _: rightalt+_
"#;
    check(
        config,
        &[
            ("Pf Rf Pi Ri Pq Rq Pi Ri", "+up -up +i -i"),
            // escape returns to the first layer
            ("Pf Rf Pesc Resc Pi Ri", "+i -i"),
            ("Pesc Resc", "+esc -esc"),
            // toggle-layer is active while held, the wildcard binding applies to unmapped keys
            ("Pt Pe Re Px Rx Rt Pe Re", "+up -up +rightalt +x -rightalt -x +e -e"),
        ],
    );
}

#[test]
fn no_pass_through_swallows_keys() {
    check(
        "layers:\n- name: 1\n  passThrough: false\n  bindings:\n    a: b\n",
        &[("Pc Rc Pa Ra", "+b -b")],
    );
}

#[test]
fn mod_layer() {
    let config = r#"
layers:
- name: 1
  bindings:
    leftctrl: mod-layer leftctrl vim
- name: vim
  bindings:
    h: left
"#;
    check(
        config,
        &[
            ("Pleftctrl Pc Rc Rleftctrl", "+leftctrl +c -c -leftctrl"),
            ("Pleftctrl Ph Rh Rleftctrl", "+leftctrl -leftctrl +left -left +leftctrl -leftctrl"),
        ],
    );
}

#[test]
fn key_combos_and_repeat() {
    let config = r#"
layers:
- name: 1
  bindings:
    a: leftshift+k1
    m: button left
"#;
    check(
        config,
        &[
            ("Pa Ta Ra", "+leftshift +k1 +k1 -leftshift -k1"),
            // keys without a key binding are not repeated
            ("Pm Tm Rm", "+left! -left!"),
        ],
    );
}

#[test]
fn exec_and_buttons() {
    let config = r#"
layers:
- name: 1
  enterCommand: "echo enter"
  bindings:
    e: exec notepad.exe
    p: exec-press-release echo down ; echo up
    l: button left
    r: button right 10
- name: 2
"#;
    check(
        config,
        &[
            ("Pe Re", "exec:notepad.exe"),
            ("Pp Rp", "exec:echo down exec:echo up"),
            ("Pl Rl", "+left! -left!"),
            ("Pr 10 10 Rr 20", "+right! -right! +right! -right! +right! -right!"),
        ],
    );
}

#[test]
fn mouse_moves_and_stops() {
    let config = r#"
baseMouseSpeed: 1000
baseScrollSpeed: 20
mouseLoopInterval: 10
layers:
- name: 1
  bindings:
    l: move 1 0
    p: scroll up
"#;
    let mut engine = Engine::new(parse_config(config).unwrap());
    let t0 = Instant::now();
    engine.handle_event(Event { code: parse_key("l").unwrap(), is_press: true, time: t0 });
    let deadline = engine.next_deadline().expect("mouse loop must be running");
    let outs = engine.on_timer(deadline + Duration::from_millis(10));
    match outs.as_slice() {
        [Output::MouseMove { dx, dy }] => assert!(*dx > 0 && *dy == 0, "dx={dx} dy={dy}"),
        o => panic!("unexpected outputs {o:?}"),
    }
    engine.handle_event(Event { code: parse_key("l").unwrap(), is_press: false, time: t0 });
    // without deceleration the mouse stops immediately, and the mouse loop stops afterwards
    for _ in 0..3 {
        let Some(next) = engine.next_deadline() else { break };
        assert!(engine.on_timer(next + Duration::from_millis(10)).is_empty());
    }
    assert_eq!(engine.next_deadline(), None);

    engine.handle_event(Event { code: parse_key("p").unwrap(), is_press: true, time: t0 });
    let next = engine.next_deadline().unwrap();
    let outs = engine.on_timer(next + Duration::from_millis(100));
    assert!(matches!(outs.as_slice(), [Output::Wheel { horizontal: false, delta }] if *delta > 0), "{outs:?}");
}

#[test]
fn release_all() {
    let mut engine = Engine::new(parse_config("layers:\n- name: 1\n  bindings:\n    m: button left\n").unwrap());
    let t = Instant::now();
    engine.handle_event(Event { code: parse_key("a").unwrap(), is_press: true, time: t });
    engine.handle_event(Event { code: parse_key("m").unwrap(), is_press: true, time: t });
    let outs = engine.release_all();
    assert!(outs.contains(&Output::Key { code: 30, down: false }));
    assert!(outs.contains(&Output::Button { button: MouseButton::Left, down: false }));
}

#[test]
fn move_towards_accelerates() {
    // linear acceleration from 0 to 10 in 4 steps
    let mut v = 0.0;
    let mut speeds = Vec::new();
    for _ in 0..5 {
        v = move_towards(v, 10.0, 10.0, 0.0, 1.0, 0.25, 1.0, 1.0);
        speeds.push(v);
    }
    assert_eq!(speeds, vec![2.5, 5.0, 7.5, 10.0, 10.0]);
    // deceleration
    assert_eq!(move_towards(10.0, 0.0, 10.0, 0.0, 1.0, 0.25, 1.0, 0.5), 5.0);
    assert_eq!(move_towards(-10.0, 0.0, 10.0, 0.0, 1.0, 0.25, 1.0, 0.5), -5.0);
}
