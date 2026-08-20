use ha_desktop_agent::config::Config;
use ha_desktop_agent::transport::discovery_payload;

#[test]
fn golden_device_discovery_contains_expected_components() {
    let yaml = include_str!("../config.example.yaml");
    let config: Config = serde_yaml::from_str(yaml).unwrap();
    let payload = discovery_payload(&config, "testdevice01");
    let encoded = serde_json::to_string_pretty(&payload).unwrap();
    assert!(encoded.contains("\"p\": \"sensor\""));
    assert!(payload["cmps"]["cpu_usage"]["unique_id"] == "testdevice01_cpu_usage");
    assert!(payload["cmps"]["uptime"]["unit_of_measurement"] == "h");
    assert!(payload["cmps"]["gpu_power"]["device_class"] == "power");
    assert!(payload["cmps"]["caffeine"]["p"] == "switch");
    assert!(payload["cmps"]["lock"]["p"] == "button");
    assert!(payload["cmps"]["hibernate"]["p"] == "button");
    assert!(payload["cmps"]["shutdown"]["p"] == "button");
    assert!(payload["cmps"]["reboot"]["p"] == "button");
    assert!(payload["cmps"]["active_window_title"]["p"] == "sensor");
    assert!(payload["dev"]["ids"][0] == "testdevice01");
    assert!(payload["cmps"]["ssh_listening"]["p"] == "binary_sensor");
    assert!(payload["cmps"]["tailscale_ip"]["p"] == "sensor");
    assert!(payload["cmps"]["lan_ip"]["p"] == "sensor");
    assert!(payload["cmps"]["disk_root_free"]["p"] == "sensor");
    assert!(payload["cmps"]["chassis"]["p"] == "sensor");
    assert!(payload["cmps"]["battery_percent"]["p"] == "sensor");
    assert!(payload["cmps"]["ac_power"]["p"] == "binary_sensor");
    assert!(payload["cmps"]["mute"]["p"] == "switch");
    assert!(payload["cmps"]["do_not_disturb"]["p"] == "switch");
    assert!(payload["cmps"]["notify_message"]["p"] == "notify");
    assert!(payload["cmps"]["notify_urgent"]["p"] == "notify");
    assert!(payload["cmps"].get("notify").is_none());
    assert!(payload["cmps"]["media_play_pause"]["p"] == "button");
    assert!(payload["cmps"].get("http_alt_listening").is_none());
    assert!(payload["cmps"]["update_available"]["p"] == "binary_sensor");
    assert!(payload["cmps"]["update_auto"]["p"] == "switch");
    assert!(payload["cmps"]["apply_update"]["p"] == "button");
    assert!(
        payload["cmps"]["cpu_usage"]["json_attributes_topic"] == "ha-desktop/testdevice01/state"
    );
}
