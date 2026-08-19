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
    assert!(payload["cmps"]["gpu_power"]["device_class"] == "power");
    assert!(payload["cmps"]["caffeine"]["p"] == "switch");
    assert!(payload["cmps"]["lock"]["p"] == "button");
    assert!(payload["cmps"].get("hibernate").is_none());
    assert!(payload["cmps"].get("active_window_title").is_none());
    assert!(payload["dev"]["ids"][0] == "testdevice01");
    assert!(payload["state_topic"] == "ha-desktop/testdevice01/state");
}
