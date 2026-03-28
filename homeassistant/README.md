# RuView — Home Assistant Integration

This directory contains two ways to integrate **RuView WiFi Sensor** data into [Home Assistant](https://www.home-assistant.io/):

| Method | Best for |
|--------|----------|
| **Custom Component** (`custom_components/ruview/`) | REST API polling — works without any extra config on the sensing server |
| **MQTT + Auto-Discovery** | Push-based, real-time — requires `--mqtt-host` flag on the sensing server |

---

## Method 1 — Custom Component (REST Polling)

### Requirements

- RuView sensing server running (default `http://localhost:8080`)
- Home Assistant 2024.1+

### Installation

1. Copy the `custom_components/ruview/` folder into your Home Assistant `config/custom_components/` directory:

   ```
   config/
   └── custom_components/
       └── ruview/          ← copy this entire folder
           ├── __init__.py
           ├── binary_sensor.py
           ├── config_flow.py
           ├── const.py
           ├── manifest.json
           ├── sensor.py
           ├── strings.json
           └── translations/
               └── en.json
   ```

2. Restart Home Assistant.

3. Go to **Settings → Devices & Services → Add Integration** and search for **RuView WiFi Sensor**.

4. Enter the sensing server host/IP and port (default `8080`).

### Entities created

| Entity | Type | Description |
|--------|------|-------------|
| `binary_sensor.ruview_presence` | Binary sensor | Occupancy detection (ON/OFF) |
| `binary_sensor.ruview_fall_detected` | Binary sensor | Fall alert (ON/OFF) |
| `sensor.ruview_breathing_rate` | Sensor | Breathing rate (BPM) |
| `sensor.ruview_heart_rate` | Sensor | Heart rate (BPM) |
| `sensor.ruview_person_count` | Sensor | Estimated person count |
| `sensor.ruview_motion_level` | Sensor | absent / present_still / active |
| `sensor.ruview_wifi_signal` | Sensor | WiFi signal strength (dBm) |

---

## Method 2 — MQTT Auto-Discovery (Recommended)

This method uses the MQTT publisher built into the sensing server.
Home Assistant discovers all entities automatically — no YAML needed.

### Requirements

- MQTT broker (e.g. [Mosquitto add-on](https://github.com/home-assistant/addons/tree/master/mosquitto) in HA)
- Home Assistant MQTT integration enabled

### Start the sensing server with MQTT

```bash
# Basic (no auth)
cargo run -p wifi-densepose-sensing-server -- \
  --source esp32 \
  --mqtt-host 192.168.1.10

# With authentication
cargo run -p wifi-densepose-sensing-server -- \
  --source esp32 \
  --mqtt-host 192.168.1.10 \
  --mqtt-username mqtt_user \
  --mqtt-password mqtt_pass

# Custom device name shown in HA
cargo run -p wifi-densepose-sensing-server -- \
  --source esp32 \
  --mqtt-host homeassistant.local \
  --mqtt-device-name "Living Room WiFi Sensor" \
  --mqtt-device-id  living_room_01
```

### Environment variables (alternative to CLI flags)

```bash
export MQTT_HOST=192.168.1.10
export MQTT_PORT=1883
export MQTT_USERNAME=user
export MQTT_PASSWORD=secret
export MQTT_DEVICE_ID=ruview_01
export MQTT_DEVICE_NAME="RuView WiFi Sensor"
cargo run -p wifi-densepose-sensing-server -- --source esp32
```

### MQTT topics

| Topic | Description |
|-------|-------------|
| `ruview/<device_id>/state` | JSON state published at every sensing tick |
| `homeassistant/binary_sensor/<device_id>_presence/config` | HA discovery (retained) |
| `homeassistant/binary_sensor/<device_id>_fall/config` | HA discovery (retained) |
| `homeassistant/sensor/<device_id>_breathing_rate/config` | HA discovery (retained) |
| `homeassistant/sensor/<device_id>_heart_rate/config` | HA discovery (retained) |
| `homeassistant/sensor/<device_id>_person_count/config` | HA discovery (retained) |
| `homeassistant/sensor/<device_id>_rssi/config` | HA discovery (retained) |
| `homeassistant/sensor/<device_id>_motion_level/config` | HA discovery (retained) |

### State payload example

```json
{
  "presence": true,
  "fall_detected": false,
  "person_count": 1,
  "breathing_rate_bpm": 15.2,
  "heart_rate_bpm": 68.4,
  "rssi": -52,
  "motion_level": "present_still"
}
```

---

## Example Automations

### Alert when a fall is detected

```yaml
automation:
  alias: "RuView Fall Alert"
  trigger:
    - platform: state
      entity_id: binary_sensor.ruview_fall_detected
      to: "on"
  action:
    - service: notify.mobile_app
      data:
        title: "Fall Detected!"
        message: "RuView detected a fall in the monitored area."
```

### Turn on lights when someone enters

```yaml
automation:
  alias: "RuView Presence Light"
  trigger:
    - platform: state
      entity_id: binary_sensor.ruview_presence
      to: "on"
  action:
    - service: light.turn_on
      target:
        entity_id: light.living_room
```

---

## Troubleshooting

| Problem | Solution |
|---------|----------|
| Can't connect to sensing server | Make sure the server is running: `cargo run -p wifi-densepose-sensing-server` and is accessible on the configured host/port |
| MQTT entities not appearing | Check MQTT broker is reachable; check sensing-server logs for "MQTT: HA discovery complete" |
| Stale values | Increase `--tick-ms` for faster updates or decrease `scan_interval` in the custom component config |
| No ESP32 data | Confirm the ESP32 is flashed and provisioned; check UDP port 5005 is open |
