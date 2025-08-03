'use client';

import React, { createContext, useContext, useState, ReactNode, useEffect, useCallback, useMemo, useRef } from 'react';
import { getPipelines, startPipeline as apiStartPipeline, getPipelineState } from '../utils/api';
import { SystemConfig } from '@/types/eeg';
import { useEventStream } from './EventStreamContext';

interface Pipeline {
  id: string;
  name: string;
}

export interface PipelineState {
  status: 'stopped' | 'starting' | 'started' | 'error';
  config: SystemConfig | null;
}

interface PipelineContextType {
  pipelines: Pipeline[];
  selectedPipeline: Pipeline | null;
  pipelineConfig: SystemConfig | null;
  pipelineStatus: 'stopped' | 'starting' | 'started' | 'error';
  selectAndStartPipeline: (id: string) => Promise<void>;
  pipelineState: PipelineState;
}

const PipelineContext = createContext<PipelineContextType | undefined>(undefined);

interface PipelineProviderProps {
  children: ReactNode;
}

export const PipelineProvider = ({ children }: PipelineProviderProps) => {
  const [pipelines, setPipelines] = useState<Pipeline[]>([]);
  const [selectedPipeline, setSelectedPipeline] = useState<Pipeline | null>(null);
  const [pipelineState, setPipelineState] = useState<PipelineState>({
    status: 'stopped',
    config: null,
  });
  const { events } = useEventStream();
  const initializationStarted = useRef(false);

  const selectAndStartPipeline = useCallback(async (id: string) => {
    const pipelineToStart = pipelines.find((p: Pipeline) => p.id === id);
    if (!pipelineToStart) {
      console.error(`Pipeline with id ${id} not found.`);
      setPipelineState({ status: 'error', config: null });
      return;
    }

    setPipelineState(prevState => ({ ...prevState, status: 'starting' }));
    setSelectedPipeline(pipelineToStart);

    try {
      await apiStartPipeline(id);
      console.log(`Pipeline ${id} start command issued successfully.`);
      // State will be updated via SSE events.
    } catch (error) {
      console.error(`Failed to start pipeline ${id}:`, error);
      setPipelineState({ status: 'error', config: null });
    }
  }, [pipelines]);

  useEffect(() => {
    if (initializationStarted.current) return;
    initializationStarted.current = true;

    const initialize = async () => {
      try {
        const availablePipelines = await getPipelines();
        setPipelines(availablePipelines);

        const currentState = await getPipelineState();
        if (currentState && currentState.stages.length > 0) {
          console.log('[PipelineProvider] A pipeline is already running. Syncing state.');
          setPipelineState({ status: 'started', config: currentState });
          const runningPipeline = availablePipelines.find((p: Pipeline) => p.id === currentState.id) || null;
          setSelectedPipeline(runningPipeline);
        } else {
          console.log('[PipelineProvider] No pipeline running. Starting default pipeline.');
          const defaultPipeline = availablePipelines.find((p: Pipeline) => p.id === 'default');
          if (defaultPipeline) {
            await selectAndStartPipeline(defaultPipeline.id);
          } else {
            console.error("No 'default' pipeline found.");
            setPipelineState({ status: 'error', config: null });
          }
        }
      } catch (error) {
        console.error("Failed to initialize pipeline:", error);
        setPipelineState({ status: 'error', config: null });
      }
    };

    initialize();
  }, [selectAndStartPipeline]);

  useEffect(() => {
    const lastEvent = events[events.length - 1];
    if (!lastEvent) return;

    if (lastEvent.type === 'pipeline_state') {
      const { data } = lastEvent;
      const newStatus = data.status === 'running' ? 'started' : data.status;
      setPipelineState(prevState => ({
        ...prevState,
        status: newStatus,
        // Do not update config from this event, as the shape is different.
        // The full config is loaded on initialization.
      }));
    } else if (lastEvent.type === 'PipelineFailed') {
      setPipelineState(prevState => ({ ...prevState, status: 'error' }));
    }
  }, [events]);

  const value = useMemo(() => ({
    pipelines,
    selectedPipeline,
    pipelineConfig: pipelineState.config,
    pipelineStatus: pipelineState.status,
    selectAndStartPipeline,
    pipelineState: pipelineState,
  }), [pipelines, selectedPipeline, pipelineState, selectAndStartPipeline]);

  return (
    <PipelineContext.Provider value={value}>
      {children}
    </PipelineContext.Provider>
  );
};

// Custom hook to use the pipeline context
export const usePipeline = () => {
  const context = useContext(PipelineContext);
  if (context === undefined) {
    throw new Error('usePipeline must be used within a PipelineProvider');
  }
  return context;
};