import { useEffect } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useCoreEvents, type EngineStateType } from '../hooks/useCoreEvents';
import './Pill.css';

const STATE_LABELS: Record<EngineStateType, string> = {
  Idle: 'Wispr',
  Recording: 'Listening',
  Transcribing: 'Processing',
  Cleaning: 'Cleaning',
  Inserting: 'Done',
  Error: 'Error',
};

function Pill() {
  const { engineState } = useCoreEvents();

  // Make the window click-through on mount
  useEffect(() => {
    const setupClickThrough = async () => {
      try {
        const currentWindow = getCurrentWindow();
        await currentWindow.setIgnoreCursorEvents(true);
      } catch (e) {
        console.warn('Failed to set click-through:', e);
      }
    };
    setupClickThrough();
  }, []);

  const stateClass = `pill--${engineState.toLowerCase()}`;

  return (
    <div className="pill-container">
      <div className={`pill ${stateClass}`}>
        <span className="pill-dot" />
        <span className="pill-label">{STATE_LABELS[engineState]}</span>
      </div>
    </div>
  );
}

export default Pill;
