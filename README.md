# ha-desktop-agent

Background agent that exposes a Linux (and later Windows) desktop to [Home Assistant](https://www.home-assistant.io/) as **one MQTT device**. Telemetry is collected locally, normalized into a stable entity model, and published with MQTT Discovery. Commands are allowlisted in configuration; MQTT payloads are never executed as a shell.

The first version targets a modern systemd Linux desktop (Ubuntu, GNOME, Wayland) with optional NVIDIA GPUs.

## Features (v1)

Sensors include CPU, RAM, swap, RAPL package/DRAM power when readable, NVIDIA GPU metrics, uptime, idle time, chassis type (SMBIOS `/sys/class/dmi/id/chassis_type`: desktop, laptop, ...), session type, desktop environment, optional focused application, process presence (Discord, Ollama, ...), Tailscale and LAN IPv4 addresses, WireGuard, TCP listeners (SSH/VNC/RDP by default), disk usage, LAN throughput, Wi-Fi, audio volume, system battery and AC (not mouse/keyboard HID batteries), and an estimated wall-power model you can calibrate.

Actions (allowlisted): lock, suspend, hibernate, shutdown, reboot, and a caffeine switch that takes a logind inhibit lock. Dangerous power actions are off by default.

## Requirements

- Linux with systemd and a user graphical session
- MQTT broker reachable from the desktop (the Home Assistant Mosquitto add-on is fine)
- Home Assistant MQTT integration with discovery enabled (device discovery, HA 2024.8+)
- Optional: NVIDIA driver (`libnvidia-ml.so`)
- Optional: [Focused Window D-Bus](https://extensions.gnome.org/extension/5592/focused-window-d-bus/) for the active application on GNOME Wayland

Run the agent as the logged-in user (`systemd --user`). A system-wide root service cannot see the GNOME session bus.

## Build

```bash
cargo build --release
install -D target/release/ha-desktop-agent ~/.local/bin/ha-desktop-agent
```

## Configuration

Copy [`config.example.yaml`](config.example.yaml) to `~/.config/ha-desktop-agent/config.yaml` and set the broker host, credentials, and device name.

Network sensors (Linux): Tailscale uses the `tailscale*` interface (or a running `tailscaled` process) and its IPv4. LAN IPv4 is taken from the default-route interface with the lowest metric that is not loopback, docker/veth/bridges, `tun*`, Tailscale, or `wgN`. WireGuard is any `wgN` interface. Listeners are TCP `LISTEN` sockets in `/proc/net/tcp` and `/proc/net/tcp6` for the configured ports. Missing addresses are published as unavailable (JSON null), not the string `none`.

Chassis is always published from SMBIOS `chassis_type`. Battery and AC adapters use `/sys/class/power_supply` with `sensors.battery` (default on). Only `scope=System` batteries (or `BAT*` / `CMB*` when scope is missing) are used; HID mouse/keyboard packs are ignored. Without a system battery, `battery_present` is off and the other battery sensors are unavailable. Status, health, and cycle count are diagnostic entities.

Audio uses `wpctl` on the default PipeWire sink (`mute`, `volume_up` / `volume_down`). Desktop notifications are two MQTT notify entities on the same device: `notify_message` (normal urgency, respects GNOME Do Not Disturb) and `notify_urgent` (critical urgency, GNOME still shows a banner when DND is on). Use Home Assistant `notify.send_message`. A plain payload is the body; `notify.title` in YAML is the default title. An empty payload uses `notify.body`. Optional JSON `{"title":"...","body":"..."}` (or `message` instead of `body`) is allowed; extra keys are ignored and urgency is never taken from JSON. Title and body are capped at 255 characters. Payloads are passed to D-Bus `Notify` only, never to a shell. GNOME Do Not Disturb is the `do_not_disturb` switch (`gsettings` `org.gnome.desktop.notifications show-banners`; ON means banners off). MPRIS media controls are off unless `sensors.mpris: true`.

```bash
chmod 600 ~/.config/ha-desktop-agent/config.yaml
ha-desktop-agent validate --config ~/.config/ha-desktop-agent/config.yaml
```

Password can also come from `HA_DESKTOP_MQTT_PASSWORD`. TLS is optional (`mqtt.tls: true`). Custom commands must be listed as fixed `argv` arrays; the agent never interpolates MQTT payloads into a shell.

### Power model

```text
estimated_w = idle_w + sum(coefficient[k] * feature[k])
```

Features: `cpu_package_w`, `dram_w`, `gpu_w`, `cpu_usage`, `gpu_usage`. Missing features count as zero. Set `power.log_csv` to record timestamps and features while you measure the PC with an external meter, then paste new coefficients back into YAML.

### RAPL permissions

CPU/DRAM watts come from `/sys/class/powercap`. Many distros restrict those files. If `cpu_power` / `dram_power` stay unavailable, either skip them in the model or grant read access with a udev/sysfs rule. The agent does not run as root to work around this.

## systemd --user

```bash
mkdir -p ~/.config/systemd/user
cp systemd/ha-desktop-agent.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now ha-desktop-agent.service
```

The unit starts after `graphical-session.target`.

## MQTT layout

- Discovery (retained): `homeassistant/device/<device_id>/config`
- Availability + LWT: `ha-desktop/<device_id>/availability` (`online` / `offline`)
- State JSON: `ha-desktop/<device_id>/state`
- Commands: `ha-desktop/<device_id>/command/<entity_id>`

`device_id` defaults to the first 12 characters of `/etc/machine-id`.

## Safety

- Shutdown, reboot, and hibernate are disabled until you set them to `true`
- Only registered actions and `commands:` entries can run
- Prefer a dedicated MQTT user with ACL limited to this device prefix

## License

MIT
