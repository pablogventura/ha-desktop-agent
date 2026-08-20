# Agent notes

Public Rust crate (`ha-desktop-agent`). Code, comments, and docs in **English**. Conventional Commits on `main`. MIT.

User-session Linux agent plus Windows service/session helper: collect telemetry, map to a stable entity model, publish **one** Home Assistant MQTT Discovery device. ESPHome Native API is a future transport; keep collectors independent of MQTT.

## Stack

- Rust 2021, Tokio, `rumqttc`, Serde YAML/JSON
- Linux: `zbus`, `if-addrs`, `libc`, NVML (`nvml-wrapper`), PipeWire via `wpctl`, `gsettings` (DND)
- Windows: `windows` crate, `windows-service`, named pipe IPC, WinRT toasts, WASAPI
- Binary: `ha-desktop-agent`. Lib: `src/lib.rs`

## Commands

```bash
cargo build
cargo build --release
cargo test
cargo check
cargo fmt
cargo clippy --all-targets
cargo run -- --config ~/.config/ha-desktop-agent/config.yaml
cargo run -- validate --config ~/.config/ha-desktop-agent/config.yaml
make linux
make windows
make test
make deb
make windows-installer
```

Install (from README): `install -D target/release/ha-desktop-agent ~/.local/bin/ha-desktop-agent`

After a local install: `systemctl --user restart ha-desktop-agent.service`

- **dev server / migrate / package scripts:** not in this repo (Makefile is for build/deb/NSIS only)
- **CI:** none (do not add GitHub Actions unless asked)
- **Version:** SemVer in `Cargo.toml` only; packaging (`make deb` / `make windows-installer` / `make release`) and `agent_version` / discovery `sw` follow it. See `.cursor/rules/versioning.mdc`.
- **Updates:** GitHub Releases via manual `gh release create` (no Actions). Agent checks `update.github_repo` (default `pablogventura/ha-desktop-agent`), verifies Ed25519-signed `SHA256SUMS`, applies `.deb`/`setup.exe`/local binary. MQTT: `update_available`, `update_latest_version`, switch `update_auto` (default on), button `apply_update`.

## Layout

- `src/main.rs` - clap (`run` default, `validate`; Windows `service` / `session`)
- `src/app.rs` - poll loop, publish, commands
- `src/config.rs` - YAML + `HA_DESKTOP_MQTT_PASSWORD` (Windows: `%ProgramData%\ha-desktop-agent\config.yaml`)
- `src/entity.rs` - entity registry / discovery metadata
- `src/snapshot.rs` - values + `attrs`; hysteresis
- `src/ipc.rs` - length-prefixed JSON for the Windows named pipe
- `src/collect/` - OS collectors (`proc`, `net`, `disk`, `audio`, `battery`, `dnd`, `linux_session`, `windows/`, `nvidia`, `rapl`, ...)
- `src/action.rs` - switches/buttons/notify parse + `commands:` argv
- `src/transport/` - MQTT + discovery (`mqtt_bool_template` for binaries)
- `src/power.rs` - linear estimated watts
- `config.example.yaml` - committed template
- `systemd/ha-desktop-agent.service` - `systemd --user` (`~/.local/bin`)
- `packaging/debian/` - `.deb` user unit (`/usr/bin`)
- `installer/windows/ha-desktop-agent.nsi` - NSIS setup.exe
- `installer/windows/install.ps1` - elevated/remote install without UAC hang over SSH
- `fixtures/` - `/proc`, DMI chassis, `power_supply` samples
- `tests/` - `mqtt_discovery.rs`, `proc_fixtures.rs`

User config lives at `~/.config/ha-desktop-agent/config.yaml` (`chmod 600`) on Linux. That path is **not** in git (`config.yaml` is gitignored). Windows uses `%PROGRAMDATA%\ha-desktop-agent\config.yaml`.

## Conventions

- Identifiers, logs, and entity ids in English (`snake_case` ids, HA names in entity meta)
- Data-size sensors: decimal **GB** (1e9 bytes), not raw bytes
- `uptime` is hours (`h`); `/proc/uptime` is still parsed as seconds then divided
- HA entity **state** max 255 chars (`truncate_ha_state`); extra lists go in JSON `attrs`
- Missing optional strings (IPs, SSID, sink): `Value::Unavailable` / JSON `null`, not `"none"`
- Binary MQTT: only map JSON **booleans** to ON/OFF; `null` must stay unknown, not OFF
- New MQTT entities: add to `enabled_entities` / `static_entities`, collect every poll tick (do not omit keys on fast ticks)
- Linux-only modules behind `cfg(target_os = "linux")`
- Power actions `shutdown` / `reboot` / `hibernate` stay **off** unless config enables them
- Notify: MQTT entities `notify_message` (normal) and `notify_urgent` (critical); no notify **button**. Payload is D-Bus `Notify` on Linux or WinRT toast on Windows (plain text or JSON `title`/`body`/`message`). Urgency comes from the entity id, not JSON
- Lock: GNOME/Freedesktop `ScreenSaver.Lock` first; logind `LockSession` only on a **user** seat session, not systemd `--user` `manager`. Windows: session helper `LockWorkStation`
- Chassis: always from DMI `chassis_type` (Linux) or SMBIOS via `GetSystemFirmwareTable` (Windows). Battery: `sensors.battery`; System/`BAT*` only, skip `hidpp*` Device packs. Desktop with no Mains sysfs: `ac_power` true. Windows uses `GetSystemPowerStatus`
- Windows: MQTT lives in the service; session helper talks over a named pipe. Do not add `do_not_disturb` on Windows. RAPL stays Unavailable.

## Tests

Unit tests live next to the code (`#[cfg(test)]` in `src/`). Integration tests in `tests/` plus `fixtures/`.

```bash
cargo test
cargo test --lib collect::battery
```

No live MQTT/broker required for tests. Do not lock the live session or send `notify_urgent` unless asked.

## Do not

- Commit `config.yaml`, passwords, MQTT hosts from a real LAN, or calibration CSVs
- Interpolate MQTT payloads into a shell; only fixed `commands:` argv and allowlisted actions
- Run the agent as root to read RAPL/sysfs
- Add CI workflows or AI attribution
- Walk sysfs RAPL or `power_supply` via generic directory recursion (`device` symlinks loop)
- Treat logind `delay` inhibitors as suspend blocks (only `block` / `block-weak`)
- Count HID mouse/keyboard batteries as the laptop pack
- Default-enable `sensors.mpris` (media metadata is sensitive)
- Default-listen on port 8080
- Duplicate README feature lists here; keep this file operational
