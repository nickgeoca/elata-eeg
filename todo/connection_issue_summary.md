# Connection Issue Summary

This document summarizes the connection issues encountered between the frontend Kiosk application and the backend EEG daemon, the steps taken to resolve them, and the current hypothesis.

## The Situation

The web application is unable to establish stable connections to its backend services. This manifests as:
- **WebSocket Failures:** Both the data WebSocket (for EEG data) and the command WebSocket fail to connect, resulting in `WebSocket closed with code: 1006` and `Firefox can’t establish a connection` errors in the browser console.
- **SSE Errors:** The Server-Sent Events (SSE) connection for real-time events is also unstable, showing `EventSource error` messages.
- **Non-functional UI:** Due to these connection failures, the application's core functionality is broken.

## What We've Tried

We have attempted several strategies to resolve the issue, primarily focused on correctly proxying requests from the Next.js development server (port 3000) to the backend services (ports 9000 and 9001).

1.  **Initial Analysis:** Identified that the frontend was making direct requests to ports 9000 and 9001, which were being blocked by the browser's Same-Origin Policy.
2.  **Next.js `rewrites`:** Updated `next.config.ts` to use the `rewrites` property. This only works for HTTP requests and did not resolve the WebSocket issues.
3.  **Next.js `devServer` Proxy:** Switched to the `devServer` proxy configuration in `next.config.ts`, which is the modern approach for handling WebSockets. This failed, likely due to a cross-origin issue that was not yet visible.
4.  **`devServer` with `allowedDevOrigins`:** After identifying a cross-origin error, we attempted to add `allowedDevOrigins` to the `next.config.ts`. This resulted in a TypeScript error, suggesting the Next.js version in use does not support this property.
5.  **Custom API Route with `http-proxy-middleware`:** Created a custom API route to handle proxying. This failed with a `TypeError: socket.on is not a function`, indicating an incompatibility between the middleware and the Next.js API route environment.
6.  **Custom Node.js Server:** Created a custom `server.js` file with `http-proxy` and updated the `dev` script in `package.json` to use it. This is the current approach, but it is still failing to proxy WebSocket connections correctly.

## Current Hypothesis

The core issue is that the custom Node.js server (`kiosk/server.js`) is not correctly handling the WebSocket `upgrade` event. The current implementation only proxies standard HTTP requests (`proxy.web(...)`) and does not listen for the `upgrade` event on the server, which is necessary to hand off the WebSocket connection to the proxy.

The backend services themselves appear to be running correctly, as shown by the terminal logs. The problem lies entirely within the proxy layer.

## Relevant Files

-   **`kiosk/server.js`**: The custom Node.js server responsible for proxying. **This is the most likely location for the fix.**
-   **`kiosk/next.config.ts`**: The Next.js configuration file. It has been simplified to remove all conflicting proxy settings.
-   **`kiosk/package.json`**: Contains the `dev` script that runs the custom server.
-   **`kiosk/src/components/EegDataHandler.tsx`**: Frontend component that establishes the data WebSocket connection.
-   **`kiosk/src/context/CommandWebSocketContext.tsx`**: Frontend context that establishes the command WebSocket connection.
-   **`crates/daemon/src/main.rs`**: The main entry point for the backend daemon.
-   **`crates/pipeline/src/stages/websocket_sink.rs`**: The implementation of the data WebSocket sink on the backend.