'use client';

import { createContext, useContext, useEffect, useState, useCallback, useMemo, useRef } from 'react';

// Define the types for the events based on the API documentation
type EventType = 'pipeline_state' | 'parameter_update' | 'error' | 'info' | 'data_update' | 'PipelineFailed';

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
  data: {
    meta: Record<string, any>;
  };
}

interface PipelineFailedEvent {
  type: 'PipelineFailed';
  data: {
    error: string;
  };
}

type EventData = PipelineStateEvent | ParameterUpdateEvent | ErrorEvent | InfoEvent | DataUpdateEvent | SourceReadyEvent | PipelineFailedEvent;

interface EventStreamContextType {
  events: EventData[];
  addEvent: (event: EventData) => void;
  clearEvents: () => void;
  isConnected: boolean;
  error: string | null;
  fatalError: string | null; // New state for unrecoverable errors
  connect: () => void;
  disconnect: () => void;
}

const EventStreamContext = createContext<EventStreamContextType | undefined>(undefined);

export const useEventStream = () => {
  const context = useContext(EventStreamContext);
  if (!context) {
    throw new Error('useEventStream must be used within an EventStreamProvider');
  }
  return context;
};

export function EventStreamProvider({ children }: { children: React.ReactNode }) {
  const [events, setEvents] = useState<EventData[]>([]);
  const [isConnected, setIsConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fatalError, setFatalError] = useState<string | null>(null);
  const eventSourceRef = useRef<EventSource | null>(null);

  const addEvent = useCallback((event: EventData) => {
    setEvents(prevEvents => [...prevEvents, event]);
  }, []);

  const clearEvents = useCallback(() => {
    setEvents([]);
  }, []);

  const connect = useCallback(() => {
    // Prevent multiple connections
    if (eventSourceRef.current) {
      return;
    }

    // The URL should be relative to the current host, so it's correctly
    // proxied by the Next.js development server.
    const url = `/api/events`;

    console.log('[EventStream] Connecting to SSE endpoint:', url);
    setFatalError(null); // Clear fatal error on new connection attempt

    const eventSource = new EventSource(url);
    eventSourceRef.current = eventSource;

    eventSource.onopen = () => {
      console.log('[EventStream] Connection established');
      setIsConnected(true);
      setError(null);
    };

    eventSource.onmessage = (event) => {
      try {
        const parsedData = JSON.parse(event.data);
        // The event is an object with a single key which is the event type
        const eventType = Object.keys(parsedData)[0];
        const eventPayload = parsedData[eventType];
        const eventData = { type: eventType, data: eventPayload } as EventData;
        console.log('[EventStream] Received event:', eventData);
        if (eventData.type === 'PipelineFailed') {
          console.error(`[EventStream] Fatal pipeline error: ${eventData.data.error}`);
          setFatalError(eventData.data.error);
          disconnect(); // Stop trying to reconnect
        } else {
          addEvent(eventData);
        }
      } catch (err) {
        console.error('[EventStream] Error parsing event data:', err);
        addEvent({
          type: 'error',
          data: {
            message: `Failed to parse event data: ${err instanceof Error ? err.message : String(err)}`
          }
        });
      }
    };

    eventSource.onerror = (err) => {
      console.error('[EventStream] EventSource error:', err);
      setError('Connection to event stream failed');
      setIsConnected(false);
      // Don't set eventSourceRef.current to null here - let disconnect handle cleanup
    };

  }, [addEvent]);

  const disconnect = useCallback(() => {
    if (eventSourceRef.current) {
      console.log('[EventStream] Disconnecting from SSE endpoint');
      eventSourceRef.current.close();
      eventSourceRef.current = null;
      setIsConnected(false);
      setError(null);
    }
  }, []);

  // Clean up on unmount
  useEffect(() => {
    return () => {
      disconnect();
    };
  }, [disconnect]);

  // Auto-connect on mount
  useEffect(() => {
    connect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const value = useMemo(() => ({
    events,
    addEvent,
    clearEvents,
    isConnected,
    error,
    fatalError,
    connect,
    disconnect
  }), [events, addEvent, clearEvents, isConnected, error, fatalError, connect, disconnect]);

  return (
    <EventStreamContext.Provider value={value}>
      {children}
    </EventStreamContext.Provider>
  );
}