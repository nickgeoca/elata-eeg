# Plan to Fix Data Disappearing Issue

## 1. The Problem

The EEG graphs (`EegRenderer` and `EegCircularGraph`) are showing a "no data" message because of a race condition between the data buffering and rendering logic. The rendering components are using a `requestAnimationFrame` loop to continuously poll for new data, which often results in the data buffer being cleared before the data can be processed.

## 2. The Solution

The solution is to refactor the rendering components to be more reactive to data changes, eliminating the continuous polling. This will be achieved by:

1.  **Triggering re-renders from `EegMonitor.tsx`**: A state variable will be added to `EegMonitor.tsx` that will be updated whenever new data is received. This will trigger a re-render of the component and its children.
2.  **Removing `requestAnimationFrame` loops**: The `requestAnimationFrame` loops will be removed from `EegCircularGraph.tsx` and `EegRenderer.tsx`. The rendering logic will be moved into the body of the component, so that it is executed whenever the component re-renders.

## 3. Step-by-Step Implementation

### Step 1: Modify `EegMonitor.tsx`

1.  Add a new state variable to `EegMonitor.tsx` to track the data version:

    ```typescript
    const [eegDataVersion, setEegDataVersion] = useState(0);
    ```

2.  In the `useEffect` hook that subscribes to the raw data stream, update the `eegDataVersion` whenever new data is received:

    ```typescript
    unsubSignal = subscribeRaw((newSampleChunks) => {
      if (newSampleChunks.length > 0) {
        signalGraphBuffer.addData(newSampleChunks);
        setEegDataVersion(v => v + 1); // Trigger re-render
      }
    });
    ```

    Do the same for the circular graph subscription.

### Step 2: Modify `EegCircularGraph.tsx`

1.  Remove the `useEffect` hook that sets up the `requestAnimationFrame` loop.
2.  Move the data processing logic from the `render` function into the body of the component:

    ```typescript
    const sampleChunks = dataBuffer.getAndClearData();

    if (sampleChunks.length > 0 && rendererRef.current) {
      sampleChunks.forEach((chunk: SampleChunk) => {
        chunk.samples.forEach((sample) => {
          const chIndex = sample.channelIndex;
          if (chIndex < numChannels && rendererRef.current) {
            rendererRef.current.addNewSample(chIndex, sample.value);
          }
        });
      });
    }
    ```

### Step 3: Modify `EegRenderer.tsx`

1.  Remove the `renderLoop` function and the `useEffect` hook that sets it up.
2.  Move the data processing logic from the `renderLoop` function into the body of the component:

    ```typescript
    const sampleChunks = dataBuffer.getAndClearData();

    if (sampleChunks.length > 0) {
      // ... (rest of the data processing logic)
    }

    if (wglpRef.current) {
      wglpRef.current.update();
    }
    ```

## 4. Expected Outcome

These changes will ensure that the rendering components only update when there is new data to display, eliminating the race condition and resolving the "no data" issue. The data flow will be more robust and predictable.