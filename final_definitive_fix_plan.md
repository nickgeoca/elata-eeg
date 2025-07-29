# Definitive Fix for State Synchronization

This plan refactors the component communication to eliminate the race condition by consolidating all connection logic within the `EegDataHandler`.

### 1. Modify `kiosk/src/context/EegDataContext.tsx`

The `useEegDataHandler` hook will now be given the `pipelineStatus` directly. The conditional logic will be removed.

```diff
--- a/kiosk/src/context/EegDataContext.tsx
+++ b/kiosk/src/context/EegDataContext.tsx
@@ -272,8 +272,8 @@
   }, [isReconnecting]);
 
   const { status: wsStatus } = useEegDataHandler({
-    // Only pass the config (and thus enable the handler) when the pipeline is started.
-    config: pipelineStatus === 'started' ? config : null,
+    config: config,
+    status: pipelineStatus, // Pass status directly
     onDataUpdate: handleDataUpdate,
     onError: handleError,
     onSamples: handleSamples,

```

### 2. Modify `kiosk/src/components/EegDataHandler.tsx`

The hook will now accept the `status` prop, and its main `useEffect` will use it to make the connection decision.

```diff
--- a/kiosk/src/components/EegDataHandler.tsx
+++ b/kiosk/src/components/EegDataHandler.tsx
@@ -18,6 +18,7 @@
  
 interface EegDataHandlerProps {
   config: any;
+  status: 'stopped' | 'starting' | 'started' | 'error';
   onDataUpdate: (dataReceived: boolean) => void;
   onError?: (error: string) => void;
   onSamples: (samples: { values: Float32Array; timestamps: BigUint64Array }[]) => void;
@@ -35,6 +36,7 @@
 
 export function useEegDataHandler({
   config,
+  status,
   onDataUpdate,
   onError,
   onSamples,
@@ -127,9 +129,9 @@
     console.log(`[EegDataHandler] Effect running to establish WebSocket connection.`);
     let isMounted = true;
 
-    // If config is not provided, we should not attempt to connect.
+    // Only connect if the pipeline is started and we have a valid config.
     // Clean up any existing connection and exit the effect.
-    if (!config) {
+    if (status !== 'started' || !config) {
       console.log("[EegDataHandler] No config provided. Ensuring WebSocket is closed.");
       if (wsRef.current) {
         wsRef.current.close();
@@ -403,7 +405,7 @@
         wsRef.current = null;
       }
     };
-  }, [config]); // Re-run effect when config changes
+  }, [config, status]); // Re-run effect when config or status changes
 
   // Return status and debug info
   return {
