'use client';

import { createContext, useContext, useEffect, useState, useCallback, useMemo, useRef } from 'react';

// Define the types for the events based on the API documentation
type EventType =
  | 'pipeline_state'
  | 'parameter_update'
  | 'error'
  | 'info'
  | 'data_update'
  | 'PipelineFailed'
  | 'SourceReady';

interface PipelineStateEvent {
  type: 'pipeline_state';
  data: {
    id: string;
    name: string;
    status: 'running' | 'stopped' | 'error';
    stages: Array<{
      id: string;
      name: string;
      parameters: Record<string, any>;
    }>;
  };
}

interface ParameterUpdateEvent {
  type: 'parameter_update';
  data: {
    stage_id: string;
    parameter_id: string;
    value: any;
  };
}

interface ErrorEvent {
  type: 'error';
  data: {
    message: string;
    code?: string;
  };
}

interface InfoEvent {
  type: 'info';
  data: {
    message: string;
  };
}

interface DataUpdateEvent {
  type: 'data_update';
  data: {
    timestamp: number;
    sample_count: number;
  };
}

interface SourceReadyEvent {
  type: 'SourceReady';
  data: Record<string, any>;
}

interface PipelineFailedEvent {
  type: 'PipelineFailed';
  data: {
    error: string;
  };
}

type EventData =
  | PipelineStateEvent
  | ParameterUpdateEvent
  | ErrorEvent
  | InfoEvent
  | DataUpdateEvent
  | SourceReadyEvent
  | PipelineFailedEvent;

// Separate stable and dynamic context values
interface EventStreamContextStableType {
  subscribe: (eventType: string, callback: (data: any) => void) => () => void;
  connect: () => void;
  disconnect: () => void;
}

interface EventStreamContextDynamicType {
  isConnected: boolean;
  error: string | null;
  fatalError: string | null;
}

const EventStreamStableContext = createContext<EventStreamContextStableType | undefined>(undefined);
const EventStreamDynamicContext = createContext<EventStreamContextDynamicType | undefined>(undefined);

export const useEventStream = () => {
  const context = useContext(EventStreamStableContext);
  if (!context) {
    throw new Error('useEventStream must be used within an EventStreamProvider');
  }
  return context;
};

export const useEventStreamData = () => {
  const context = useContext(EventStreamDynamicContext);
  if (!context) {
    throw new Error('useEventStreamData must be used within an EventStreamProvider');
  }
  return context;
};

export function EventStreamProvider({ children }: { children: React.ReactNode }) {
  const [isConnected, setIsConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fatalError, setFatalError] = useState<string | null>(null);
  const eventSourceRef = useRef<EventSource | null>(null);
  const listeners = useRef<Record<string, Record<string, (data: any) => void>>>({});
  
  // Use window property to track connection attempts in React Strict Mode
  // This persists across double executions unlike refs which are reset per component instance
  const connectionGuardKey = '__eventstream_connection_guard__';

  const subscribe = useCallback((eventType: string, callback: (data: any) => void) => {
    const id = Math.random().toString(36).substring(2, 9);
    if (!listeners.current[eventType]) {
      listeners.current[eventType] = {};
    }
    listeners.current[eventType][id] = callback;

    // Return an unsubscribe function
    return () => {
      delete listeners.current[eventType][id];
      if (Object.keys(listeners.current[eventType]).length === 0) {
        delete listeners.current[eventType];
      }
    };
  }, []);

  const disconnect = useCallback(() => {
    if (eventSourceRef.current) {
      console.log('[EventStream] Disconnecting from SSE endpoint');
      eventSourceRef.current.close();
      eventSourceRef.current = null;
      setIsConnected(false);
    }
  }, []);

  const connect = useCallback(() => {
    // This is a no-op as the connection is managed by useEffect
    console.log('[EventStream] Connection is managed automatically by the provider.');
  }, []);

  useEffect(() => {
    // Check if we're in React Strict Mode development double-run scenario
    // In Strict Mode, the first run sets the window guard, and the second run should be ignored
    // @ts-ignore - Accessing custom property on window object
    if (window[connectionGuardKey]) {
      console.log('[EventStream] Connection attempt already made, skipping duplicate connection attempt.');
      return;
    }

    // Ensure we don't create duplicate connections
    if (eventSourceRef.current) {
      console.log('[EventStream] Duplicate connection.');
      return;
    }
    
    // Set the connection guard on window to prevent duplicate connections
    // @ts-ignore - Adding custom property to window object
    window[connectionGuardKey] = true;

    console.log('[EventStream] Connecting to SSE endpoint...');
    const eventSource = new EventSource('/api/events');
    eventSourceRef.current = eventSource;
    setFatalError(null);

    eventSource.onopen = () => {
      console.log('[EventStream] SSE connection established.');
      setIsConnected(true);
      setError(null);
    };

    eventSource.onmessage = (event) => {
      try {
        const parsedData = JSON.parse(event.data);
        const eventType = Object.keys(parsedData)[0] as EventType;
        const eventPayload = parsedData[eventType];
        const eventData = { type: eventType, data: eventPayload } as EventData;

        if (eventData.type === 'PipelineFailed') {
          console.error(`[EventStream] Fatal pipeline error: ${eventData.data.error}`);
          setFatalError(eventData.data.error);
          // On fatal error, permanently close the connection.
          eventSource.close();
          setIsConnected(false);
        }

        // Publish the event to all registered listeners for this event type
        if (listeners.current[eventType]) {
          Object.values(listeners.current[eventType]).forEach(callback => {
            try {
              callback(eventPayload);
            } catch (e) {
              console.error(`[EventStream] Error in event listener for ${eventType}:`, e);
            }
          });
        }
      } catch (err) {
        console.error('[EventStream] Error parsing event data:', err);
      }
    };

    eventSource.onerror = (err) => {
      // In React Strict Mode, a quick mount/unmount/remount cycle can trigger a benign error
      // when the initial connection is aborted. We check the readyState to ensure we only
      // act on genuine errors from an active or connecting stream.
      if (eventSource.readyState === EventSource.OPEN || eventSource.readyState === EventSource.CONNECTING) {
        console.error('[EventStream] SSE connection error:', err);
        setError('SSE connection failed. The connection will be retried automatically.');
        setIsConnected(false);
      } else {
        // Reset connection guard on window when connection fails
        // @ts-ignore - Adding custom property to window object
        window[connectionGuardKey] = false;
      }
      // If the state is CLOSED, we assume it was intentional (e.g., via the cleanup function)
      // and we don't want to display an error.
    };

    // Return a cleanup function to be called on component unmount
    return () => {
      console.log('[EventStream] Closing SSE connection on unmount.');
      eventSource.close();
      eventSourceRef.current = null;
      // Reset the connection guard on window to allow for new connection attempts.
      // @ts-ignore - Adding custom property to window object
      window[connectionGuardKey] = false;
    };
  }, []); // Empty dependency array ensures this effect runs only once on mount

  const stableValue = useMemo(() => ({
    subscribe,
    connect,
    disconnect,
  }), [subscribe, connect, disconnect]);

  const dynamicValue = useMemo(() => ({
    isConnected,
    error,
    fatalError,
  }), [isConnected, error, fatalError]);

  return (
    <EventStreamStableContext.Provider value={stableValue}>
      <EventStreamDynamicContext.Provider value={dynamicValue}>
        {children}
      </EventStreamDynamicContext.Provider>
    </EventStreamStableContext.Provider>
  );
}