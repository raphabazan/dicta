interface RecordingWidgetProps {
  isRecording: boolean;
  onCancel: () => void;
  onStop: () => void;
}

function RecordingWidget({ isRecording, onCancel, onStop }: RecordingWidgetProps) {
  if (!isRecording) return null;

  return (
    <div className="fixed top-4 right-4 z-50">
      <div
        className="flex items-center gap-2 bg-gray-900/95 border border-red-500/50 rounded-full px-4 py-2 shadow-2xl backdrop-blur-sm"
        style={{ width: '125px', height: '40px' }}
      >
        {/* Recording indicator */}
        <div className="w-2 h-2 bg-red-500 rounded-full animate-pulse"></div>

        {/* Cancel button */}
        <button
          onClick={onCancel}
          className="w-6 h-6 flex items-center justify-center hover:bg-gray-700/50 rounded-full transition-colors"
          title="Cancel"
        >
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" xmlns="http://www.w3.org/2000/svg">
            <path d="M1 1L11 11M1 11L11 1" stroke="white" strokeWidth="1.5" strokeLinecap="round"/>
          </svg>
        </button>

        {/* Stop/Send button */}
        <button
          onClick={onStop}
          className="w-6 h-6 flex items-center justify-center bg-red-500 hover:bg-red-600 rounded-full transition-colors"
          title="Stop & Send"
        >
          <div className="w-2 h-2 bg-white rounded-sm"></div>
        </button>
      </div>
    </div>
  );
}

export default RecordingWidget;
