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
  const webSocketRef = useRef<WebSocket | null>(null);

  const addEvent = useCallback((event: EventData) => {
    setEvents(prevEvents => [...prevEvents, event]);
  }, []);

  const clearEvents = useCallback(() => {
    setEvents([]);
  }, []);

  const disconnect = useCallback(() => {
    if (webSocketRef.current) {
      console.log('[EventStream] Disconnecting from WebSocket endpoint');
      webSocketRef.current.close();
      webSocketRef.current = null;
      setIsConnected(false);
    }
  }, []);

  const connect = useCallback(() => {
    if (webSocketRef.current) {
      return;
    }

    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const host = window.location.host;
    const url = `${protocol}//${host}/ws`;

    console.log('[EventStream] Connecting to WebSocket endpoint:', url);
    setFatalError(null);

    const newWebSocket = new WebSocket(url);
    webSocketRef.current = newWebSocket;

    newWebSocket.onopen = () => {
      console.log('[EventStream] WebSocket connection established');
      setIsConnected(true);
      setError(null);
      const subscriptionMessage = {
        subscribe: "eeg_voltage"
      };
      newWebSocket.send(JSON.stringify(subscriptionMessage));
    };

    newWebSocket.onmessage = (event) => {
      try {
        const parsedData = JSON.parse(event.data);
        const eventType = Object.keys(parsedData)[0];
        const eventPayload = parsedData[eventType];
        const eventData = { type: eventType, data: eventPayload } as EventData;

        if (eventData.type === 'PipelineFailed') {
          console.error(`[EventStream] Fatal pipeline error: ${eventData.data.error}`);
          setFatalError(eventData.data.error);
          disconnect();
        } else {
          addEvent(eventData);
        }
      } catch (err) {
        console.error('[EventStream] Error parsing event data:', err);
      }
    };

    newWebSocket.onerror = (err) => {
      console.error('[EventStream] WebSocket error:', err);
      setError('WebSocket connection failed. Retrying...');
      setIsConnected(false);
    };

    newWebSocket.onclose = () => {
      console.log('[EventStream] WebSocket connection closed. Attempting to reconnect...');
      setIsConnected(false);
      setTimeout(connect, 5000); // Reconnect after 5 seconds
    };
  }, [addEvent, disconnect]);

  useEffect(() => {
    connect();
    return () => {
      disconnect();
    };
  }, [connect, disconnect]);

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