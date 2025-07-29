'use client';

import { createContext, useState, useContext, useMemo, useCallback } from 'react';
import { startPipeline, stopPipeline, sendControlCommand } from '../utils/api';

interface CommandContextType {
  startRecording: () => Promise<void>;
  stopRecording: () => Promise<void>;
  sendPowerlineFilterCommand: (value: number | null) => Promise<void>;
  recordingStatus: string;
  recordingFilePath: string | null;
  isStartRecordingPending: boolean;
  recordingError: string | null;
}

const CommandContext = createContext<CommandContextType | undefined>(
  undefined
);

export const CommandProvider = ({
  children,
}: {
  children: React.ReactNode;
}) => {
  const [recordingStatus, setRecordingStatus] = useState('Not recording');
  const [recordingFilePath, setRecordingFilePath] = useState<string | null>(null);
  const [isStartRecordingPending, setIsStartRecordingPending] = useState(false);
  const [recordingError, setRecordingError] = useState<string | null>(null);

  const startRecording = useCallback(async () => {
    if (isStartRecordingPending) return;

    console.log('[Command] Sending "start" command...');
    setIsStartRecordingPending(true);
    setRecordingError(null);

    try {
      await startPipeline('default');
      console.log('[Command] "start" command issued successfully.');
      setRecordingStatus('Recording started...');
    } catch (error: any) {
      console.error('[Command] Failed to start recording:', error);
      setRecordingError(error.message || 'Failed to start recording.');
      setRecordingStatus('Error');
    } finally {
      setIsStartRecordingPending(false);
    }
  }, [isStartRecordingPending]);

  const stopRecording = useCallback(async () => {
    console.log('[Command] Sending "stop" command...');
    setRecordingError(null);

    try {
      await stopPipeline();
      console.log('[Command] "stop" command issued successfully.');
      setRecordingStatus('Recording stopped.');
      setRecordingFilePath(null);
    } catch (error: any) {
      console.error('[Command] Failed to stop recording:', error);
      setRecordingError(error.message || 'Failed to stop recording.');
    }
  }, []);

  const sendPowerlineFilterCommand = useCallback(async (value: number | null) => {
    console.log(`[Command] Sending "set_powerline_filter" command with value: ${value}`);
    
    try {
      const commandPayload = {
        command: 'SetParameter',
        stage_id: 'notch_filter_stage', 
        parameter_id: 'frequency',
        value: value,
      };
      await sendControlCommand(commandPayload);
      console.log('[Command] "set_powerline_filter" command sent successfully.');
    } catch (error: any) {
      console.error('[Command] Failed to send powerline filter command:', error);
    }
  }, []);

  const value = useMemo(() => ({
    startRecording,
    stopRecording,
    sendPowerlineFilterCommand,
    recordingStatus,
    recordingFilePath,
    isStartRecordingPending,
    recordingError,
  }), [startRecording, stopRecording, sendPowerlineFilterCommand, recordingStatus, recordingFilePath, isStartRecordingPending, recordingError]);

  return (
    <CommandContext.Provider value={value}>
      {children}
    </CommandContext.Provider>
  );
};

export const useCommand = () => {
  const context = useContext(CommandContext);
  if (!context) {
    throw new Error(
      'useCommand must be used within a CommandProvider'
    );
  }
  return context;
};