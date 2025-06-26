# FFT Plugin and Protocol Refinement Plan

**Status:** Proposed
**Author(s):** Roo (AI Architect), Human User

## 1. Problem Summary

The `brain_waves_fft` plugin is not displaying data in the Kiosk UI. The root cause is an ambiguous binary wire protocol between the backend and frontend. The current implementation attempts to parse all binary messages as raw EEG data, causing it to misinterpret FFT packets. Initial plans to fix this risked violating the project's decoupled plugin architecture.

## 2. Core Goals

- **Fix:** Correctly parse and display FFT data in the Kiosk UI.
- **Architect:** Establish a robust, versioned, and performant binary wire protocol that adheres strictly to the decoupled plugin architecture.
- **Improve:** Enhance system debuggability and long-term maintainability.
- **Test:** Ensure the solution is thoroughly tested at both the unit and integration levels.

## 3. The Plan: A Hybrid, Architecturally-Sound Protocol

We will implement a new binary wire protocol that is unambiguous, efficient, and extensible.

### Protocol Structure

Each binary WebSocket message will have the following structure:

`| 0: VERSION (u8) | 1: TOPIC (u8) | 2..n: PAYLOAD (Bytes) |`

- **VERSION:** A protocol version number. The initial version will be `1`. Allows for future, non-breaking changes.
- **TOPIC:** An enum identifying the payload's content type.
- **PAYLOAD:** The raw, serialized data for the given topic.

## 4. Backend Implementation

### 4.1. `eeg_types` Crate (`crates/eeg_types/src/event.rs`)

- Define the protocol version constant.
  ```rust
  pub const PROTOCOL_VERSION: u8 = 1;
  ```
- Define the `WebSocketTopic` enum.
  ```rust
  #[repr(u8)]
  pub enum WebSocketTopic {
      FilteredEeg = 0,
      Fft = 1,
      Log = 255, // For human-readable debug/error messages
  }
  ```
- Define a new `SensorEvent` variant for broadcasting, using `Bytes` for a zero-copy payload.
  ```rust
  use bytes::Bytes;

  pub enum SensorEvent {
      // Internal events, like RawEeg and FilteredEeg, remain.
      
      // New variant for all data destined for the WebSocket:
      WebSocketBroadcast {
          topic: WebSocketTopic,
          payload: Bytes, // Use Bytes for efficient, zero-copy slices
      },
  }
  ```

### 4.2. Plugin Responsibility (e.g., `plugins/brain_waves_fft/src/lib.rs`)

- Plugins that send data to the UI are now responsible for serializing their data into a `Bytes` object.
- They will then broadcast this data using the new `SensorEvent::WebSocketBroadcast` variant.
  ```rust
  // Example in a plugin's run loop:
  let my_data_packet = produce_data_for_ui();
  let payload_bytes = Bytes::from(serde_json::to_vec(&my_data_packet)?);

  bus.broadcast(SensorEvent::WebSocketBroadcast {
      topic: WebSocketTopic::Fft,
      payload: payload_bytes,
  }).await;
  ```

### 4.3. `ConnectionManager` (`crates/device/src/connection_manager.rs`)

- The `ConnectionManager` will *only* subscribe to `SensorEvent::WebSocketBroadcast`.
- Its logic is simplified to be a content-agnostic forwarder.
- It will efficiently prepend the header bytes without re-allocating the payload.
  ```rust
  // Logic for handling the broadcast event:
  let header = [PROTOCOL_VERSION, event.topic as u8];
  let message_to_send = Bytes::from(header.to_vec()).chain(event.payload);
  
  websocket_sink.send(Message::binary(message_to_send)).await?;
  ```

## 5. Frontend Implementation (`kiosk/src/components/EegDataHandler.tsx`)

- The `onmessage` handler will be updated to parse the new protocol.
- It will not decode the entire buffer to a string; it will route the raw payload to the correct handler.
  ```typescript
  // Logic within the ws.onmessage handler:
  if (event.data instanceof ArrayBuffer) {
    const buffer = new Uint8Array(event.data);
    if (buffer.length < 2) {
      return; // Not a valid packet
    }

    const version = buffer[0];
    const topic = buffer[1];
    const payload = buffer.slice(2); // A zero-copy view of the payload

    if (version !== 1) {
      console.error(`Received unsupported protocol version: ${version}`);
      return;
    }

    switch (topic) {
      case 0: // FilteredEeg
        handleEegPayload(payload); // This handler parses the binary EEG data
        break;
      case 1: // Fft
        handleFftPayload(payload); // This handler parses the binary FFT data
        break;
      case 255: // Log
        handleLogPayload(payload); // This handler uses TextDecoder to get a string
        break;
      default:
        console.warn(`Received message with unknown topic ID: ${topic}`);
    }
  }
  ```

## 6. Testing Strategy

- **Unit Tests:** Each plugin's serialization logic will be tested against a "golden fixture" file to ensure its byte output is correct and consistent.
- **Integration Tests:** An end-to-end test will be created. It will spin up the `device` crate with an in-process WebSocket server, broadcast one of each `WebSocketTopic`, and assert that a test client receives and correctly parses each message.

## 7. Next Steps

1.  Final approval of this plan.
2.  Switch to `code` mode to begin implementation, starting with the `eeg_types` crate.