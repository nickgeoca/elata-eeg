// This file defines the shared data structures between the backend and frontend.

export interface StageConfig {
  name: string;
  type: string;
  params: any;
  inputs: string[];
  outputs: string[];
}

export interface SystemConfig {
  stages: StageConfig[];
}

export interface EegSample {
  value: number;
  timestamp: bigint;
  channelIndex: number;
}

export interface SampleChunk {
  config: {
    channelCount: number;
    sampleRate: number;
  };
  samples: EegSample[];
}