# EEG Data WebSocket Hybrid Binary Protocol (Optimized)

## 1. Overview

This document defines a stateful, hybrid binary/JSON protocol for streaming EEG data. The design minimizes bandwidth and processing overhead by sending static metadata only when it changes, while streaming dynamic data in a highly efficient format.

## 2. Protocol Design

The protocol consists of two distinct message types sent over the same WebSocket connection. The frontend is responsible for maintaining the state (the `SensorMeta`) for each data stream (`topic`).

---

### Message Type 1: `meta_update`

This is a **JSON-only** message used to send or update the sensor metadata for a given topic.

**When to Send:**
*   Immediately after a client connects and subscribes to a topic.
*   Any time the `SensorMeta` for that stream changes (e.g., gain is adjusted).

**Structure:**
```json
{
  "message_type": "meta_update",
  "topic": "string",
  "meta": {
    "sensor_id": "number",
    "meta_rev": "number",
    "source_type": "string",
    "v_ref": "number",
    "adc_bits": "number",
    "gain": "number",
    "sample_rate": "number",
    "offset_code": "number",
    "is_twos_complement": "boolean",
    "channel_names": ["string"]
  }
}
```

---

### Message Type 2: `data_packet`

This is a **hybrid binary** message for sending high-frequency sample data. It assumes the frontend has already received and stored the metadata from a `meta_update` message for the corresponding `topic`.

**Structure:**
A single binary frame (`ArrayBuffer`) composed of three parts:

1.  **Header Length (4 bytes):** A 32-bit unsigned little-endian integer specifying the length of the JSON metadata header.
2.  **JSON Metadata Header (variable length):** A UTF-8 encoded JSON string with a minimal set of fields.
3.  **Sample Payload (variable length):** A raw binary blob of the EEG sample data (e.g., `Float32Array`).

**Visual Layout:**
```
[ 4 bytes ] [ N bytes        ] [ M bytes                ]
+-----------+------------------+--------------------------+
|JSON_LEN   | JSON_HEADER      | RAW_SAMPLES              |
| (uint32)  | (utf-8 string)   | (e.g., Float32Array)     |
+-----------+------------------+--------------------------+
```

**JSON Header Structure (`data_packet`):**
```json
{
  "message_type": "data_packet",
  "topic": "string",
  "ts_ns": "number",
  "batch_size": "number",
  "num_channels": "number",
  "packet_type": "string"
}
```
- **`packet_type`**: A string indicating the data type of the raw samples (e.g., `"Voltage"`, `"RawI32"`), telling the frontend how to interpret the binary payload.

---

## 3. Implementation Plan

### Backend (`websocket_sink` stage)
1.  **State Management:** The stage must detect when a new stream begins or when `SensorMeta` changes (by tracking `meta_rev`).
2.  **Send `meta_update`:** On a new stream or change, serialize the full `SensorMeta` into the `meta_update` JSON format and send it to the broker.
3.  **Send `data_packet`:** For every data packet:
    -   Create the minimal JSON header.
    -   Serialize it to a JSON string.
    -   Get the raw byte slice (`&[u8]`) from the sample data (`Vec<f32>`, etc.).
    -   Construct the hybrid binary message: `[json_len_u32 | json_bytes | raw_sample_bytes]`.
    -   Send the combined `Vec<u8>` to the broker.

### Frontend (`EegDataContext`)
1.  **State Management:** Maintain a `Map<string, SensorMeta>` to store the metadata for each `topic`.
2.  **Message Dispatch:** When a WebSocket message arrives:
    -   If it's a string, parse it as JSON. If `message_type` is `meta_update`, update the state map with the new metadata for that `topic`.
    -   If it's a binary `ArrayBuffer`, parse it as a `data_packet`.
3.  **`data_packet` Parsing:**
    -   Read the 4-byte header length.
    -   Decode the JSON header.
    -   Look up the `SensorMeta` from the state map using the `topic` from the header.
    -   Create a zero-copy `Float32Array` (or other type) view on the rest of the buffer.
    -   Combine the looked-up `SensorMeta` and the new data packet for processing by the renderer.