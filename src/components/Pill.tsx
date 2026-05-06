import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useCoreEvents, type EngineStateType } from '../hooks/useCoreEvents';
import './Pill.css';

const STATE_LABELS: Record<EngineStateType, string> = {
  Idle: 'wisprflow ready',
  Listening: 'Listening',
  Transcribing: 'Transcribing',
  Refining: 'Refining',
  Inserting: 'Inserting',
  Error: 'Error',
};

function Pill() {
  const { engineState } = useCoreEvents();
  const [pillStyle, setPillStyle] = useState<'aurora' | 'minimal'>('aurora');

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

  useEffect(() => {
    const unlisten = listen<{ pill_style?: 'aurora' | 'minimal' }>(
      'pill-settings',
      (event) => {
        setPillStyle(event.payload.pill_style ?? 'aurora');
      },
    );

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const stateClass = `pill--${engineState.toLowerCase()}`;

  return (
    <div className="pill-container">
      <div className={`pill pill-style--${pillStyle} ${stateClass}`} aria-label={STATE_LABELS[engineState]}>
        <span className="pill-visual" aria-hidden="true">
          <span className="pill-wave pill-wave--one" />
          <span className="pill-wave pill-wave--two" />
          <span className="pill-wave pill-wave--three" />
          <span className="pill-core" />
          <span className="pill-bars" />
          <span className="pill-dots" />
          <span className="pill-warning" />
        </span>
      </div>
    </div>
  );
}

export default Pill;
