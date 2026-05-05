import { Component, useEffect, useMemo, useRef, useState } from 'react'
import type { ErrorInfo, FormEvent, KeyboardEvent, ReactNode } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useCoreEvents, type EngineStateType } from './hooks/useCoreEvents'
import {
  type AppSettings,
  type CloudProvider,
  type ModelSize,
  type SttEngine,
  useSettingsStore,
} from './store/settingsStore'
import './App.css'

type View = 'home' | 'settings'
type SortMode = 'newest' | 'oldest'

interface TranscriptionEntry {
  id: number
  text: string
  created_at: string
  duration_secs: number
}

interface AppBoundaryProps {
  children: ReactNode
}

interface AppBoundaryState {
  error: Error | null
}

const MODEL_OPTIONS: Array<{
  value: ModelSize
  label: string
  installed: boolean
  speed: number
  accuracy: number
}> = [
  { value: 'tiny', label: 'Tiny', installed: true, speed: 92, accuracy: 38 },
  { value: 'base', label: 'Base', installed: true, speed: 62, accuracy: 58 },
  { value: 'small', label: 'Small', installed: true, speed: 58, accuracy: 82 },
  { value: 'medium', label: 'Medium', installed: false, speed: 24, accuracy: 68 },
  { value: 'large', label: 'Large', installed: false, speed: 8, accuracy: 92 },
]

const PROVIDERS: Array<{ value: CloudProvider; label: string }> = [
  { value: 'gladia', label: 'Gladia' },
  { value: 'openai', label: 'OpenAI Whisper API' },
  { value: 'groq', label: 'Groq Whisper' },
  { value: 'deepgram', label: 'Deepgram' },
]

const ENGINE_LABELS: Record<SttEngine, string> = {
  auto: 'Auto',
  local: 'Local',
  cloud: 'Cloud',
}

const STATE_LABELS: Record<EngineStateType, string> = {
  Idle: 'Idle',
  Recording: 'Recording',
  Transcribing: 'Transcribing',
  Cleaning: 'Cleaning',
  Inserting: 'Inserting',
  Error: 'Error',
}

class AppBoundary extends Component<AppBoundaryProps, AppBoundaryState> {
  state: AppBoundaryState = {
    error: null,
  }

  static getDerivedStateFromError(error: Error): AppBoundaryState {
    return { error }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('wisprflow failed to render', error, info)
  }

  render() {
    if (this.state.error) {
      return (
        <main className="app-fallback" role="alert">
          <div className="app-fallback-panel">
            <p>wisprflow</p>
            <h1>App could not load</h1>
            <span>{this.state.error.message}</span>
            <button type="button" onClick={() => window.location.reload()}>
              Reload
            </button>
          </div>
        </main>
      )
    }

    return this.props.children
  }
}

function App() {
  return (
    <AppBoundary>
      <WisprFlowApp />
    </AppBoundary>
  )
}

function WisprFlowApp() {
  const settingsStore = useSettingsStore()
  const { engineState, errorMessage } = useCoreEvents()
  const [view, setView] = useState<View>('home')
  const [search, setSearch] = useState('')
  const [sortMode, setSortMode] = useState<SortMode>('newest')
  const [history, setHistory] = useState<TranscriptionEntry[]>([])
  const [historyLoading, setHistoryLoading] = useState(true)
  const [historyError, setHistoryError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false

    async function loadHistory() {
      setHistoryLoading(true)
      setHistoryError(null)
      try {
        const entries = await invoke<TranscriptionEntry[]>('list_transcriptions', { limit: 100 })
        if (!cancelled) {
          setHistory(entries)
        }
      } catch (err) {
        if (!cancelled) {
          setHistoryError(err instanceof Error ? err.message : String(err))
        }
      } finally {
        if (!cancelled) {
          setHistoryLoading(false)
        }
      }
    }

    void loadHistory()

    const unlisten = listen<TranscriptionEntry>('transcription-created', (event) => {
      setHistory((current) => [event.payload, ...current.filter((item) => item.id !== event.payload.id)])
    })

    return () => {
      cancelled = true
      unlisten.then((fn) => fn())
    }
  }, [])

  const visibleHistory = useMemo(() => {
    const query = search.trim().toLowerCase()
    const filtered = query
      ? history.filter((entry) => entry.text.toLowerCase().includes(query))
      : history

    return [...filtered].sort((a, b) => {
      const aTime = new Date(a.created_at).getTime()
      const bTime = new Date(b.created_at).getTime()
      return sortMode === 'newest' ? bTime - aTime : aTime - bTime
    })
  }, [history, search, sortMode])

  return (
    <main className="desktop-shell">
      <Sidebar view={view} setView={setView} />

      <section className="app-surface">
        <TitleBar engineState={engineState} />

        {view === 'home' ? (
          <HomeScreen
            engineState={engineState}
            errorMessage={errorMessage}
            history={visibleHistory}
            historyError={historyError}
            historyLoading={historyLoading}
            search={search}
            setSearch={setSearch}
            settings={settingsStore.settings}
            sortMode={sortMode}
            setSortMode={setSortMode}
          />
        ) : (
          <SettingsScreen settingsStore={settingsStore} />
        )}
      </section>
    </main>
  )
}

function Sidebar({ view, setView }: { view: View; setView: (view: View) => void }) {
  return (
    <aside className="sidebar" data-tauri-drag-region>
      <TrafficLights />

      <nav className="nav-stack" aria-label="Main navigation">
        <button
          className={view === 'home' ? 'nav-item is-active' : 'nav-item'}
          type="button"
          onClick={() => setView('home')}
        >
          <Icon name="home" />
          <span>Home</span>
        </button>
        <button
          className={view === 'settings' ? 'nav-item is-active' : 'nav-item'}
          type="button"
          onClick={() => setView('settings')}
        >
          <Icon name="settings" />
          <span>Settings</span>
        </button>
      </nav>

      <div className="sidebar-footer">
        <button className="help-link" type="button">
          <Icon name="help" />
          <span>Help</span>
        </button>
        <div className="sidebar-meta">
          <span>v1.2.3</span>
          <button type="button" aria-label="Open settings" onClick={() => setView('settings')}>
            <Icon name="settings" />
          </button>
        </div>
      </div>
    </aside>
  )
}

function TrafficLights() {
  async function runWindowAction(action: 'close' | 'minimize' | 'toggleMaximize') {
    try {
      const appWindow = getCurrentWindow()
      if (action === 'close') {
        await appWindow.close()
      } else if (action === 'minimize') {
        await appWindow.minimize()
      } else {
        await appWindow.toggleMaximize()
      }
    } catch {
      if (action === 'close') {
        window.close()
      }
    }
  }

  return (
    <div className="traffic-lights" aria-label="Window controls">
      <button className="traffic traffic-close" type="button" aria-label="Close" onClick={() => void runWindowAction('close')} />
      <button className="traffic traffic-minimize" type="button" aria-label="Minimize" onClick={() => void runWindowAction('minimize')} />
      <button className="traffic traffic-open" type="button" aria-label="Open" onClick={() => void runWindowAction('toggleMaximize')} />
    </div>
  )
}

function TitleBar({ engineState }: { engineState: EngineStateType }) {
  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region>
        <img src="/app-icon.png" alt="" />
        <strong>wisprflow</strong>
      </div>
      <button className="state-picker" type="button">
        <span className={`state-dot state-dot--${engineState.toLowerCase()}`} />
        <span>{STATE_LABELS[engineState]}</span>
        <Icon name="chevron" />
      </button>
    </header>
  )
}

function HomeScreen({
  engineState,
  errorMessage,
  history,
  historyError,
  historyLoading,
  search,
  setSearch,
  settings,
  sortMode,
  setSortMode,
}: {
  engineState: EngineStateType
  errorMessage: string | null
  history: TranscriptionEntry[]
  historyError: string | null
  historyLoading: boolean
  search: string
  setSearch: (value: string) => void
  settings: AppSettings
  sortMode: SortMode
  setSortMode: (value: SortMode) => void
}) {
  const isWorking = engineState !== 'Idle' && engineState !== 'Error'

  return (
    <div className="screen home-screen">
      <div className="home-toolbar">
        <label className="search-field">
          <Icon name="search" />
          <input
            value={search}
            placeholder="Search transcriptions..."
            onChange={(event) => setSearch(event.target.value)}
          />
        </label>

        <label className="sort-field">
          <Icon name="sort" />
          <span>Sort:</span>
          <select value={sortMode} onChange={(event) => setSortMode(event.target.value as SortMode)}>
            <option value="newest">Newest</option>
            <option value="oldest">Oldest</option>
          </select>
        </label>
      </div>

      <section className="history-region" aria-busy={historyLoading || isWorking}>
        {isWorking ? (
          <LoadingState engineState={engineState} />
        ) : historyLoading ? (
          <LoadingState engineState="Transcribing" />
        ) : historyError ? (
          <MessageState title="Could not load transcriptions" detail={historyError} />
        ) : history.length > 0 ? (
          <div className="transcription-list" aria-label="Past transcriptions">
            {history.map((entry) => (
              <TranscriptionRow entry={entry} key={entry.id} />
            ))}
          </div>
        ) : (
          <EmptyState hotkey={settings.hotkey} search={search} />
        )}
      </section>

      {errorMessage && !isWorking && (
        <div className="runtime-alert" role="alert">
          <strong>Runtime error</strong>
          <span>{errorMessage}</span>
        </div>
      )}
    </div>
  )
}

function LoadingState({ engineState }: { engineState: EngineStateType }) {
  return (
    <div className="state-panel loading-panel">
      <div className="wave-loader" aria-hidden="true">
        <span />
        <span />
        <span />
        <span />
        <span />
      </div>
      <h2>{engineState === 'Recording' ? 'Listening...' : 'Preparing transcription...'}</h2>
      <p>{STATE_LABELS[engineState]} is active. Your history will reappear when this step finishes.</p>
    </div>
  )
}

function EmptyState({ hotkey, search }: { hotkey: string; search: string }) {
  const hasSearch = search.trim().length > 0

  return (
    <div className="state-panel empty-panel">
      <div className="empty-icon" aria-hidden="true">
        <Icon name="fileStack" />
      </div>
      <h2>{hasSearch ? 'No matching transcriptions' : 'No transcriptions yet'}</h2>
      <p>
        {hasSearch
          ? 'Try a different search term.'
          : 'Start recording to create your first transcription.'}
      </p>
      {!hasSearch && (
        <button className="listen-button" type="button">
          <Icon name="mic" />
          <span>Start Listening</span>
          <kbd>{displayHotkey(hotkey)}</kbd>
        </button>
      )}
    </div>
  )
}

function MessageState({ title, detail }: { title: string; detail: string }) {
  return (
    <div className="state-panel empty-panel">
      <div className="empty-icon" aria-hidden="true">
        <Icon name="file" />
      </div>
      <h2>{title}</h2>
      <p>{detail}</p>
    </div>
  )
}

function TranscriptionRow({ entry }: { entry: TranscriptionEntry }) {
  return (
    <article className="transcription-row">
      <Icon name="file" />
      <div className="transcription-copy">
        <h3>{entryTitle(entry.text)}</h3>
        <p>
          {formatDate(entry.created_at)}
          <span />
          {formatTime(entry.created_at)}
          <span />
          {formatDuration(entry.duration_secs)}
        </p>
      </div>
      <button type="button" aria-label="Play transcription">
        <Icon name="play" />
      </button>
      <button type="button" aria-label="More actions">
        <Icon name="more" />
      </button>
    </article>
  )
}

function SettingsScreen({ settingsStore }: { settingsStore: ReturnType<typeof useSettingsStore> }) {
  const {
    apiKeyStatus,
    deleteApiKey,
    error,
    isLoading,
    isSaving,
    refreshApiStatus,
    saveSettings,
    saveSettingsPatch,
    settings,
    storeApiKey,
  } = settingsStore
  const [apiKeyInput, setApiKeyInput] = useState('')
  const [notice, setNotice] = useState<string | null>(null)
  const [capturingHotkey, setCapturingHotkey] = useState(false)
  const hotkeyButtonRef = useRef<HTMLButtonElement | null>(null)

  useEffect(() => {
    void refreshApiStatus(settings.cloud_provider)
  }, [refreshApiStatus, settings.cloud_provider])

  async function savePatch(patch: Partial<AppSettings>, message?: string) {
    setNotice(null)
    await saveSettingsPatch(patch)
    if (message) {
      setNotice(message)
    }
  }

  async function onApiKeySubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setNotice(null)
    await storeApiKey(settings.cloud_provider, apiKeyInput)
    setApiKeyInput('')
    setNotice('API key saved.')
  }

  async function onDeleteApiKey() {
    setNotice(null)
    await deleteApiKey(settings.cloud_provider)
    setNotice('API key removed.')
  }

  function startCapture() {
    setCapturingHotkey(true)
    window.setTimeout(() => hotkeyButtonRef.current?.focus(), 0)
  }

  async function captureHotkey(event: KeyboardEvent<HTMLButtonElement>) {
    if (!capturingHotkey) {
      return
    }
    event.preventDefault()
    const nextHotkey = hotkeyFromEvent(event)
    if (!nextHotkey) {
      return
    }
    setCapturingHotkey(false)
    await savePatch({ hotkey: nextHotkey }, 'Hotkey updated.')
  }

  return (
    <div className="screen settings-screen" aria-busy={isLoading || isSaving}>
      {(error || notice) && (
        <div className={error ? 'settings-notice is-error' : 'settings-notice'} role={error ? 'alert' : 'status'}>
          {error ?? notice}
        </div>
      )}

      <section className="settings-section">
        <h2>1. Model Selection</h2>
        <div className="model-grid">
          {MODEL_OPTIONS.map((model) => (
            <button
              className={settings.stt_model_size === model.value ? 'model-card is-selected' : 'model-card'}
              type="button"
              key={model.value}
              disabled={isSaving}
              onClick={() => void savePatch({ stt_model_size: model.value }, `${model.label} model selected.`)}
            >
              <span className="model-check">
                {settings.stt_model_size === model.value && <Icon name="check" />}
              </span>
              <strong>{model.label}</strong>
              <Meter label="Speed" value={model.speed} />
              <Meter label="Accuracy" value={model.accuracy} />
              <small className={model.installed ? 'install-state is-installed' : 'install-state'}>
                {model.installed ? 'Installed' : 'Not Installed'}
              </small>
            </button>
          ))}
        </div>
        <p>Larger models provide higher accuracy but require more processing power and time.</p>
      </section>

      <section className="settings-section hotkey-section">
        <h2>2. Hotkey Configuration</h2>
        <div className="hotkey-layout">
          <div>
            <label>Global Transcription Hotkey</label>
            <button
              className={capturingHotkey ? 'hotkey-capture is-capturing' : 'hotkey-capture'}
              type="button"
              ref={hotkeyButtonRef}
              onClick={startCapture}
              onKeyDown={(event) => void captureHotkey(event)}
            >
              {capturingHotkey ? 'Press keys...' : displayHotkey(settings.hotkey)}
            </button>
            <span>Press the key combination to capture</span>
          </div>

          <div className="conflict-callout">
            <Icon name="warning" />
            <div>
              <strong>Potential conflict detected</strong>
              <span>This hotkey is used by another application.</span>
            </div>
          </div>

          <button
            className="reset-button"
            type="button"
            disabled={isSaving}
            onClick={() => void savePatch({ hotkey: 'Super+Space' }, 'Hotkey reset.')}
          >
            <Icon name="reset" />
            Reset to Default
          </button>
        </div>
      </section>

      <section className="settings-section cloud-section">
        <h2>3. Cloud Transcription</h2>
        <div className="cloud-grid">
          <label className="toggle-setting">
            <input
              type="checkbox"
              checked={settings.stt_engine !== 'local'}
              onChange={(event) =>
                void savePatch(
                  { stt_engine: event.target.checked ? 'auto' : 'local' },
                  event.target.checked ? 'Cloud transcription enabled.' : 'Local transcription only.',
                )
              }
            />
            <span />
            <div>
              <strong>Enable Cloud Transcription</strong>
              <small>Send audio to the cloud for transcription.</small>
            </div>
          </label>

          <label className="select-setting">
            <span>Provider</span>
            <select
              value={settings.cloud_provider}
              onChange={(event) =>
                void savePatch(
                  { cloud_provider: event.target.value as CloudProvider },
                  'Cloud provider updated.',
                )
              }
            >
              {PROVIDERS.map((provider) => (
                <option value={provider.value} key={provider.value}>
                  {provider.label}
                </option>
              ))}
            </select>
          </label>

          <form className="api-key-form" onSubmit={(event) => void onApiKeySubmit(event)}>
            <label>
              <span>API Key</span>
              <div className="password-field">
                <input
                  type="password"
                  value={apiKeyInput}
                  placeholder={apiKeyStatus?.configured ? 'Stored API key' : 'Paste API key'}
                  onChange={(event) => setApiKeyInput(event.target.value)}
                />
                <Icon name="eye" />
              </div>
            </label>
            <small>Your API key is stored securely and never transmitted to our servers.</small>
            <div className="api-actions">
              <button type="submit" disabled={!apiKeyInput.trim()}>
                Save Key
              </button>
              <button type="button" onClick={() => void onDeleteApiKey()}>
                Remove
              </button>
            </div>
          </form>
        </div>
      </section>

      <section className="settings-section general-section">
        <h2>4. General Settings</h2>
        <div className="general-grid">
          <label className="toggle-setting">
            <input
              type="checkbox"
              checked={settings.launch_at_login}
              onChange={(event) =>
                void savePatch({ launch_at_login: event.target.checked }, 'Launch preference updated.')
              }
            />
            <span />
            <div>
              <strong>Auto-start on login</strong>
              <small>Launch wisprflow automatically when you log in to your system.</small>
            </div>
          </label>

          <label className="select-setting">
            <span>Engine</span>
            <select
              value={settings.stt_engine}
              onChange={(event) =>
                void savePatch({ stt_engine: event.target.value as SttEngine }, 'Engine mode updated.')
              }
            >
              {(Object.keys(ENGINE_LABELS) as SttEngine[]).map((engine) => (
                <option value={engine} key={engine}>
                  {ENGINE_LABELS[engine]}
                </option>
              ))}
            </select>
          </label>

          <label className="select-setting">
            <span>Export Format</span>
            <select value="markdown" onChange={() => undefined}>
              <option value="markdown">Markdown (.md)</option>
              <option value="text">Plain Text (.txt)</option>
            </select>
          </label>
        </div>

        <div className="settings-actions">
          <button className="save-button" type="button" disabled={isSaving} onClick={() => void saveSettings()}>
            {isSaving ? 'Saving...' : 'Save Preferences'}
          </button>
        </div>
      </section>
    </div>
  )
}

function Meter({ label, value }: { label: string; value: number }) {
  return (
    <div className="meter">
      <span>{label}</span>
      <i>
        <b style={{ width: `${value}%` }} />
      </i>
    </div>
  )
}

function Icon({ name }: { name: string }) {
  return (
    <svg className={`icon icon-${name}`} viewBox="0 0 24 24" aria-hidden="true">
      {name === 'home' && <path d="M3.5 11.5 12 4l8.5 7.5M5.5 10v9h5v-5h3v5h5v-9" />}
      {name === 'settings' && <path d="M12 8.2a3.8 3.8 0 1 1 0 7.6 3.8 3.8 0 0 1 0-7.6Zm0-5.2 1.3 2.2 2.5.7 2.2-1.2 1.8 3.1-2.1 1.4.1 1.4 2 1.5-1.8 3.1-2.3-1-1.2.7-.3 2.6h-3.6l-.3-2.6-1.2-.7-2.3 1L4.9 12l2-1.5.1-1.4-2.1-1.4 1.8-3.1 2.2 1.2 2.5-.7L12 3Z" />}
      {name === 'help' && <path d="M9.1 9a3 3 0 1 1 4.9 2.3c-1.2.9-1.9 1.4-1.9 2.7M12 18h.01M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z" />}
      {name === 'search' && <path d="m20 20-4.5-4.5M10.8 18a7.2 7.2 0 1 1 0-14.4 7.2 7.2 0 0 1 0 14.4Z" />}
      {name === 'sort' && <path d="M4 7h10M4 12h7M4 17h4m10-9v10m0 0 3-3m-3 3-3-3" />}
      {name === 'file' && <path d="M7 3h7l4 4v14H7V3Zm7 0v5h5M9.5 13h5M9.5 17h5" />}
      {name === 'fileStack' && <path d="M7 3h7l4 4v12H7V3Zm7 0v5h5M10 14h4m-4 3h4M5 7H4v14h10v-1" />}
      {name === 'play' && <path d="m9 6 9 6-9 6V6Z" />}
      {name === 'more' && <path d="M5 12h.01M12 12h.01M19 12h.01" />}
      {name === 'mic' && <path d="M12 3a3 3 0 0 0-3 3v5a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3Zm-7 8a7 7 0 0 0 14 0M12 18v3m-4 0h8" />}
      {name === 'chevron' && <path d="m7 10 5 5 5-5" />}
      {name === 'check' && <path d="m5 12 4 4L19 6" />}
      {name === 'warning' && <path d="M12 4 3 20h18L12 4Zm0 5v5m0 3h.01" />}
      {name === 'reset' && <path d="M4 12a8 8 0 1 0 2.3-5.7L4 8m0 0V3m0 5h5" />}
      {name === 'eye' && <path d="M2.5 12s3.5-6 9.5-6 9.5 6 9.5 6-3.5 6-9.5 6-9.5-6-9.5-6Zm9.5 3a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z" />}
    </svg>
  )
}

function displayHotkey(hotkey: string) {
  return hotkey.replace('Super', 'Win').replace(/\+/g, ' + ')
}

function hotkeyFromEvent(event: KeyboardEvent<HTMLButtonElement>) {
  const key = normalizeEventKey(event.key)
  if (!key) {
    return null
  }

  const parts = [
    event.ctrlKey ? 'Ctrl' : null,
    event.altKey ? 'Alt' : null,
    event.shiftKey ? 'Shift' : null,
    event.metaKey ? 'Super' : null,
    key,
  ].filter(Boolean)

  return parts.join('+')
}

function normalizeEventKey(key: string) {
  if (['Control', 'Alt', 'Shift', 'Meta'].includes(key)) {
    return null
  }
  if (key === ' ') {
    return 'Space'
  }
  if (key.length === 1) {
    return key.toUpperCase()
  }
  return key
}

function entryTitle(text: string) {
  const trimmed = text.trim()
  if (trimmed.length <= 132) {
    return trimmed
  }
  return `${trimmed.slice(0, 129)}...`
}

function formatDate(value: string) {
  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  }).format(new Date(value))
}

function formatTime(value: string) {
  return new Intl.DateTimeFormat(undefined, {
    hour: 'numeric',
    minute: '2-digit',
  }).format(new Date(value))
}

function formatDuration(seconds: number) {
  const safeSeconds = Math.max(0, Math.round(seconds))
  const minutes = Math.floor(safeSeconds / 60)
  const remainder = safeSeconds % 60
  return `${minutes}m ${String(remainder).padStart(2, '0')}s`
}

export default App
