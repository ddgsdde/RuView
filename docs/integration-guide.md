# RuView Integration Guide

> **按仓库现有代码逐文件解析接入方式，以及官方固件能力全览**
>
> File-by-file walkthrough of how each layer integrates, plus a complete
> reference for what the official ESP32 firmware can do.

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Integration Layer Map](#integration-layer-map)
3. [File-by-File Integration Walkthrough](#file-by-file-integration-walkthrough)
   - [Firmware (ESP32-S3 C layer)](#1-firmware-esp32-s3-c-layer)
   - [Python Hardware Abstraction Layer](#2-python-hardware-abstraction-layer-v1srchardware)
   - [Python Core Processing](#3-python-core-processing-v1srccore)
   - [Sensing Pipeline](#4-sensing-pipeline-v1srcsensing)
   - [WebSocket Bridge to UI](#5-websocket-bridge-v1srcsensing-ws_serverpy)
   - [Configuration](#6-configuration-v1srcconfig)
   - [REST API / Main App](#7-rest-api--main-app-v1srcmainpy)
   - [Web UI Services](#8-web-ui-services-uiservices)
4. [Official Firmware Capabilities](#official-firmware-capabilities)
5. [Data Flow End-to-End](#data-flow-end-to-end)
6. [Quick Integration Recipes](#quick-integration-recipes)

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│  ESP32-S3 Firmware (C / FreeRTOS / ESP-IDF)            │
│  ┌──────────┐  ┌───────────┐  ┌───────────┐           │
│  │ CSI      │  │ Edge DSP  │  │ WASM      │           │
│  │ Collector│→ │ (Core 1)  │→ │ Runtime   │           │
│  └──────────┘  └───────────┘  └───────────┘           │
│       │                │            │                   │
│  UDP stream       Vitals pkt    WASM events             │
│  (ADR-018)        (0xC5110002)  (0xC5110004)            │
└──────────┬──────────────┬────────────┘                  │
           │ UDP :5005    │ UDP :5005 (same socket)        │
           ▼              ▼                                │
┌──────────────────────────────────────────────┐          │
│  Python Sensing Pipeline  (v1/src/)          │          │
│  hardware/csi_extractor.py                   │          │
│     ↓ CSIData                                │          │
│  core/csi_processor.py  ←  core/phase_sanitizer.py     │
│     ↓                                        │          │
│  sensing/feature_extractor.py                │          │
│     ↓ RssiFeatures                           │          │
│  sensing/classifier.py  → SensingResult      │          │
│     ↓                                        │          │
│  sensing/ws_server.py  → ws://localhost:8765 │          │
└──────────────────────────────────────────────┘          │
           │ WebSocket JSON                                │
           ▼                                              │
┌──────────────────────────┐                              │
│  Web UI  (ui/)           │                              │
│  services/websocket.service.js                          │
│  services/sensing.service.js                            │
│  components/SensingTab.js                               │
│  components/signal-viz.js                               │
└──────────────────────────┘                              │
```

---

## Integration Layer Map

| Layer | Location | Protocol | Data Format |
|-------|----------|----------|-------------|
| ESP32 → Server | `firmware/esp32-csi-node/` | UDP | ADR-018 binary frame (magic `0xC5110001`) |
| ESP32 vitals → Server | same UDP socket | UDP | Vitals packet (magic `0xC5110002`, 32 bytes) |
| ESP32 fused vitals | same UDP socket | UDP | Fused vitals (magic `0xC5110004`, 48 bytes) |
| Server → Browser | `v1/src/sensing/ws_server.py` | WebSocket | JSON `sensing_update` |
| Browser API calls | `v1/src/main.py` (FastAPI) | HTTP/REST | JSON |
| Provisioning | `firmware/esp32-csi-node/provision.py` | USB/Serial + esptool | NVS binary |
| OTA update | ESP32 HTTP server (port 8032) | HTTP POST | Raw `.bin` firmware |
| WASM upload | ESP32 HTTP server (same port) | HTTP POST | `.wasm` or `.rvf` binary |
| Swarm / Seed | `firmware/.../swarm_bridge.c` | HTTP POST | JSON vectors |

---

## File-by-File Integration Walkthrough

### 1. Firmware (ESP32-S3 C layer)

#### `firmware/esp32-csi-node/main/main.c` — Entry point

The boot sequence determines which integrations are active:

```
nvs_flash_init()
  └─ nvs_config_load()          ← loads all runtime settings from NVS
wifi_init_sta()                 ← connects to your WiFi AP
stream_sender_init_with(ip, port) ← opens UDP socket to aggregator
csi_collector_init()            ← arms ESP-IDF CSI callback
edge_processing_init()          ← starts DSP pipeline on Core 1
ota_update_init_ex()            ← HTTP OTA server on :8032
wasm_runtime_init()             ← WASM3 interpreter + periodic timer
mmwave_sensor_init()            ← auto-detects 60 GHz / 24 GHz sensor
swarm_bridge_init()             ← HTTP client to Cognitum Seed (optional)
power_mgmt_init()               ← duty-cycle power management
display_task_start()            ← AMOLED UI (if CONFIG_DISPLAY_ENABLE)
```

**How to integrate:** flash the firmware, then run `provision.py` to write WiFi
credentials and the aggregator IP into NVS. The device connects and immediately
starts streaming.

---

#### `firmware/esp32-csi-node/provision.py` — Provisioning tool

Writes configuration into the ESP32's NVS partition without recompiling.

```bash
# Minimal: WiFi + aggregator IP
python provision.py \
  --port COM7 \
  --ssid "YourWiFi" \
  --password "secret" \
  --target-ip 192.168.1.20

# Full mesh node (TDM slot 0 of 3):
python provision.py \
  --port COM7 --ssid "YourWiFi" --password "secret" \
  --target-ip 192.168.1.20 --target-port 5005 \
  --node-id 1 \
  --tdm-slot 0 --tdm-total 3 \
  --edge-tier 2 \
  --channel 6 \
  --filter-mac AA:BB:CC:DD:EE:FF   # only CSI from this AP

# Swarm / Cognitum Seed integration:
python provision.py \
  --port COM7 --ssid "YourWiFi" --password "secret" \
  --target-ip 192.168.1.20 \
  --seed-url http://10.1.10.236 \
  --seed-token "Bearer-token-from-pairing" \
  --zone "lobby"
```

All keys written to NVS namespace `csi_cfg`.

---

#### `firmware/esp32-csi-node/main/csi_collector.c` — CSI capture + serialization

Registers the ESP-IDF WiFi CSI callback and serializes every frame into the
**ADR-018 binary format** for UDP transmission.

**Frame layout** (20-byte header + I/Q payload):

| Offset | Size | Field |
|--------|------|-------|
| 0–3 | 4 | Magic `0xC5110001` (LE) |
| 4 | 1 | Node ID |
| 5 | 1 | Number of antennas |
| 6–7 | 2 | Number of subcarriers (LE u16) |
| 8–11 | 4 | Frequency MHz (LE u32) |
| 12–15 | 4 | Sequence number (LE u32) |
| 16 | 1 | RSSI (i8) |
| 17 | 1 | Noise floor (i8) |
| 18–19 | 2 | Reserved |
| 20+ | N×2 | I/Q pairs (signed i8 bytes) |

The callback fires in promiscuous mode (~20–50 Hz after rate limiting). It also
enqueues each frame into the lock-free SPSC ring for the edge DSP pipeline.

**ADR-029 channel hopping:** after `csi_collector_set_hop_table(channels, n, dwell_ms)`,
the device cycles channels every `dwell_ms` milliseconds, enabling multi-band sensing
across channels 1/6/11 (2.4 GHz) and 36/40/44/48 (5 GHz).

---

#### `firmware/esp32-csi-node/main/stream_sender.c` — UDP transmission

Opens a single UDP socket and calls `sendto()` for every serialized frame.

Key detail: implements **ENOMEM backoff** — if `lwIP` runs out of packet buffers
it suppresses sends for 100 ms to prevent a crash loop. Integrate by listening on
the configured UDP port (default **5005**).

---

#### `firmware/esp32-csi-node/main/edge_processing.c` — On-device DSP pipeline

Runs entirely on Core 1 (FreeRTOS task). Pipeline per frame:

1. Extract phase from I/Q pairs (per subcarrier)
2. Phase unwrapping (continuous phase)
3. Welford running variance per subcarrier
4. Top-K subcarrier selection by variance
5. Biquad IIR bandpass → **breathing** (0.1–0.5 Hz)
6. Biquad IIR bandpass → **heart rate** (0.8–2.0 Hz)
7. Zero-crossing BPM estimation
8. **Presence detection** (adaptive threshold, auto-calibrates over 60 s)
9. **Fall detection** (phase acceleration > `fall_thresh` rad/s², debounced 5 s)
10. **Multi-person vitals** via subcarrier group clustering (up to 4 persons)
11. Delta compression (XOR + RLE) for bandwidth
12. Broadcasts **vitals packet** (magic `0xC5110002`, 32 bytes) over UDP

**Vitals packet wire format** (32 bytes, packed):

```c
uint32_t magic;          // 0xC5110002
uint8_t  node_id;
uint8_t  flags;          // Bit0=presence, Bit1=fall, Bit2=motion
uint16_t breathing_rate; // BPM × 100
uint32_t heartrate;      // BPM × 10000
int8_t   rssi;
uint8_t  n_persons;
uint8_t  reserved[2];
float    motion_energy;
float    presence_score;
uint32_t timestamp_ms;
uint32_t reserved2;
```

**Fused vitals packet** (48 bytes, when mmWave sensor is present, magic `0xC5110004`):
extends the 32-byte packet with `mmwave_hr_bpm`, `mmwave_br_bpm`, `mmwave_distance`,
`mmwave_targets`, and `mmwave_confidence`.

---

#### `firmware/esp32-csi-node/main/nvs_config.c` — Runtime configuration

All NVS keys and their mapping to `nvs_config_t`:

| NVS Key | Type | Description |
|---------|------|-------------|
| `ssid` | string | WiFi SSID |
| `password` | string | WiFi password |
| `target_ip` | string | Aggregator IP |
| `target_port` | u16 | UDP port (default 5005) |
| `node_id` | u8 | Node identifier |
| `hop_count` | u8 | Number of channels to cycle |
| `chan_list` | blob | Channel list (up to 8 bytes) |
| `dwell_ms` | u32 | Dwell per channel in ms |
| `tdm_slot` | u8 | TDM slot index |
| `tdm_nodes` | u8 | Total TDM nodes |
| `edge_tier` | u8 | 0=raw, 1=stats, 2=vitals |
| `pres_thresh` | u16 | Presence threshold ×1000 |
| `fall_thresh` | u16 | Fall threshold ×1000 (rad/s²) |
| `vital_win` | u16 | Phase history window (32–256) |
| `vital_int` | u16 | Vitals packet interval ms |
| `subk_count` | u8 | Top-K subcarrier count |
| `power_duty` | u8 | Duty cycle % (10–100) |
| `wasm_max` | u8 | WASM module slots (1–8) |
| `wasm_verify` | u8 | Signature verification flag |
| `wasm_pubkey` | blob | Ed25519 public key (32 bytes) |
| `csi_channel` | u8 | CSI channel override |
| `filter_mac` | blob | MAC filter (6 bytes) |
| `seed_url` | string | Cognitum Seed base URL |
| `seed_token` | string | Bearer token |
| `zone_name` | string | Zone label |
| `swarm_hb` | u16 | Heartbeat interval (s) |
| `swarm_ingest` | u16 | Vector ingest interval (s) |

---

#### `firmware/esp32-csi-node/main/ota_update.c` — HTTP OTA server

Starts a lightweight HTTP server on port **8032**. Endpoints:

- `POST /ota` — Upload a new firmware `.bin`. The device validates, writes to
  the inactive OTA partition, and reboots.

```bash
curl -X POST http://<ESP32_IP>:8032/ota \
     --data-binary @esp32-csi-node.bin
```

---

#### `firmware/esp32-csi-node/main/wasm_runtime.c` — Programmable sensing

Manages a WASM3 interpreter. Up to 4 modules (configurable via NVS `wasm_max`)
run concurrently. Each module receives:

- `on_init()` — called once at load
- `on_frame(n_subcarriers)` — called at ~20 Hz per CSI frame
- `on_timer()` — called every `WASM_TIMER_INTERVAL_MS` (default 1 s)

**Host API exposed to WASM modules:**

```
csi_get_phase(subcarrier)   → f32
csi_get_amplitude(sc)       → f32
csi_get_variance(sc)        → f32
csi_get_bpm_breathing()     → f32
csi_get_bpm_heartrate()     → f32
csi_get_presence()          → i32
csi_get_motion_energy()     → f32
csi_get_n_persons()         → i32
csi_get_timestamp()         → i32
csi_emit_event(event_type, value) → void  // event_type: u8 application-defined code; value: f32 payload
csi_log(ptr, len)           → void
csi_get_phase_history(buf_ptr, max_len) → i32
```

Modules are uploaded via `POST /wasm` on the OTA HTTP server (also port 8032).
WASM output packets use magic `0xC5110004` (same as fused vitals — context
distinguishes them by `module_id` field).

---

#### `firmware/esp32-csi-node/main/mmwave_sensor.c` — 60/24 GHz sensor fusion

Auto-detects on UART at boot. Supported sensors:

| Sensor | Freq | Capabilities |
|--------|------|-------------|
| Seeed MR60BHA2 | 60 GHz | Heart rate, breathing, presence, fall |
| HLK-LD2410 | 24 GHz | Presence, distance to target |

When detected, CSI vitals and mmWave readings are fused via Kalman filter and
broadcast as **fused vitals packets** (48 bytes, magic `0xC5110004`).

---

#### `firmware/esp32-csi-node/main/swarm_bridge.c` — Cognitum Seed client

Optional. When `seed_url` is set in NVS:

1. **Registration POST** — sent once at boot to `/api/v1/store/ingest`
2. **Heartbeat POST** — every `swarm_heartbeat_sec` seconds (default 30 s)
3. **Happiness vector ingest** — every `swarm_ingest_sec` seconds (default 5 s),
   only when presence is detected

JSON format follows the Cognitum Seed vector ingest spec:
```json
{"vectors": [[<id>, [f32; 8]]]}
```

---

### 2. Python Hardware Abstraction Layer (`v1/src/hardware/`)

#### `v1/src/hardware/csi_extractor.py` — CSI data extraction

The primary integration point between firmware and Python. Two parsers:

**`ESP32CSIParser`** — legacy text format:
```
CSI_DATA:<ts_ms>,<n_ant>,<n_sc>,<freq_mhz>,<bw_mhz>,<snr>,<amp...>,<phase...>
```

**`ESP32BinaryParser`** — ADR-018 binary format (recommended):
Matches the exact frame layout from `csi_collector.c`. Returns `CSIData` with
amplitude/phase arrays shaped `(n_antennas, n_subcarriers)`.

**`CSIExtractor`** — connection manager:

```python
from v1.src.hardware.csi_extractor import CSIExtractor

extractor = CSIExtractor({
    "hardware_type": "esp32",
    "parser_format": "binary",          # use ADR-018 format
    "aggregator_host": "0.0.0.0",
    "aggregator_port": 5005,
    "sampling_rate": 20,
    "buffer_size": 1000,
    "timeout": 5.0,
})
await extractor.connect()
csi = await extractor.extract_csi()     # CSIData object
```

For streaming:
```python
async def handle(csi: CSIData):
    print(csi.amplitude.shape, csi.phase.shape)

await extractor.start_streaming(handle)
```

---

#### `v1/src/hardware/router_interface.py` — SSH router interface

SSH-based integration for routers that expose CSI via command line (e.g.,
modified OpenWrt with Atheros CSI Tool).

> **Note:** `get_csi_data()` raises `RouterConnectionError` until a
> hardware-specific parser for your router's CSI format is implemented.
> Currently only ESP32 (binary + text) parsers are production-ready.
> Use this class for status queries and channel configuration only.

Configure with:

```python
from v1.src.hardware.router_interface import RouterInterface

iface = RouterInterface({
    "host": "192.168.1.1",
    "port": 22,
    "username": "root",
    "password": "your-password",
    "command_timeout": 30,
})
await iface.connect()
await iface.configure_csi_monitoring({"channel": 6})
status = await iface.get_router_status()
```

---

### 3. Python Core Processing (`v1/src/core/`)

#### `v1/src/core/phase_sanitizer.py`

Removes phase noise, applies linear phase correction, and unwraps continuous
phase from raw CSI data. Input: raw `(n_antennas, n_subcarriers)` phase array.
Output: sanitized phase ready for feature extraction.

#### `v1/src/core/csi_processor.py`

Combines multiple `CSIData` frames into a feature-ready tensor. Applies windowing,
normalization, and subcarrier selection. Used by the model inference pipeline.

#### `v1/src/core/router_interface.py`

Alternative router interface using `asyncssh` — same API as `hardware/router_interface.py`
but designed for batch command sequences needed by some router CSI tools.

---

### 4. Sensing Pipeline (`v1/src/sensing/`)

#### `v1/src/sensing/backend.py` — Backend protocol and `CommodityBackend`

Defines the `SensingBackend` protocol that all data sources implement:

```python
class SensingBackend(Protocol):
    def get_features(self) -> RssiFeatures: ...
    def get_capabilities(self) -> Set[Capability]: ...
```

`CommodityBackend` wires together collector → extractor → classifier for
RSSI-only sensing (no ESP32 required). Capabilities: `PRESENCE`, `MOTION`.

```python
from v1.src.sensing.backend import CommodityBackend
from v1.src.sensing.rssi_collector import create_collector

collector = create_collector(preferred="auto")   # auto-detects platform
backend = CommodityBackend(collector)
backend.start()
result = backend.get_result()                    # SensingResult
print(result.motion_level, result.confidence)
```

#### `v1/src/sensing/rssi_collector.py` — WiFi RSSI collection

Auto-detects platform:
- **`LinuxWifiCollector`** — reads `/proc/net/wireless`
- **`WindowsWifiCollector`** — calls `netsh wlan show interfaces`
- **`MacosWifiCollector`** — calls CoreWLAN via Swift helper
- **`SimulatedCollector`** — generates synthetic RSSI for testing

Use `create_collector(preferred="auto")` (ADR-049 factory) for automatic
platform selection with graceful fallback.

#### `v1/src/sensing/feature_extractor.py` — Feature extraction

Takes a window of `WifiSample` objects and produces `RssiFeatures`:

```python
RssiFeatures(
    mean,                   # mean RSSI dBm
    variance,               # variance
    std,                    # standard deviation
    motion_band_power,      # 0.5–2 Hz power (motion)
    breathing_band_power,   # 0.1–0.5 Hz power (breathing)
    dominant_freq_hz,       # dominant frequency
    n_change_points,        # CUSUM change point count
    total_spectral_power,   # total FFT power
    range, iqr,             # range & interquartile range
    skewness, kurtosis,     # distribution shape
)
```

#### `v1/src/sensing/classifier.py` — Presence/motion classification

Rule-based + threshold classifier. Outputs `SensingResult`:

```python
SensingResult(
    motion_level: MotionLevel,   # NONE / MICRO / SUBTLE / MODERATE / ACTIVE
    presence_detected: bool,
    confidence: float,           # 0.0 – 1.0
)
```

---

### 5. WebSocket Bridge (`v1/src/sensing/ws_server.py`)

The main bridge from sensing pipeline to browser UI.

**Start:**
```bash
python -m v1.src.sensing.ws_server
# or:  python v1/src/sensing/ws_server.py
```

**Auto-detects source in order:**
1. ESP32 CSI UDP on port 5005 (probes for 2 s)
2. Platform WiFi (Linux / Windows / macOS)
3. Simulated fallback

**WebSocket endpoint:** `ws://localhost:8765`

**JSON message format** (`sensing_update`):
```json
{
  "type": "sensing_update",
  "timestamp": 1722000000.0,
  "source": "esp32",
  "nodes": [{
    "node_id": 1,
    "rssi_dbm": -52,
    "position": [2.0, 0.0, 1.5],
    "amplitude": [3.2, 5.1, ...],
    "subcarrier_count": 56,
    "mean_amplitude": 8.3,
    "freq_mhz": 2437,
    "sequence": 12345,
    "source_addr": "192.168.1.101:52000"
  }],
  "features": {
    "mean_rssi": -52.3,
    "variance": 4.1,
    "motion_band_power": 0.012,
    "breathing_band_power": 0.004,
    "dominant_freq_hz": 0.28
  },
  "classification": {
    "motion_level": "SUBTLE",
    "presence": true,
    "confidence": 0.81
  },
  "signal_field": {
    "grid_size": [20, 1, 20],
    "values": [0.12, 0.34, ...]
  }
}
```

Connect from JavaScript:
```javascript
const ws = new WebSocket('ws://localhost:8765');
ws.onmessage = (e) => {
  const data = JSON.parse(e.data);
  // data.classification.presence, data.features.breathing_band_power, ...
};
```

---

### 6. Configuration (`v1/src/config/`)

#### `v1/src/config/settings.py`

Central settings object loaded from environment variables (or `.env` file):

| Variable | Default | Description |
|----------|---------|-------------|
| `HOST` | `0.0.0.0` | API server bind address |
| `PORT` | `8000` | API server port |
| `ENVIRONMENT` | `development` | `development` / `production` |
| `DATABASE_URL` | SQLite path | Database connection string |
| `HARDWARE_TYPE` | `esp32` | `esp32` / `router` / `simulated` |
| `AGGREGATOR_HOST` | `0.0.0.0` | UDP listen address |
| `AGGREGATOR_PORT` | `5005` | UDP listen port |
| `SAMPLING_RATE` | `20` | CSI samples per second |

Copy `example.env` to `.env` and set your values before starting the server.

---

### 7. REST API / Main App (`v1/src/main.py`)

Starts a FastAPI application via `uvicorn`. By default listens on `:8000`.

```bash
python -m v1.src.main
```

Key endpoints (see `plans/phase1-specification/api-spec.md` for full spec):

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Health check |
| GET | `/api/v1/sensing/status` | Current sensing state |
| GET | `/api/v1/sensing/features` | Latest feature vector |
| GET | `/api/v1/pose/current` | Current pose estimate |
| POST | `/api/v1/hardware/connect` | Connect hardware |
| GET | `/api/v1/vitals` | Latest vitals packet |

---

### 8. Web UI Services (`ui/services/`)

#### `ui/services/websocket.service.js`

Manages the WebSocket connection to `ws://localhost:8765`. Reconnects
automatically with exponential backoff. Emits events for each `sensing_update`.

#### `ui/services/sensing.service.js`

Subscribes to WebSocket updates and provides reactive state for UI components.
Tracks presence, motion level, breathing rate, signal field data.

#### `ui/services/api.service.js`

REST client pointing at `http://localhost:8000`. Configurable via
`ui/config/api.config.js`.

---

## Official Firmware Capabilities

The pre-built `esp32-csi-node.bin` (8 MB flash) and `esp32-csi-node-4mb.bin`
(4 MB flash) provide the following capabilities out of the box:

### Core: WiFi CSI Capture

- **Technology:** ESP-IDF WiFi CSI API with promiscuous mode
- **Output:** 20–50 Hz stream of raw I/Q subcarrier data per frame
- **Subcarriers:** 56 (HT20) / 114 (HT40) / 242 (HT80)
- **Protocol:** UDP unicast, ADR-018 binary format
- **Channel:** auto-detected from connected AP, or NVS override

### Edge Intelligence (Tier 2 — default)

Runs on Core 1 with no network dependency:

| Capability | Output field | Update rate |
|------------|-------------|-------------|
| **Presence detection** | `flags.Bit0`, `presence_score` | Every frame |
| **Motion detection** | `flags.Bit2`, `motion_energy` | Every frame |
| **Breathing rate** | `breathing_rate` (BPM × 100) | Every `vital_int` ms |
| **Heart rate** | `heartrate` (BPM × 10000) | Every `vital_int` ms |
| **Fall detection** | `flags.Bit1` | Per event (debounced 5 s) |
| **Person count** | `n_persons` | Per vitals packet |
| **Multi-person vitals** | via multi-person subcarrier clustering | Up to 4 persons |

### Multi-Band Sensing (ADR-029)

- Channel hopping across up to 8 configurable channels
- Programmable dwell time per channel (min 10 ms)
- Enables cross-band sensing diversity

### mmWave Sensor Fusion (ADR-063)

When Seeed MR60BHA2 or HLK-LD2410 is wired to the UART pins:

| Sensor | Additional capabilities |
|--------|------------------------|
| MR60BHA2 (60 GHz) | Independent HR + BR + fall detection + Kalman fusion with CSI |
| LD2410 (24 GHz) | Presence + distance to target |

Fused results broadcast as 48-byte packets (magic `0xC5110004`).

### Programmable Sensing (ADR-040, WASM)

Hot-load custom sensing algorithms without reflashing:

- Upload `.wasm` or `.rvf` modules via HTTP POST `/wasm`
- Up to 4 concurrent modules (configurable)
- Each module runs `on_init()`, `on_frame(n_sc)`, `on_timer()`
- Full access to phase, amplitude, variance, vitals via host API
- 10 ms per-frame budget (configurable), enforced at runtime
- Optional Ed25519 signature verification for uploaded modules

### Over-the-Air Firmware Update (OTA)

- HTTP server on port **8032**
- `POST /ota` with new firmware binary
- Automatic rollback to previous firmware if boot fails

### Swarm / Cognitum Seed Integration (ADR-066)

- Registers with a Cognitum Seed coordinator at boot
- Sends periodic heartbeats with happiness vectors
- Ingests vectors when presence is detected
- Bearer token authentication
- Configurable heartbeat (default 30 s) and ingest (default 5 s) intervals

### Power Management

- Configurable duty cycle (10–100%, NVS `power_duty`)
- CPU frequency scaling
- WiFi modem sleep between sensing windows

### AMOLED Display (ADR-045, optional)

When `CONFIG_DISPLAY_ENABLE` is set and a display is wired:
- Real-time presence / motion indicator
- Node ID and network status
- Live breathing rate BPM

### QEMU / Mock Mode (ADR-061)

Set `CONFIG_CSI_MOCK_ENABLED=y` at build time:
- Runs without real WiFi hardware (for CI / testing)
- Generates synthetic CSI frames with configurable scenarios
- Skips WiFi init and UDP networking

---

## Data Flow End-to-End

```
Real Environment
      │  (radio waves)
      ▼
ESP32-S3  (promiscuous mode)
   CSI callback fires @ 100-500 Hz
      │  rate-limited to 20-50 Hz
      │  serialized → ADR-018 binary frame
      ▼
UDP socket :5005  ─────────────────────────────────────────────┐
      │                                                          │
      ▼  (Esp32UdpCollector)                                     │
Python ws_server.py                                             │
   parse ADR-018 → CSIData                                      │
   feature extraction (FFT, variance, zero-crossing)            │
   presence/motion classification                               │
      │                                                          │
      ▼  JSON @ 2 Hz                                            │
WebSocket ws://localhost:8765                                    │
      │                                                          │
      ▼                                                          │
Browser UI                                                      │
   SensingTab: presence badge, motion meter                     │
   signal-viz: Gaussian splat field                             │
   HardwareTab: node list, subcarrier plot                      │
                                                                │
Meanwhile, on the ESP32 (Core 1):                               │
   edge_processing_task()                                       │
      phase extraction → Welford variance → IIR bandpass       │
      BPM estimation → presence score → fall detection         │
   → vitals_pkt (32 bytes, 0xC5110002)  ─────────────────────►│
      same UDP socket :5005                                     │
      parsed by ws_server if vitals magic detected              │
```

---

## Quick Integration Recipes

### A. Zero-Hardware (Simulated RSSI)

```bash
# No ESP32 needed
python -m v1.src.sensing.ws_server
# Opens ws://localhost:8765 with simulated data
```

### B. ESP32 → Python → Browser

```bash
# 1. Flash and provision ESP32
python firmware/esp32-csi-node/provision.py \
  --port COM7 --ssid "MyWifi" --password "pass" \
  --target-ip 192.168.1.10

# 2. Start sensing server (on host at 192.168.1.10)
python -m v1.src.sensing.ws_server
# Auto-detects ESP32 on UDP :5005

# 3. Open browser
open ui/index.html   # or serve via nginx / python -m http.server
```

### C. Direct CSI consumption in Python

```python
import asyncio
from v1.src.hardware.csi_extractor import CSIExtractor

async def main():
    ext = CSIExtractor({
        "hardware_type": "esp32",
        "parser_format": "binary",
        "aggregator_host": "0.0.0.0",
        "aggregator_port": 5005,
        "sampling_rate": 20,
        "buffer_size": 200,
        "timeout": 5.0,
    })
    await ext.connect()
    
    async def on_frame(csi):
        # csi.amplitude: shape (n_antennas, n_subcarriers)
        # csi.phase:     shape (n_antennas, n_subcarriers)
        # csi.metadata['rssi_dbm'], csi.metadata['node_id']
        print(f"amp mean: {csi.amplitude.mean():.2f}")
    
    await ext.start_streaming(on_frame)

asyncio.run(main())
```

### D. WASM custom algorithm

```bash
# Build a Rust WASM module targeting the CSI host API
cargo build --target wasm32-unknown-unknown --release

# Upload to running ESP32
curl -X POST http://<ESP32_IP>:8032/wasm \
     --data-binary @target/wasm32-unknown-unknown/release/my_algo.wasm
```

### E. Multi-node mesh (3 nodes, TDM)

```bash
# Node 0 (TDM slot 0 of 3, channels 1+6+11)
python provision.py --port COM7 --ssid wifi --password pass \
  --target-ip 192.168.1.10 --node-id 1 \
  --tdm-slot 0 --tdm-total 3

# Node 1 (TDM slot 1 of 3)
python provision.py --port COM8 --ssid wifi --password pass \
  --target-ip 192.168.1.10 --node-id 2 \
  --tdm-slot 1 --tdm-total 3

# Node 2 (TDM slot 2 of 3)
python provision.py --port COM9 --ssid wifi --password pass \
  --target-ip 192.168.1.10 --node-id 3 \
  --tdm-slot 2 --tdm-total 3

# All three stream to the same UDP :5005 — the server handles multiple nodes
python -m v1.src.sensing.ws_server
```

### F. Swarm / Cognitum Seed integration

```bash
python provision.py --port COM7 --ssid wifi --password pass \
  --target-ip 192.168.1.10 \
  --seed-url http://10.1.10.236 \
  --seed-token "Bearer abc123" \
  --zone "meeting-room-a"
# Node self-registers and starts ingesting vectors on presence detection
```
