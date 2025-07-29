'use client';

import React, { createContext, useContext, useState, ReactNode, useEffect, useCallback } from 'react';
import { getPipelines, startPipeline as apiStartPipeline, getPipelineState } from '../utils/api';
import { SystemConfig } from '@/types/eeg';

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

  // Fetch available pipelines on mount
  useEffect(() => {
    const fetchPipelines = async () => {
      try {
        const availablePipelines = await getPipelines();
        setPipelines(availablePipelines);
      } catch (error) {
        console.error("Failed to fetch pipelines on mount:", error);
        setPipelineState({ status: 'error', config: null });
      }
    };
    fetchPipelines();
  }, []);

  const selectAndStartPipeline = useCallback(async (id: string) => {
    const pipelineToStart = pipelines.find(p => p.id === id);
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

      // After starting, fetch the full state
      const state = await getPipelineState();
      
      // Atomic update of both status and config
      setPipelineState({ status: 'started', config: state });
      
      console.log(`Pipeline ${id} is running and state has been fetched.`);
    } catch (error) {
      console.error(`Failed to start pipeline ${id}:`, error);
      setPipelineState({ status: 'error', config: null });
    }
  }, [pipelines]);

  const value = {
    pipelines,
    selectedPipeline,
    pipelineConfig: pipelineState.config,
    pipelineStatus: pipelineState.status,
    selectAndStartPipeline,
    pipelineState: pipelineState,
  };

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