'use client';

import { useCommandWebSocket } from '../context/CommandWebSocketContext';

export default function EegRecordingControls() {
  const {
    wsConnected,
    startRecording,
    stopRecording,
    recordingStatus,
    recordingFilePath,
  } = useCommandWebSocket();

  return (
    <div className="flex flex-col w-full">
      {/* Recording controls */}
      <div className="flex items-center justify-between mb-2">
        <div className="text-white font-medium">Recording Controls</div>
        <button
          onClick={recordingStatus.startsWith('Currently recording') ? stopRecording : startRecording}
          disabled={!wsConnected}
          className={`px-4 py-2 rounded-md flex items-center ${
            !wsConnected
              ? 'bg-gray-700 text-gray-500 cursor-not-allowed'
              : recordingStatus.startsWith('Currently recording')
                ? 'bg-red-600 hover:bg-red-700 text-white'
                : 'bg-green-600 hover:bg-green-700 text-white'
          }`}
        >
          {recordingStatus.startsWith('Currently recording') ? (
            <>
              <span className="inline-block w-2 h-2 rounded-full bg-white mr-2"></span>
              Stop Recording
            </>
          ) : (
            <>
              <span className="inline-block w-2 h-2 rounded-full bg-white mr-2"></span>
              Start Recording
            </>
          )}
        </button>
      </div>
      
      {/* Recording status indicator */}
      <div className={`p-3 rounded-md ${recordingStatus.startsWith('Currently recording') ? 'bg-red-900/30' : 'bg-gray-800'}`}>
        <div className="flex items-center">
          {recordingStatus.startsWith('Currently recording') ? (
            <span className="inline-block w-2 h-2 rounded-full bg-red-500 animate-pulse mr-2"></span>
          ) : (
            <span className="inline-block w-2 h-2 rounded-full bg-gray-500 mr-2"></span>
          )}
          <span className="text-white">{recordingStatus}</span>
        </div>
        
        {recordingFilePath && (
          <div className="text-gray-300 text-sm mt-1 truncate">
            File: {recordingFilePath}
          </div>
        )}
        
        <div className="text-gray-400 text-xs mt-2">
          Note: Configuration changes are blocked while recording is in progress.
        </div>
      </div>
    </div>
  );
}