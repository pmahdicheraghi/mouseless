# mouseless for Windows

Control the mouse pointer with the keyboard on Windows. This is a Rust port of
[mouseless](https://github.com/jbensmann/mouseless) by Johannes Bensmann, which only runs on Linux.

It uses the **same config format** as the original, so configs can be shared between Linux and Windows.
All features are supported: smooth pointer movement with acceleration, speed modifiers, clicks, drag,
scrolling, key remapping, layers, `tap-hold`, `tap-hold-next`, `tap-hold-next-release`, combos (`f+d`),
`mod-layer`, `one-shot`, `multi`, `exec`, `exec-press-release`, `reload-config` and `nop`.

## Build

```
cargo build --release
```

The binary is `target\release\mouseless.exe`. It has no runtime dependencies.

## Usage

```
mouseless.exe                     # uses %APPDATA%\mouseless\config.yaml (created on first start)
mouseless.exe --config my.yaml    # use another config file
mouseless.exe --debug             # show the names of the keys you press, useful for writing a config
mouseless.exe --list-keys         # list all key names
mouseless.exe --check             # only validate the config
mouseless.exe --hidden            # run in the background without a console window
```

Stop it with `Ctrl+C` in its console, with a key bound to the `quit` action, or in the Task Manager if it
runs hidden.

With the default config, hold `a` (or hold `tab` and press another key) to activate the mouse layer:
`i`/`j`/`k`/`l` move the pointer, `f`/`d`/`s` are the left/middle/right buttons, `p`/`n` scroll, and
`leftalt`, `e` and `capslock` change the speed. `esc` or `q` leaves a layer you switched to with `layer`.
See [examples/config_full.yaml](examples/config_full.yaml) for every option.

### Start automatically at login

Press `Win+R`, enter `shell:startup`, and create a shortcut to `mouseless.exe` there. In its properties,
set the target to `"C:\path\to\mouseless.exe" --hidden`.

### Admin windows

Windows doesn't send keyboard events to a program without admin rights while an elevated window
(e.g. Task Manager or an admin terminal) is focused. To use mouseless in those windows too, run it as
administrator, for example with a Task Scheduler task with "Run with highest privileges" enabled.

## Differences from the Linux version

- **Additions:** the `quit` action, `button back` / `button forward` for the mouse side buttons, and
  key name aliases `leftwin`, `rightwin`, `apps` and `printscreen`.
- **Not supported:** the `devices` and `devicesExclude` options. A Windows keyboard hook receives the
  events of all keyboards and can't tell them apart.
- **Commands:** `exec`, `startCommand`, `enterCommand` and `exitCommand` run with `cmd.exe`. The
  environment variables `%key%` and `%key_code%` contain the key that triggered the command.
- **Injected input:** keys sent by other programs are ignored by default, so remapping tools can't
  loop. Pass `--process-injected` to handle them as well, e.g. for remote desktop or on-screen keyboards.
- **Pointer acceleration:** none. The pointer is positioned absolutely, so Windows' "Enhance pointer
  precision" setting doesn't affect it.
- **Key names:** names refer to physical key positions (based on scan codes), like on Linux, so `a` is
  always the key next to `capslock`, whatever your keyboard layout.

## Known limitations

- Keyboard layouts with AltGr generate an extra `leftctrl` event when `rightalt` is pressed. That extra
  event is always passed through, so if you remap `rightalt` on such a layout, apps may see `ctrl` held
  while the key is down.
- `Ctrl+Alt+Del` and `Win+L` are handled by Windows before any program sees them.

## How it works

- A low-level keyboard hook (`WH_KEYBOARD_LL`) receives every key event and decides synchronously
  whether to pass it through or suppress it. Keys that only map to themselves pass through unchanged.
- Everything else, such as remapped keys, mouse movement, clicks and scrolling, is produced with
  `SendInput`. Keys are sent as scan codes, so they don't depend on the keyboard layout.
- Windows doesn't auto-repeat keys sent with `SendInput`, so the auto-repeat of the physical key is used
  to repeat the key it is mapped to.
- A timer thread drives the tap-hold and combo timeouts and the pointer movement, at the rate set by
  `mouseLoopInterval`.

The decision logic in `src/engine.rs` is a port of the original's handler chain (combo → mod-layer →
tap-hold → default → one-shot → executor). It is platform independent and covered by unit tests
(`cargo test`), many of which are taken from the original project.

## License

MIT, like the original. See [LICENSE](LICENSE).
