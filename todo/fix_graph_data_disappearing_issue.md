# Fix Graph Data Disappearing Issue

## Problem Summary
The graph data in the kiosk was disappearing quickly due to frequent WebSocket reconnections and inefficient data processing in the new EegDataContext architecture.

## Root Cause Analysis

### Primary Issues Identified:

1. **Data Accumulation in EegDataContext**: The `rawSamples` array was growing unbounded, causing memory issues and performance degradation.

2. **Unstable Refs in EegDataContext**: New refs were being created on every render, causing unnecessary WebSocket reconnections in `useEegDataHandler`.

3. **Inefficient Data Processing in EegMonitor**: The data processing effect was reprocessing ALL samples every time `rawSamples` changed, leading to data loss during reconnections.

4. **WebSocket Instability**: Code 1006 disconnections were not being handled optimally, causing frequent reconnections.

### Console Log Evidence:
```
[EegRenderer InitEffect2] Adding/Updating 3 lines.
WebSocket closed with code: 1006, reason: 
Attempting to reconnect in 500ms (attempt 1)
[EegMonitor] Creating 3 WebGL lines with 1000 points each.
```

## Implemented Solutions

### 1. Fixed Data Accumulation (EegDataContext.tsx)
- **Added circular buffer**: Limited `rawSamples` to `MAX_SAMPLE_CHUNKS = 100`
- **Added timestamp tracking**: Track sample timestamps for cleanup
- **Added periodic cleanup**: Remove old data every 10 seconds
- **Added reconnection state management**: Track reconnection status

### 2. Fixed Unstable Refs (EegDataContext.tsx)
- **Created stable refs**: Moved ref creation outside of useEegDataHandler call
- **Prevented unnecessary reconnections**: Stable refs prevent WebSocket restarts

### 3. Improved Data Processing (EegMonitor.tsx)
- **Added incremental processing**: Only process new samples since last processed index
- **Prevented data reprocessing**: Track `lastProcessedIndexRef` to avoid duplicate processing
- **Preserved data during reconnections**: Data isn't lost when effects re-run

### 4. Enhanced WebSocket Stability (EegDataHandler.tsx)
- **Better error handling**: Distinguish between expected and unexpected closures
- **Smarter reconnection logic**: Different delays for different closure types
- **Connection attempt limits**: Stop after 10 failed attempts
- **Reset reconnection counter**: Reset on successful connection

## Technical Details

### Data Flow Before Fix:
```
WebSocket Data → EegDataContext (unbounded array) → EegMonitor (reprocess all) → WebGL Lines
                                ↓
                        Memory issues + data loss during reconnections
```

### Data Flow After Fix:
```
WebSocket Data → EegDataContext (circular buffer) → EegMonitor (incremental processing) → WebGL Lines
                                ↓
                        Stable memory usage + data persistence during reconnections
```

### Key Code Changes:

1. **EegDataContext.tsx**:
   - Added `MAX_SAMPLE_CHUNKS`, `sampleTimestamps`, `cleanupOldData()`
   - Created stable refs: `lastDataChunkTimeRef`, `latestTimestampRef`, `debugInfoRef`
   - Added `isReconnecting` state and better error handling

2. **EegDataHandler.tsx**:
   - Enhanced `onclose` handler with better error classification
   - Added reconnection attempt limits and smarter delays
   - Reset reconnection counter on successful connection

3. **EegMonitor.tsx**:
   - Added `lastProcessedIndexRef` to track processed samples
   - Changed from full reprocessing to incremental processing
   - Added reset logic when WebGL lines are recreated

## Expected Outcomes

✅ **Graph data persistence**: Data should no longer disappear during WebSocket reconnections
✅ **Memory stability**: Circular buffer prevents unbounded memory growth
✅ **Reduced reconnections**: Stable refs prevent unnecessary WebSocket restarts
✅ **Better performance**: Incremental processing reduces CPU usage
✅ **Improved error handling**: Better distinction between connection issues

## Testing Recommendations

1. **Monitor WebSocket stability**: Check for reduced 1006 disconnections
2. **Verify data persistence**: Ensure graph continues during brief disconnections
3. **Check memory usage**: Confirm memory doesn't grow unbounded over time
4. **Test reconnection scenarios**: Manually disconnect/reconnect backend
5. **Performance monitoring**: Verify smooth graph updates at expected frame rates

## Future Considerations

- **Add data compression**: For high-frequency data, consider compression
- **Implement data buffering**: Buffer data during longer disconnections
- **Add connection quality metrics**: Track connection stability over time
- **Consider WebSocket heartbeat**: Implement ping/pong for connection health

## Related Files Modified

- `kiosk/src/context/EegDataContext.tsx` - Core data management fixes
- `kiosk/src/components/EegDataHandler.tsx` - WebSocket stability improvements
- `kiosk/src/components/EegMonitor.tsx` - Efficient data processing

## Date
2025-06-23

## Status
✅ **IMPLEMENTED** - All fixes applied and ready for testing

## Update: Fast Scrolling Issue Fix (2025-06-23)

### Additional Problem Identified
After the initial fixes, users reported that the graph data was still "teleporting off the graph" and "scrolling so fast" like it was being fast-forwarded.

### Root Cause of Fast Scrolling
The issue was in the data processing logic in [`EegMonitor.tsx`](../kiosk/src/components/EegMonitor.tsx:345). The component was adding entire batches of samples (25 samples) at once to the WebGL display using `shiftAdd(channelSamples)`, causing the graph to jump forward by 25 samples worth of time in a single frame.

**Problem Flow:**
1. Mock driver generates batches of 25 samples every 100ms
2. EegMonitor receives these batches via the context
3. **BUG**: `lines[chIndex].shiftAdd(channelSamples)` adds all 25 samples at once
4. Graph jumps forward by 25 samples (100ms worth) instantly
5. Result: Fast-forward scrolling effect

### Solution Implemented
**Sample Queue with Rate-Limited Display:**

1. **Added Sample Queue**: Created `sampleQueueRef` to buffer individual samples
2. **Batch Decomposition**: Convert incoming batches into individual samples and queue them
3. **Rate-Limited Display**: Use `setInterval` at 60 FPS to add samples at controlled rate
4. **Smooth Scrolling**: Add only 1 sample per display frame for smooth real-time visualization

### Code Changes Made

#### [`kiosk/src/utils/eegConstants.ts`](../kiosk/src/utils/eegConstants.ts:35)
```typescript
// Display timing constants for smooth real-time visualization
export const DISPLAY_FPS = 60; // Target display frame rate
export const SAMPLES_PER_DISPLAY_FRAME = 1; // Number of samples to add per display frame
export const DISPLAY_FRAME_INTERVAL_MS = 1000 / DISPLAY_FPS; // ~16.67ms between frames
```

#### [`kiosk/src/components/EegMonitor.tsx`](../kiosk/src/components/EegMonitor.tsx:47)
- Added `sampleQueueRef` for buffering individual samples
- Added `displayIntervalRef` for managing the display timer
- Replaced batch processing with sample queuing logic
- Added rate-limited display using `setInterval` at 60 FPS
- Added proper cleanup for intervals and queue

### Technical Details

**Before Fix:**
```
Batch[25 samples] → shiftAdd(all 25) → Graph jumps 100ms forward
```

**After Fix:**
```
Batch[25 samples] → Queue[sample1, sample2, ...] → Display Timer → shiftAdd(1 sample) every 16.67ms
```

### Expected Results
✅ **Smooth real-time scrolling** - Data moves at expected speed (not fast-forwarded)
✅ **Consistent timing** - No more sudden jumps or teleporting
✅ **Better visual quality** - Readable, stable waveforms
✅ **Proper buffering** - Handles data bursts without overwhelming display

### Testing Notes
- Monitor console logs for queue size management
- Verify smooth scrolling at expected real-time speed
- Check that data doesn't accumulate excessively in the queue
- Ensure proper cleanup when switching views