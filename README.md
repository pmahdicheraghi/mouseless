# mouseless for Windows

Control the mouse pointer with the keyboard on Windows. This is a Rust port of
[mouseless](https://github.com/jbensmann/mouseless) by Johannes Bensmann, which only runs on Linux.

It uses the **same config format** as the original, so configs can be shared between Linux and Windows.
All features are supported: smooth pointer movement with acceleration, speed modifiers, clicks, drag,
scrolling, key remapping, layers, `tap-hold`, `tap-hold-next`, `tap-hold-next-release`, combos (`f+d`),
`mod-layer`, `one-shot`, `multi`, `exec`, `exec-press-release`, `reload-config` and `nop`.

## Install

Download or build `mouseless-setup-<version>.exe` (see below) and run it. The installer:

- installs mouseless to `C:\Program Files\mouseless` and adds it to the Start menu
- optionally starts it automatically at login **with administrator rights**, so that it also works
  while an admin window (e.g. Task Manager or an admin terminal) is focused; Windows doesn't send key
  events to normal programs then
- stops a running mouseless before an update, and removes the autostart when uninstalling

Your config (`%APPDATA%\mouseless\config.yaml`) is created on the first start and is kept on uninstall.

## The tray icon

mouseless runs in the background with an icon in the notification area. The icon shows the state:
dark when the first layer is active, **blue** when another layer (e.g. the mouse layer) is active, and
gray when paused. Click it for the menu:

- **Enabled**: pause mouseless; while paused all keys work normally
- **Reload config** after you've edited it, or **Edit config** to open it
- **Open log**: the log file `%LOCALAPPDATA%\mouseless\mouseless.log`
- **Start at login (as administrator)**: creates or removes the scheduled task that starts mouseless
  at login, which asks for administrator rights once
- **Exit**

With the default config, hold `a` (or hold `tab` and press another key) to activate the mouse layer:
`i`/`j`/`k`/`l` move the pointer, `f`/`d`/`s` are the left/middle/right buttons, `p`/`n` scroll, and
`leftalt`, `e` and `capslock` change the speed. `esc` or `q` leaves a layer you switched to with `layer`.
See [examples/config_full.yaml](examples/config_full.yaml) for every option.

## Command line

```
mouseless                       # uses %APPDATA%\mouseless\config.yaml (created on first start)
mouseless --config my.yaml      # use another config file
mouseless --debug               # show the names of the keys you press, useful for writing a config
mouseless --list-keys           # list all key names
mouseless --check               # only validate the config
mouseless --quit                # stop the running mouseless
mouseless --enable-autostart    # start at login as administrator (--disable-autostart to undo)
mouseless --no-tray             # run without the tray icon
```

When started from a terminal, mouseless prints its log there and stops with `Ctrl+C` or when the
terminal is closed. Started from the Start menu, it runs in the background and logs to
`%LOCALAPPDATA%\mouseless\mouseless.log`.

Running as administrator also means that the commands in your config (`exec`, `startCommand`, ...)
run as administrator, so keep the config file only writable by you.

## Build

```
cargo build --release              # target\release\mouseless.exe, no runtime dependencies
.\installer\build.ps1              # target\installer\mouseless-setup-<version>.exe
```

The installer needs [Inno Setup 6](https://jrsoftware.org/isinfo.php)
(`winget install JRSoftware.InnoSetup`). The icons are generated with `python assets/make_icons.py`
(requires Pillow); the results are committed, so this is only needed to change them.

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
