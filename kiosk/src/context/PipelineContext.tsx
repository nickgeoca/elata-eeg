'use client';

import React, { createContext, useContext, useState, ReactNode, useEffect, useCallback, useMemo } from 'react';
import { getPipelines, startPipeline as apiStartPipeline, getPipelineState } from '../utils/api';
import { SystemConfig } from '@/types/eeg';
import { useEventStream } from './EventStreamContext'; // Import the event stream

// Define the shape of a single pipeline
interface Pipeline {
  id: string;
  name: string;
}

// New interface for the combined pipeline state
export interface PipelineState {
  status: 'stopped' | 'starting' | 'started' | 'error';
  config: SystemConfig | null;
}

// Define the shape of the context data
interface PipelineContextType {
  pipelines: Pipeline[];
  selectedPipeline: Pipeline | null;
  pipelineConfig: SystemConfig | null;
  pipelineStatus: 'stopped' | 'starting' | 'started' | 'error';
  selectAndStartPipeline: (id: string) => Promise<void>;
  pipelineState: PipelineState; // Expose the whole state object
}

// Create the context with a default value
const PipelineContext = createContext<PipelineContextType | undefined>(undefined);

// Define the props for the provider component
interface PipelineProviderProps {
  children: ReactNode;
}

export const PipelineProvider = ({ children }: PipelineProviderProps) => {
  const [pipelines, setPipelines] = useState<Pipeline[]>([]);
  const [selectedPipeline, setSelectedPipeline] = useState<Pipeline | null>(null);
  
  // Combine pipelineConfig and pipelineStatus into a single state object
  const [pipelineState, setPipelineState] = useState<PipelineState>({
    status: 'stopped',
    config: null,
  });

  const { events } = useEventStream();

  // This effect now orchestrates the entire startup sequence
  useEffect(() => {
    const initialize = async () => {
      try {
        // 1. Fetch the list of available pipelines
        const availablePipelines = await getPipelines();
        setPipelines(availablePipelines);

        // 2. Check if a pipeline is already running by fetching the current state
        const currentState = await getPipelineState();
        if (currentState && currentState.stages.length > 0) {
          console.log('[PipelineProvider] A pipeline is already running. Syncing state.');
          setPipelineState({ status: 'started', config: currentState });
          const runningPipeline = availablePipelines.find(p => p.id === currentState.id) || null;
          setSelectedPipeline(runningPipeline);
        } else {
          // 3. If no pipeline is running, start the 'default' one
          console.log('[PipelineProvider] No pipeline running. Starting default pipeline.');
          const defaultPipeline = availablePipelines.find(p => p.id === 'default');
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
  }, []); // Runs once on mount

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
      
      // The final state will be set by the EegDataContext when it receives the SourceReady event
      // For now, we can fetch the state to get a preliminary config
      const state = await getPipelineState();
      setPipelineState({ status: 'started', config: state });
      console.log(`Pipeline ${id} is running and state has been fetched.`);

    } catch (error) {
      console.error(`Failed to start pipeline ${id}:`, error);
      setPipelineState({ status: 'error', config: null });
    }
  }, [pipelines]);

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