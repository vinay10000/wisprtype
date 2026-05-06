import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';

export type EngineStateType =
  | 'Idle'
  | 'Listening'
  | 'Transcribing'
  | 'Refining'
  | 'Inserting'
  | 'Error';

export interface EngineStatePayload {
  state: EngineStateType;
  message?: string;
}

export function useCoreEvents() {
  const [engineState, setEngineState] = useState<EngineStateType>('Idle');
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  useEffect(() => {
    const unlisten = listen<EngineStatePayload>('engine-state', (event) => {
      const payload = event.payload;
      setEngineState(payload.state);

      if (payload.state === 'Error' && payload.message) {
        setErrorMessage(payload.message);
      } else {
        setErrorMessage(null);
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return { engineState, errorMessage };
}
