# Plan: Fix Graph Re-Render Loop

**Date:** 2025-06-24
**Status:** Proposed

## 1. Problem Summary

The user has reported that the EEG graph visualizations "flash and disappear." Console logs confirm that React components are caught in a rapid re-render loop, causing them to unmount and remount continuously. This is triggered by frequent "configuration change" events, even when the configuration data itself is identical to the previous state.

## 2. Root Cause Analysis

The root cause is an unstable context value in `kiosk/src/components/EegConfig.tsx`. The `EegConfigContext.Provider` creates a new `value` object on every render:

```tsx
// This creates a new object on every render
<EegConfigContext.Provider value={{ config, status, refreshConfig, isConfigReady }}>
  {children}
</EegConfigContext.Provider>
```

In React, passing a new object reference to a context provider forces all consuming components to re-render. This is happening on every state change within `EegConfigProvider`, leading to a chain reaction that tears down and rebuilds the graph components.

## 3. Solution: Memoize the Context Value

The solution is to stabilize the context value by memoizing it with the `useMemo` hook. This ensures that the context value object reference only changes when its underlying data (`config`, `status`, etc.) actually changes.

### Implementation Steps

1.  **File to Modify:** `kiosk/src/components/EegConfig.tsx`

2.  **Action:**
    *   Import the `useMemo` hook from React.
    *   Wrap the context provider's `value` object in a `useMemo` hook.
    *   Add the properties passed in the value object (`config`, `status`, `refreshConfig`, `isConfigReady`) to the `useMemo` dependency array.

    **Example:**
    ```tsx
    const contextValue = useMemo(() => ({
      config,
      status,
      refreshConfig,
      isConfigReady
    }), [config, status, refreshConfig, isConfigReady]);

    return (
      <EegConfigContext.Provider value={contextValue}>
        {children}
      </EegConfigContext.Provider>
    );
    ```

3.  **Verification:** After applying the fix, we will need to run the application and monitor the browser console. The re-render loop should be gone, and the graph should render stably without flashing.

## 4. Expected Outcome

- The component re-render loop will be broken.
- The graph visualization will be stable and no longer flash.
- The application's overall performance and responsiveness will improve.