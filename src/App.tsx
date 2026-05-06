import { Component, useCallback, useEffect, useMemo, useState } from 'react'
import type { ErrorInfo, ReactNode } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useCoreEvents, type EngineStateType } from './hooks/useCoreEvents'
import Settings from './pages/Settings'
import './pages/Settings.css'
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

const STATE_LABELS: Record<EngineStateType, string> = {
  Idle: 'Idle',
  Listening: 'Listening',
  Transcribing: 'Transcribing',
  Refining: 'Refining',
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
  const { engineState, errorMessage } = useCoreEvents()
  const [view, setView] = useState<View>('home')
  const [search, setSearch] = useState('')
  const [sortMode, setSortMode] = useState<SortMode>('newest')
  const [history, setHistory] = useState<TranscriptionEntry[]>([])
  const [historyLoading, setHistoryLoading] = useState(true)
  const [historyError, setHistoryError] = useState<string | null>(null)
  const [busyEntryId, setBusyEntryId] = useState<number | null>(null)

  const loadHistory = useCallback(async (query = search) => {
    setHistoryLoading(true)
    setHistoryError(null)
    try {
      const entries = await invoke<TranscriptionEntry[]>('list_transcriptions', {
        limit: 100,
        query: query.trim() || null,
      })
      setHistory(entries)
    } catch (err) {
      setHistoryError(err instanceof Error ? err.message : String(err))
    } finally {
      setHistoryLoading(false)
    }
  }, [search])

  useEffect(() => {
    let cancelled = false

    async function run() {
      setHistoryLoading(true)
      setHistoryError(null)
      try {
        const entries = await invoke<TranscriptionEntry[]>('list_transcriptions', {
          limit: 100,
          query: search.trim() || null,
        })
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

    void run()

    const unlisten = listen<TranscriptionEntry>('transcription-created', (event) => {
      const matchesSearch =
        search.trim().length === 0 || event.payload.text.toLowerCase().includes(search.trim().toLowerCase())

      setHistory((current) => {
        const next = [event.payload, ...current.filter((item) => item.id !== event.payload.id)]
        return matchesSearch ? sortEntries(next, sortMode) : current
      })
    })

    return () => {
      cancelled = true
      unlisten.then((fn) => fn())
    }
  }, [search, sortMode])

  const visibleHistory = useMemo(() => sortEntries(history, sortMode), [history, sortMode])

  async function copyEntry(entry: TranscriptionEntry) {
    await navigator.clipboard.writeText(entry.text)
  }

  async function reinsertEntry(entry: TranscriptionEntry) {
    setBusyEntryId(entry.id)
    setHistoryError(null)
    try {
      await invoke<TranscriptionEntry>('reinsert_transcription', { id: entry.id })
    } catch (err) {
      setHistoryError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusyEntryId(null)
    }
  }

  async function deleteEntry(entry: TranscriptionEntry) {
    setBusyEntryId(entry.id)
    setHistoryError(null)
    try {
      await invoke('delete_transcription', { id: entry.id })
      setHistory((current) => current.filter((item) => item.id !== entry.id))
    } catch (err) {
      setHistoryError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusyEntryId(null)
    }
  }

  return (
    <main className="desktop-shell">
      <Sidebar view={view} setView={setView} />

      <section className="app-surface">
        <TitleBar engineState={engineState} />

        {view === 'home' ? (
          <HomeScreen
            busyEntryId={busyEntryId}
            engineState={engineState}
            errorMessage={errorMessage}
            history={visibleHistory}
            historyError={historyError}
            historyLoading={historyLoading}
            onCopy={copyEntry}
            onDelete={deleteEntry}
            onRefresh={loadHistory}
            onReinsert={reinsertEntry}
            search={search}
            setSearch={setSearch}
            sortMode={sortMode}
            setSortMode={setSortMode}
          />
        ) : (
          <div className="screen settings-screen-shell">
            <Settings />
          </div>
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
        <div className="help-copy">
          <strong>Private dictation</strong>
          <span>Local-first voice typing with cloud fallback when you choose it.</span>
        </div>
        <div className="sidebar-meta">
          <span>Desktop preview</span>
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
      <button className="traffic traffic-open" type="button" aria-label="Toggle maximize" onClick={() => void runWindowAction('toggleMaximize')} />
    </div>
  )
}

function TitleBar({ engineState }: { engineState: EngineStateType }) {
  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region>
        <img src="/app-icon.png" alt="" />
        <div>
          <strong>wisprflow</strong>
          <p>System-wide AI dictation for Windows</p>
        </div>
      </div>
      <div className="state-picker" aria-live="polite">
        <span className={`state-dot state-dot--${engineState.toLowerCase()}`} />
        <span>{STATE_LABELS[engineState]}</span>
      </div>
    </header>
  )
}

function HomeScreen({
  busyEntryId,
  engineState,
  errorMessage,
  history,
  historyError,
  historyLoading,
  onCopy,
  onDelete,
  onRefresh,
  onReinsert,
  search,
  setSearch,
  sortMode,
  setSortMode,
}: {
  busyEntryId: number | null
  engineState: EngineStateType
  errorMessage: string | null
  history: TranscriptionEntry[]
  historyError: string | null
  historyLoading: boolean
  onCopy: (entry: TranscriptionEntry) => Promise<void>
  onDelete: (entry: TranscriptionEntry) => Promise<void>
  onRefresh: (query?: string) => Promise<void>
  onReinsert: (entry: TranscriptionEntry) => Promise<void>
  search: string
  setSearch: (value: string) => void
  sortMode: SortMode
  setSortMode: (value: SortMode) => void
}) {
  const isWorking = engineState !== 'Idle' && engineState !== 'Error'

  return (
    <div className="screen home-screen">
      <section className="home-hero">
        <div>
          <p className="home-kicker">Transcript history</p>
          <h1>Search, reuse, and reinsert your voice notes.</h1>
          <p>
            Every successful dictation stays local in SQLite so you can find it fast, copy it back out,
            or insert it again into the app you are working in.
          </p>
        </div>
        <div className="home-hero-card" aria-live="polite">
          <strong>{STATE_LABELS[engineState]}</strong>
          <span>
            {isWorking
              ? 'The dictation pipeline is active right now.'
              : 'Hold your global hotkey anywhere in Windows to start dictating.'}
          </span>
        </div>
      </section>

      <div className="home-toolbar">
        <label className="search-field">
          <Icon name="search" />
          <input
            value={search}
            placeholder="Search transcript history"
            onChange={(event) => setSearch(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                void onRefresh(event.currentTarget.value)
              }
            }}
          />
        </label>

        <label className="sort-field">
          <Icon name="sort" />
          <span>Sort</span>
          <select value={sortMode} onChange={(event) => setSortMode(event.target.value as SortMode)}>
            <option value="newest">Newest</option>
            <option value="oldest">Oldest</option>
          </select>
        </label>

        <button className="refresh-button" type="button" onClick={() => void onRefresh()}>
          Refresh
        </button>
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
              <TranscriptionCard
                busy={busyEntryId === entry.id}
                entry={entry}
                key={entry.id}
                onCopy={onCopy}
                onDelete={onDelete}
                onReinsert={onReinsert}
              />
            ))}
          </div>
        ) : (
          <EmptyState search={search} />
        )}
      </section>

      {(errorMessage || historyError) && !isWorking && (
        <div className="runtime-alert" role="alert">
          <strong>{errorMessage ? 'Runtime error' : 'History error'}</strong>
          <span>{errorMessage ?? historyError}</span>
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
      <h2>{engineState === 'Listening' ? 'Listening...' : `${STATE_LABELS[engineState]}...`}</h2>
      <p>{STATE_LABELS[engineState]} is active. Your transcript history will reappear when the step finishes.</p>
    </div>
  )
}

function EmptyState({ search }: { search: string }) {
  const hasSearch = search.trim().length > 0

  return (
    <div className="state-panel empty-panel">
      <div className="empty-icon" aria-hidden="true">
        <Icon name="fileStack" />
      </div>
      <h2>{hasSearch ? 'No matching transcriptions' : 'No transcripts yet'}</h2>
      <p>
        {hasSearch
          ? 'Try a different term or clear your search.'
          : 'Your completed dictations will appear here after successful insertion.'}
      </p>
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

function TranscriptionCard({
  busy,
  entry,
  onCopy,
  onDelete,
  onReinsert,
}: {
  busy: boolean
  entry: TranscriptionEntry
  onCopy: (entry: TranscriptionEntry) => Promise<void>
  onDelete: (entry: TranscriptionEntry) => Promise<void>
  onReinsert: (entry: TranscriptionEntry) => Promise<void>
}) {
  return (
    <article className="transcription-card">
      <div className="transcription-card-head">
        <div className="transcription-meta">
          <span>{formatDate(entry.created_at)}</span>
          <span>{formatTime(entry.created_at)}</span>
          <span>{formatDuration(entry.duration_secs)}</span>
        </div>
        <Icon name="file" />
      </div>

      <h3>{entryTitle(entry.text)}</h3>
      <p className="transcription-body">{entry.text}</p>

      <div className="transcription-actions">
        <button type="button" onClick={() => void onCopy(entry)} disabled={busy}>
          Copy
        </button>
        <button type="button" onClick={() => void onReinsert(entry)} disabled={busy}>
          {busy ? 'Working...' : 'Reinsert'}
        </button>
        <button type="button" onClick={() => void onDelete(entry)} disabled={busy}>
          Delete
        </button>
      </div>
    </article>
  )
}

function Icon({ name }: { name: string }) {
  return (
    <svg className={`icon icon-${name}`} viewBox="0 0 24 24" aria-hidden="true">
      {name === 'home' && <path d="M3.5 11.5 12 4l8.5 7.5M5.5 10v9h5v-5h3v5h5v-9" />}
      {name === 'settings' && <path d="M12 8.2a3.8 3.8 0 1 1 0 7.6 3.8 3.8 0 0 1 0-7.6Zm0-5.2 1.3 2.2 2.5.7 2.2-1.2 1.8 3.1-2.1 1.4.1 1.4 2 1.5-1.8 3.1-2.3-1-1.2.7-.3 2.6h-3.6l-.3-2.6-1.2-.7-2.3 1L4.9 12l2-1.5.1-1.4-2.1-1.4 1.8-3.1 2.2 1.2 2.5-.7L12 3Z" />}
      {name === 'search' && <path d="m20 20-4.5-4.5M10.8 18a7.2 7.2 0 1 1 0-14.4 7.2 7.2 0 0 1 0 14.4Z" />}
      {name === 'sort' && <path d="M4 7h10M4 12h7M4 17h4m10-9v10m0 0 3-3m-3 3-3-3" />}
      {name === 'file' && <path d="M7 3h7l4 4v14H7V3Zm7 0v5h5M9.5 13h5M9.5 17h5" />}
      {name === 'fileStack' && <path d="M7 3h7l4 4v12H7V3Zm7 0v5h5M10 14h4m-4 3h4M5 7H4v14h10v-1" />}
      {name === 'chevron' && <path d="m7 10 5 5 5-5" />}
    </svg>
  )
}

function entryTitle(text: string) {
  const trimmed = text.trim()
  if (trimmed.length <= 88) {
    return trimmed
  }
  return `${trimmed.slice(0, 85)}...`
}

function sortEntries(entries: TranscriptionEntry[], sortMode: SortMode) {
  return [...entries].sort((a, b) => {
    const aTime = new Date(a.created_at).getTime()
    const bTime = new Date(b.created_at).getTime()
    return sortMode === 'newest' ? bTime - aTime : aTime - bTime
  })
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

export function displayHotkey(hotkey: string) {
  return hotkey.replace('Super', 'Win').replace(/\+/g, ' + ')
}

export default App
