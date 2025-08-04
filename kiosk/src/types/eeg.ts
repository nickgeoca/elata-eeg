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

export interface SensorMeta {
  sensor_id: number;
  meta_rev: number;
  source_type: string;
  v_ref: number;
  adc_bits: number;
  gain: number;
  sample_rate: number;
  offset_code: number;
  is_twos_complement: boolean;
  channel_names: string[];
}

export interface MetaUpdateMsg {
  message_type: 'meta_update';
  topic: string;
  meta: SensorMeta;
}

export interface DataPacketHeader {
  message_type: 'data_packet';
  topic: string;
  ts_ns: number;
  batch_size: number;
  num_channels: number;
  packet_type: 'Voltage' | 'RawI32';
}

export interface SampleChunk {
  meta: SensorMeta;
  samples: Float32Array; // Now a direct Float32Array for performance
  timestamp: number; // The timestamp of the first sample in the chunk
}