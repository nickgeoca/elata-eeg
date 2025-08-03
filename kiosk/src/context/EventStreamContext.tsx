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
  | 'FilteredEeg'
  | 'Fft'
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

interface FilteredEegEvent {
  type: 'FilteredEeg';
  data: any; // Base64 encoded binary data
}

interface FftEvent {
  type: 'Fft';
  data: any;
}

type EventData =
  | PipelineStateEvent
  | ParameterUpdateEvent
  | ErrorEvent
  | InfoEvent
  | DataUpdateEvent
  | SourceReadyEvent
  | PipelineFailedEvent
  | FilteredEegEvent
  | FftEvent;

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

  const disconnect = useCallback(() => {
    if (eventSourceRef.current) {
      console.log('[EventStream] Disconnecting from SSE endpoint');
      eventSourceRef.current.close();
      eventSourceRef.current = null;
      setIsConnected(false);
    }
  }, []);

  const connect = useCallback(() => {
    // Manual connection is not the focus of this refactor.
    // The primary goal is a stable auto-connection on mount.
    if (eventSourceRef.current) {
        console.log('[EventStream] Already connected.');
        return;
    }
    console.log('[EventStream] Manual connect is not implemented, connection is handled by useEffect.');
  }, []);

  useEffect(() => {
    if (eventSourceRef.current) {
      return;
    }

    const relativeUrl = '/api/events';
    console.log('[EventStream] Connecting to SSE endpoint:', relativeUrl);
    setFatalError(null);

    const es = new EventSource(relativeUrl);
    eventSourceRef.current = es;

    const handleDisconnect = () => {
      if (eventSourceRef.current) {
        console.log('[EventStream] Closing SSE connection.');
        eventSourceRef.current.close();
        eventSourceRef.current = null;
        setIsConnected(false);
      }
    };

    es.onopen = () => {
      console.log('[EventStream] SSE connection established');
      setIsConnected(true);
      setError(null);
    };

    es.onmessage = (event) => {
      try {
        const parsedData = JSON.parse(event.data);
        const eventType = Object.keys(parsedData)[0];
        const eventPayload = parsedData[eventType];
        const eventData = { type: eventType, data: eventPayload } as EventData;

        if (eventData.type === 'PipelineFailed') {
          console.error(`[EventStream] Fatal pipeline error: ${eventData.data.error}`);
          setFatalError(eventData.data.error);
          handleDisconnect();
        } else {
          setEvents(prevEvents => [...prevEvents, eventData]);
        }
      } catch (err) {
        console.error('[EventStream] Error parsing event data:', err);
      }
    };

    es.onerror = (err) => {
      console.error('[EventStream] SSE error:', err);
      setError('SSE connection failed. Retrying...');
      setIsConnected(false);
      // EventSource handles reconnection automatically.
    };

    return () => {
      handleDisconnect();
    };
  }, []); // Empty dependency array ensures this runs only once on mount.

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