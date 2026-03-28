//! MQTT publisher for Home Assistant integration.
//!
//! Publishes WiFi-DensePose sensing data to an MQTT broker using
//! Home Assistant MQTT discovery so entities appear automatically.
//!
//! Discovery prefix: `homeassistant/` (configurable)
//! State prefix:     `ruview/` (configurable via --mqtt-topic-prefix)
//!
//! Entities published per node:
//! - `binary_sensor` — presence (on/off)
//! - `binary_sensor` — fall_detected (on/off)
//! - `sensor`        — breathing_rate_bpm
//! - `sensor`        — heart_rate_bpm
//! - `sensor`        — person_count
//! - `sensor`        — rssi (dBm)
//! - `sensor`        — motion_level (absent/low/medium/high)

use std::time::Duration;
use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS};
use serde_json::json;
use tracing::{debug, error, info, warn};

/// Configuration for the MQTT publisher.
#[derive(Debug, Clone)]
pub struct MqttConfig {
    /// MQTT broker hostname or IP address.
    pub host: String,
    /// MQTT broker port (default 1883).
    pub port: u16,
    /// Optional username for broker authentication.
    pub username: Option<String>,
    /// Optional password for broker authentication.
    pub password: Option<String>,
    /// Topic prefix for state topics (default "ruview").
    pub topic_prefix: String,
    /// Home Assistant discovery prefix (default "homeassistant").
    pub discovery_prefix: String,
    /// Whether to publish HA MQTT discovery messages on connect.
    pub ha_discovery: bool,
    /// Unique device identifier for HA (default "ruview_01").
    pub device_id: String,
    /// Human-readable device name shown in HA (default "RuView Sensor").
    pub device_name: String,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 1883,
            username: None,
            password: None,
            topic_prefix: "ruview".to_string(),
            discovery_prefix: "homeassistant".to_string(),
            ha_discovery: true,
            device_id: "ruview_01".to_string(),
            device_name: "RuView WiFi Sensor".to_string(),
        }
    }
}

/// A single state update to publish over MQTT.
#[derive(Debug, Clone)]
pub struct MqttSensingState {
    pub presence: bool,
    pub fall_detected: bool,
    pub person_count: u32,
    pub breathing_rate_bpm: f64,
    pub heart_rate_bpm: f64,
    pub rssi: i32,
    pub motion_level: String,
}

/// Milliseconds to wait after creating the MQTT event loop before publishing,
/// giving rumqttc time to complete the TCP connection and CONNACK exchange.
const CONNECT_WAIT_MS: u64 = 500;

/// Seconds to wait before reconnecting after an event loop error.
const RECONNECT_DELAY_SECS: u64 = 5;
fn build_client(cfg: &MqttConfig) -> (AsyncClient, EventLoop) {
    let client_id = format!("ruview-sensing-{}", cfg.device_id);
    let mut opts = MqttOptions::new(client_id, &cfg.host, cfg.port);
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_clean_session(true);

    if let (Some(u), Some(p)) = (&cfg.username, &cfg.password) {
        opts.set_credentials(u, p);
    }

    AsyncClient::new(opts, 64)
}

/// Build the HA MQTT discovery payload for each entity.
fn discovery_payloads(cfg: &MqttConfig) -> Vec<(String, String)> {
    let prefix = &cfg.topic_prefix;
    let disc = &cfg.discovery_prefix;
    let dev_id = &cfg.device_id;
    let dev_name = &cfg.device_name;

    let device = json!({
        "identifiers": [dev_id],
        "name": dev_name,
        "model": "ESP32-S3 WiFi CSI Node",
        "manufacturer": "RuView / rUv",
        "sw_version": env!("CARGO_PKG_VERSION"),
    });

    let state_topic = format!("{prefix}/{dev_id}/state");

    let mut payloads = Vec::new();

    // ── binary_sensor: presence ──────────────────────────────────────────────
    let topic = format!("{disc}/binary_sensor/{dev_id}_presence/config");
    let payload = json!({
        "name": "Presence",
        "unique_id": format!("{dev_id}_presence"),
        "state_topic": state_topic,
        "value_template": "{{ 'ON' if value_json.presence else 'OFF' }}",
        "payload_on": "ON",
        "payload_off": "OFF",
        "device_class": "occupancy",
        "device": device,
    });
    payloads.push((topic, payload.to_string()));

    // ── binary_sensor: fall_detected ─────────────────────────────────────────
    let topic = format!("{disc}/binary_sensor/{dev_id}_fall/config");
    let payload = json!({
        "name": "Fall Detected",
        "unique_id": format!("{dev_id}_fall"),
        "state_topic": state_topic,
        "value_template": "{{ 'ON' if value_json.fall_detected else 'OFF' }}",
        "payload_on": "ON",
        "payload_off": "OFF",
        "device_class": "safety",
        "device": device,
    });
    payloads.push((topic, payload.to_string()));

    // ── sensor: breathing_rate ────────────────────────────────────────────────
    let topic = format!("{disc}/sensor/{dev_id}_breathing_rate/config");
    let payload = json!({
        "name": "Breathing Rate",
        "unique_id": format!("{dev_id}_breathing_rate"),
        "state_topic": state_topic,
        "value_template": "{{ value_json.breathing_rate_bpm | round(1) }}",
        "unit_of_measurement": "BPM",
        "state_class": "measurement",
        "icon": "mdi:lungs",
        "device": device,
    });
    payloads.push((topic, payload.to_string()));

    // ── sensor: heart_rate ────────────────────────────────────────────────────
    let topic = format!("{disc}/sensor/{dev_id}_heart_rate/config");
    let payload = json!({
        "name": "Heart Rate",
        "unique_id": format!("{dev_id}_heart_rate"),
        "state_topic": state_topic,
        "value_template": "{{ value_json.heart_rate_bpm | round(1) }}",
        "unit_of_measurement": "BPM",
        "state_class": "measurement",
        "device_class": "heart_rate",
        "device": device,
    });
    payloads.push((topic, payload.to_string()));

    // ── sensor: person_count ──────────────────────────────────────────────────
    let topic = format!("{disc}/sensor/{dev_id}_person_count/config");
    let payload = json!({
        "name": "Person Count",
        "unique_id": format!("{dev_id}_person_count"),
        "state_topic": state_topic,
        "value_template": "{{ value_json.person_count }}",
        "unit_of_measurement": "persons",
        "state_class": "measurement",
        "icon": "mdi:account-multiple",
        "device": device,
    });
    payloads.push((topic, payload.to_string()));

    // ── sensor: rssi ──────────────────────────────────────────────────────────
    let topic = format!("{disc}/sensor/{dev_id}_rssi/config");
    let payload = json!({
        "name": "WiFi Signal Strength",
        "unique_id": format!("{dev_id}_rssi"),
        "state_topic": state_topic,
        "value_template": "{{ value_json.rssi }}",
        "unit_of_measurement": "dBm",
        "state_class": "measurement",
        "device_class": "signal_strength",
        "entity_category": "diagnostic",
        "device": device,
    });
    payloads.push((topic, payload.to_string()));

    // ── sensor: motion_level ──────────────────────────────────────────────────
    let topic = format!("{disc}/sensor/{dev_id}_motion_level/config");
    let payload = json!({
        "name": "Motion Level",
        "unique_id": format!("{dev_id}_motion_level"),
        "state_topic": state_topic,
        "value_template": "{{ value_json.motion_level }}",
        "icon": "mdi:motion-sensor",
        "device": device,
    });
    payloads.push((topic, payload.to_string()));

    payloads
}

/// Serialize a `MqttSensingState` to the JSON state topic payload.
pub fn state_payload(s: &MqttSensingState) -> String {
    json!({
        "presence": s.presence,
        "fall_detected": s.fall_detected,
        "person_count": s.person_count,
        "breathing_rate_bpm": s.breathing_rate_bpm,
        "heart_rate_bpm": s.heart_rate_bpm,
        "rssi": s.rssi,
        "motion_level": s.motion_level,
    })
    .to_string()
}

/// Long-running MQTT task.
///
/// 1. Connects to the broker (retries on failure).
/// 2. Publishes HA discovery payloads once per connection.
/// 3. Receives state updates from the main loop via a `tokio::sync::watch` channel.
/// 4. Publishes the state to `<prefix>/<device_id>/state`.
pub async fn mqtt_task(
    cfg: MqttConfig,
    mut state_rx: tokio::sync::watch::Receiver<Option<MqttSensingState>>,
) {
    loop {
        info!(
            "MQTT: connecting to {}:{}",
            cfg.host, cfg.port
        );

        let (client, mut eventloop) = build_client(&cfg);

        // Drive the event loop in a background task so publishes don't block.
        let el_handle = tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(_event) => {
                        debug!("MQTT event");
                    }
                    Err(e) => {
                        warn!("MQTT event loop error: {e}");
                        break;
                    }
                }
            }
        });

        // Give the event loop a moment to establish the connection.
        tokio::time::sleep(Duration::from_millis(CONNECT_WAIT_MS)).await;

        // Publish HA discovery messages.
        if cfg.ha_discovery {
            info!("MQTT: publishing Home Assistant discovery payloads");
            for (topic, payload) in discovery_payloads(&cfg) {
                if let Err(e) = client
                    .publish(&topic, QoS::AtLeastOnce, true, payload.as_bytes())
                    .await
                {
                    error!("MQTT discovery publish error on {topic}: {e}");
                }
            }
            info!("MQTT: HA discovery complete — entities will appear in Home Assistant");
        }

        let state_topic = format!("{}/{}/state", cfg.topic_prefix, cfg.device_id);
        info!("MQTT: publishing state to {state_topic}");

        // Publish state updates as they arrive.
        loop {
            if el_handle.is_finished() {
                warn!("MQTT: event loop dropped, reconnecting in {RECONNECT_DELAY_SECS} s");
                tokio::time::sleep(Duration::from_secs(RECONNECT_DELAY_SECS)).await;
                break;
            }

            match state_rx.changed().await {
                Ok(()) => {
                    let maybe_state = state_rx.borrow().clone();
                    if let Some(ref s) = maybe_state {
                        let payload = state_payload(s);
                        if let Err(e) = client
                            .publish(&state_topic, QoS::AtLeastOnce, false, payload.as_bytes())
                            .await
                        {
                            warn!("MQTT publish error: {e}");
                        }
                    }
                }
                Err(_) => {
                    info!("MQTT: state channel closed, shutting down MQTT task");
                    return;
                }
            }
        }
    }
}

/// Bridge task: subscribes to the sensing broadcast channel, converts updates to
/// `MqttSensingState`, and sends them to the MQTT task via a watch channel.
///
/// This is the entry point spawned from `main()` when `--mqtt-host` is provided.
pub async fn mqtt_bridge_task(
    cfg: MqttConfig,
    mut broadcast_rx: tokio::sync::broadcast::Receiver<String>,
) {
    let (tx, rx) = tokio::sync::watch::channel::<Option<MqttSensingState>>(None);

    // Spawn the MQTT publisher.
    tokio::spawn(mqtt_task(cfg, rx));

    loop {
        match broadcast_rx.recv().await {
            Ok(json) => {
                if let Some(state) = parse_sensing_update(&json) {
                    let _ = tx.send(Some(state));
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                warn!("MQTT bridge lagged by {n} messages");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                info!("MQTT bridge: broadcast channel closed, exiting");
                return;
            }
        }
    }
}

/// Parse a `SensingUpdate` JSON string into an `MqttSensingState`.
fn parse_sensing_update(json: &str) -> Option<MqttSensingState> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;

    let presence = v["classification"]["presence"].as_bool().unwrap_or(false);
    let motion_level = v["classification"]["motion_level"]
        .as_str()
        .unwrap_or("absent")
        .to_string();

    let fall_detected = v["edge_vitals"]["fall_detected"].as_bool().unwrap_or(false);
    let person_count = v["estimated_persons"].as_u64().unwrap_or(0) as u32;

    let breathing_rate_bpm = v["vital_signs"]["breathing_rate_bpm"]
        .as_f64()
        .unwrap_or(0.0);
    let heart_rate_bpm = v["vital_signs"]["heart_rate_bpm"]
        .as_f64()
        .unwrap_or(0.0);

    // Use mean_rssi from features if available.
    let rssi = v["features"]["mean_rssi"].as_f64().unwrap_or(0.0) as i32;

    Some(MqttSensingState {
        presence,
        fall_detected,
        person_count,
        breathing_rate_bpm,
        heart_rate_bpm,
        rssi,
        motion_level,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sensing_update_presence() {
        let json = r#"{
            "classification": {"presence": true, "motion_level": "active"},
            "estimated_persons": 2,
            "vital_signs": {"breathing_rate_bpm": 14.5, "heart_rate_bpm": 72.0},
            "features": {"mean_rssi": -55.0}
        }"#;
        let s = parse_sensing_update(json).unwrap();
        assert!(s.presence);
        assert_eq!(s.motion_level, "active");
        assert_eq!(s.person_count, 2);
        assert!((s.breathing_rate_bpm - 14.5).abs() < 0.01);
        assert!((s.heart_rate_bpm - 72.0).abs() < 0.01);
        assert_eq!(s.rssi, -55);
        assert!(!s.fall_detected);
    }

    #[test]
    fn test_parse_sensing_update_absent() {
        let json = r#"{
            "classification": {"presence": false, "motion_level": "absent"},
            "features": {"mean_rssi": -70.0}
        }"#;
        let s = parse_sensing_update(json).unwrap();
        assert!(!s.presence);
        assert_eq!(s.motion_level, "absent");
        assert_eq!(s.person_count, 0);
        assert_eq!(s.rssi, -70);
    }

    #[test]
    fn test_parse_sensing_update_fall() {
        let json = r#"{
            "classification": {"presence": true, "motion_level": "active"},
            "edge_vitals": {"fall_detected": true},
            "features": {}
        }"#;
        let s = parse_sensing_update(json).unwrap();
        assert!(s.fall_detected);
    }

    #[test]
    fn test_parse_sensing_update_invalid_json() {
        assert!(parse_sensing_update("not json").is_none());
    }

    #[test]
    fn test_state_payload_serialization() {
        let s = MqttSensingState {
            presence: true,
            fall_detected: false,
            person_count: 1,
            breathing_rate_bpm: 15.0,
            heart_rate_bpm: 68.0,
            rssi: -50,
            motion_level: "present_still".to_string(),
        };
        let payload = state_payload(&s);
        let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(v["presence"], true);
        assert_eq!(v["person_count"], 1);
        assert_eq!(v["motion_level"], "present_still");
        assert_eq!(v["rssi"], -50);
    }

    #[test]
    fn test_discovery_payloads_count() {
        let cfg = MqttConfig::default();
        let payloads = discovery_payloads(&cfg);
        // 2 binary_sensors + 5 sensors = 7 total
        assert_eq!(payloads.len(), 7);
    }

    #[test]
    fn test_discovery_payload_topics() {
        let cfg = MqttConfig {
            device_id: "test_node".to_string(),
            discovery_prefix: "homeassistant".to_string(),
            ..MqttConfig::default()
        };
        let payloads = discovery_payloads(&cfg);
        let topics: Vec<&str> = payloads.iter().map(|(t, _)| t.as_str()).collect();
        assert!(topics.iter().any(|t| t.contains("binary_sensor/test_node_presence")));
        assert!(topics.iter().any(|t| t.contains("binary_sensor/test_node_fall")));
        assert!(topics.iter().any(|t| t.contains("sensor/test_node_breathing_rate")));
        assert!(topics.iter().any(|t| t.contains("sensor/test_node_heart_rate")));
        assert!(topics.iter().any(|t| t.contains("sensor/test_node_person_count")));
        assert!(topics.iter().any(|t| t.contains("sensor/test_node_rssi")));
        assert!(topics.iter().any(|t| t.contains("sensor/test_node_motion_level")));
    }

    #[test]
    fn test_discovery_payload_contains_device_info() {
        let cfg = MqttConfig {
            device_id: "mydev".to_string(),
            device_name: "My RuView Sensor".to_string(),
            ..MqttConfig::default()
        };
        let payloads = discovery_payloads(&cfg);
        for (_, payload) in &payloads {
            let v: serde_json::Value = serde_json::from_str(payload).unwrap();
            assert!(v["device"]["identifiers"][0].as_str().unwrap().contains("mydev"));
            assert_eq!(v["device"]["name"], "My RuView Sensor");
        }
    }
}
