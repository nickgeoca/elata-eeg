# Fix EEG Data WebSocket Connection Issue

## Problem Summary
The kiosk is showing "no data" consistently. The configuration WebSocket is working properly (receiving config data), but the EEG data WebSocket at `/eeg` endpoint is not receiving data, causing `dataReceived` to remain `false`.

## Environment Details
- **Frontend**: Accessed via `http://raspberrypi.local:3000` from Mac
- **Backend**: Running on Raspberry Pi 5, listening on `ws://0.0.0.0:8080`
- **Backend Logs**: Show WebSocket connections and disconnections for both `/eeg` and `/config` endpoints
- **Frontend Logs**: Show config WebSocket working, but EEG data processing not triggering

## Root Cause Analysis

### What's Working ✅
1. **Network connectivity**: Can access kiosk UI via `raspberrypi.local:3000`
2. **Config WebSocket**: Successfully receiving configuration data
3. **Backend daemon**: Running and accepting WebSocket connections
4. **WebSocket connection logic**: All components correctly use `window.location.hostname`

### What's Not Working ❌
1. **EEG data reception**: `dataReceived` state remains `false`
2. **Data processing**: No binary data packets being processed in `EegDataHandler`
3. **Graph rendering**: Container size issue is secondary to no data

### Potential Causes
1. **Backend not sending data**: EEG driver might not be producing data
2. **Data format mismatch**: Frontend expecting different binary format than backend sends
3. **WebSocket message handling**: Binary data not being processed correctly
4. **Timing issue**: Data handler not properly initialized when WebSocket connects

## Investigation Plan

### Phase 1: Backend Data Generation Verification
1. **Check if backend is actually generating EEG data**
   - Look for data generation logs in backend
   - Verify mock EEG driver is running
   - Check if data is being sent over WebSocket

2. **Verify WebSocket data transmission**
   - Add logging to backend WebSocket handler for `/eeg` endpoint
   - Check if binary data packets are being sent
   - Verify packet structure matches frontend expectations

### Phase 2: Frontend Data Reception Debugging
1. **Add comprehensive logging to EegDataHandler**
   - Log WebSocket connection events
   - Log all received messages (binary and text)
   - Log data parsing attempts and failures

2. **Verify binary data parsing**
   - Check if `event.data instanceof ArrayBuffer` condition is met
   - Verify packet structure parsing (timestamp + error flag + data)
   - Log sample values being extracted

### Phase 3: Data Flow Integration Testing
1. **Test WebSocket connection lifecycle**
   - Verify connection establishment
   - Test reconnection behavior
   - Check for connection drops

2. **Validate data processing pipeline**
   - Ensure `linesRef.current` is properly initialized
   - Verify `onDataUpdate(true)` is being called
   - Check timeout mechanism for `dataReceived` state

## Implementation Strategy

### Step 1: Enhanced Frontend Debugging
Add detailed logging to `EegDataHandler.tsx` to track:
- WebSocket connection status
- Message reception events
- Binary data parsing steps
- Data processing success/failure

### Step 2: Backend Data Flow Verification
Check backend logs and add debugging to verify:
- EEG data generation
- WebSocket message transmission
- Binary packet structure

### Step 3: Fix Data Reception Issues
Based on debugging results:
- Fix binary data parsing if format mismatch
- Resolve WebSocket connection issues
- Ensure proper data handler initialization

## Expected Outcome
After implementing this plan:
- ✅ EEG data WebSocket will receive binary data packets
- ✅ `dataReceived` state will become `true`
- ✅ Graph lines will be created and populated with data
- ✅ All visual plugins will receive data
- ✅ "no data" status will change to "receiving data"

## Files to Modify
1. **Primary**: `kiosk/src/components/EegDataHandler.tsx` - Add debugging and fix data reception
2. **Secondary**: Backend WebSocket handlers - Add data transmission logging
3. **Testing**: Add temporary debugging components for data flow visualization

## Next Steps
1. Add comprehensive debugging to frontend data handler
2. Verify backend data generation and transmission
3. Fix identified issues in data reception pipeline
4. Test with real EEG data flow

## Future Considerations
**Multi-Device Network Support**: The current hostname-based approach (`raspberrypi.local`) will break with multiple EEG devices on the same network. After fixing the immediate data issue, consider:
- Device discovery using mDNS/Bonjour
- Configuration option for backend hostname/IP
- Device selection UI for multiple EEG systems
- Unique device naming scheme (e.g., `eeg-device-001.local`)

**Related Future Task**: Create `todo/multi_device_network_support_plan.md`