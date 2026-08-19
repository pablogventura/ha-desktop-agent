# Agent notes

Public Rust crate (`ha-desktop-agent`). Code, comments, and docs in **English**. Conventional Commits on `main`. MIT.

User-session Linux agent: collect telemetry, map to a stable entity model, publish **one** Home Assistant MQTT Discovery device. ESPHome Native API and Windows are future transports/OS backends; keep collectors independent of MQTT.

## Stack

- Rust 2021, Tokio, `rumqttc`, Serde YAML/JSON
- Linux: `zbus`, `if-addrs`, `libc`, NVML (`nvml-wrapper`), PipeWire via `wpctl`
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
```

Install (from README): `install -D target/release/ha-desktop-agent ~/.local/bin/ha-desktop-agent`

- **dev server / migrate / package scripts:** not in this repo
- **CI:** none (do not add GitHub Actions unless asked)

## Layout

- `src/main.rs` - clap (`run` default, `validate`)
- `src/app.rs` - poll loop, publish, commands
- `src/config.rs` - YAML + `HA_DESKTOP_MQTT_PASSWORD`
- `src/entity.rs` - entity registry / discovery metadata
- `src/snapshot.rs` - values + `attrs`; hysteresis
- `src/collect/` - OS collectors (`proc`, `net`, `disk`, `audio`, `linux_session`, `nvidia`, `rapl`, ...)
- `src/action.rs` - allowlisted switches/buttons + `commands:` argv
- `src/transport/` - MQTT + discovery
- `src/power.rs` - linear estimated watts
- `config.example.yaml` - committed template
- `systemd/ha-desktop-agent.service` - `systemd --user`
- `fixtures/` - `/proc` samples for tests
- `tests/` - `mqtt_discovery.rs`, `proc_fixtures.rs`

User config lives at `~/.config/ha-desktop-agent/config.yaml` (`chmod 600`). That path is **not** in git (`config.yaml` is gitignored).

## Conventions

- Identifiers, logs, and entity ids in English (`snake_case` ids, HA names in entity meta)
- Data-size sensors: decimal **GB** (1e9 bytes), not raw bytes
- HA entity **state** max 255 chars (`truncate_ha_state`); extra lists go in JSON `attrs`
- Missing optional strings (IPs, SSID, sink): `Value::Unavailable` / JSON `null`, not `"none"`
- New MQTT entities: add to `enabled_entities` / `static_entities`, collect every poll tick (do not omit keys on fast ticks)
- Linux-only modules behind `cfg(target_os = "linux")`
- Power actions `shutdown` / `reboot` / `hibernate` stay **off** unless config enables them

## Tests

Unit tests live next to the code (`#[cfg(test)]` in `src/`). Integration tests in `tests/` plus `fixtures/`.

```bash
cargo test
cargo test --lib collect::net
```

No live MQTT/broker required for tests.

## Do not

- Commit `config.yaml`, passwords, MQTT hosts from a real LAN, or calibration CSVs
- Interpolate MQTT payloads into a shell; only fixed `commands:` argv and allowlisted actions
- Run the agent as root to read RAPL/sysfs
- Add CI workflows or AI attribution
- Walk sysfs RAPL via generic directory recursion (`device` symlinks loop)
- Treat logind `delay` inhibitors as suspend blocks (only `block` / `block-weak`)
- Default-enable `sensors.mpris` (media metadata is sensitive)
- Default-listen on port 8080
- Duplicate README feature lists here; keep this file operational
