import { useEffect, useMemo, useState } from 'react'
import type { FormEvent } from 'react'
import { useCoreEvents, type EngineStateType } from '../hooks/useCoreEvents'
import './Settings.css'
import {
  type CloudProvider,
  type ModelSize,
  type PillStyle,
  type SttEngine,
  type ValidationCheck,
  useSettingsStore,
} from '../store/settingsStore'

const MODEL_OPTIONS: Array<{ value: ModelSize; label: string; detail: string }> = [
  { value: 'tiny', label: 'Tiny', detail: 'Lowest latency' },
  { value: 'base', label: 'Base', detail: 'Daily balance' },
  { value: 'small', label: 'Small', detail: 'Sharper phrasing' },
  { value: 'medium', label: 'Medium', detail: 'Higher fidelity' },
  { value: 'large', label: 'Large', detail: 'Best accuracy' },
]

const ENGINE_OPTIONS: Array<{ value: SttEngine; label: string; detail: string }> = [
  { value: 'auto', label: 'Auto', detail: 'Local first, cloud fallback' },
  { value: 'local', label: 'Local', detail: 'Private on-device STT' },
  { value: 'cloud', label: 'Cloud', detail: 'Provider transcription' },
]

const PROVIDER_OPTIONS: Array<{ value: CloudProvider; label: string }> = [
  { value: 'gladia', label: 'Gladia' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'groq', label: 'Groq' },
  { value: 'deepgram', label: 'Deepgram' },
]

const PILL_STYLE_OPTIONS: Array<{ value: PillStyle; label: string; detail: string }> = [
  { value: 'aurora', label: 'Aurora', detail: 'Luminous, ambient overlay' },
  { value: 'minimal', label: 'Minimal', detail: 'Quiet compact indicator' },
]

const MANUAL_QA_CHECKS = [
  'Validate the hold-to-talk flow in Notepad, Chrome, and VS Code.',
  'Confirm the Aurora Pill tracks listening, transcribing, refining, and inserting.',
  'Run one dictation with the local engine and one with the selected cloud provider.',
  'Add a dictionary term, speak it, and confirm the custom spelling survives.',
  'Review the validation report and verify the app data directory stays free of audio files.',
  'Exercise failure modes: microphone denied, invalid API key, and a conflicting hotkey.',
]

const PIPELINE_STEPS: EngineStateType[] = [
  'Idle',
  'Listening',
  'Transcribing',
  'Refining',
  'Inserting',
]

const STATE_META: Record<
  EngineStateType,
  { label: string; detail: string; tone: 'ready' | 'live' | 'work' | 'success' | 'error' }
> = {
  Idle: {
    label: 'Ready to listen',
    detail: 'Hold your hotkey anywhere in Windows to start a fresh dictation.',
    tone: 'ready',
  },
  Listening: {
    label: 'Capturing your voice',
    detail: 'wisprflow is listening live and keeping audio in memory until release.',
    tone: 'live',
  },
  Transcribing: {
    label: 'Turning speech into text',
    detail: 'The active engine is decoding speech and preparing the first transcript.',
    tone: 'work',
  },
  Refining: {
    label: 'Polishing the draft',
    detail: 'Filler words, punctuation, and casing are being refined before insert.',
    tone: 'work',
  },
  Inserting: {
    label: 'Pasting into the focused app',
    detail: 'wisprflow is injecting the final text back into your active workflow.',
    tone: 'success',
  },
  Error: {
    label: 'Needs attention',
    detail: 'A runtime problem interrupted dictation. Review the message and recover cleanly.',
    tone: 'error',
  },
}

function statusCounts(checks: ValidationCheck[] | undefined) {
  return (checks ?? []).reduce(
    (counts, check) => {
      counts[check.status] += 1
      return counts
    },
    { pass: 0, warn: 0, fail: 0 },
  )
}

export default function Settings() {
  const {
    addDictionaryTerm,
    apiKeyStatus,
    deleteApiKey,
    error,
    isLoading,
    isSaving,
    isValidating,
    refreshApiStatus,
    removeDictionaryTerm,
    runValidationChecks,
    saveSettings,
    settings,
    setError,
    storeApiKey,
    terms,
    updateSettings,
    validationReport,
  } = useSettingsStore()
  const { engineState, errorMessage } = useCoreEvents()

  const [termInput, setTermInput] = useState('')
  const [apiKeyInput, setApiKeyInput] = useState('')
  const [notice, setNotice] = useState<string | null>(null)

  useEffect(() => {
    void refreshApiStatus(settings.cloud_provider)
  }, [refreshApiStatus, settings.cloud_provider])

  const currentState = STATE_META[engineState]
  const selectedModel = MODEL_OPTIONS.find((option) => option.value === settings.stt_model_size)
  const selectedEngine = ENGINE_OPTIONS.find((option) => option.value === settings.stt_engine)
  const selectedProvider = PROVIDER_OPTIONS.find(
    (option) => option.value === settings.cloud_provider,
  )
  const validationSummary = useMemo(
    () => statusCounts(validationReport?.checks),
    [validationReport?.checks],
  )

  async function onSaveSettings() {
    setNotice(null)
    await saveSettings()
    setNotice('Preferences saved.')
  }

  async function onAddTerm(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setNotice(null)
    await addDictionaryTerm(termInput)
    setTermInput('')
    setNotice('Dictionary updated.')
  }

  async function onSaveApiKey(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setNotice(null)
    await storeApiKey(settings.cloud_provider, apiKeyInput)
    setApiKeyInput('')
    setNotice('API key stored in Windows Credential Manager.')
  }

  async function onDeleteApiKey() {
    setNotice(null)
    await deleteApiKey(settings.cloud_provider)
    setNotice('API key removed.')
  }

  async function onRunValidationChecks() {
    setNotice(null)
    await runValidationChecks()
    setNotice('Runtime checks refreshed.')
  }

  return (
    <main className="control-shell">
      <div className="control-backdrop" aria-hidden="true" />

      <section className="control-main" aria-busy={isLoading}>
        <header className="hero-panel">
          <div className="hero-copy">
            <p className="section-kicker">Voice to text for Windows</p>
            <h1>Speak. Release. Keep typing.</h1>
            <p className="hero-text">
              wisprflow gives you a system-wide dictation loop with a hold-to-talk hotkey, local
              or cloud transcription, cleanup, and direct text insertion back into the app you are
              already using.
            </p>

            <div className="hero-actions">
              <button
                className="primary-action"
                type="button"
                disabled={isLoading || isSaving}
                onClick={() => void onSaveSettings()}
              >
                {isSaving ? 'Saving...' : 'Save preferences'}
              </button>
              <button
                className="secondary-action"
                type="button"
                disabled={isValidating}
                onClick={() => void onRunValidationChecks()}
              >
                {isValidating ? 'Checking...' : 'Run runtime checks'}
              </button>
            </div>
          </div>

          <aside className="hero-status-card">
            <div className={`live-state live-state--${currentState.tone}`}>
              <div className="live-state-head">
                <span className="live-orb" />
                <span className="live-caption">Live pipeline</span>
              </div>
              <strong>{currentState.label}</strong>
              <p>{errorMessage ?? currentState.detail}</p>
            </div>

            <div className="hero-metrics" aria-label="Current configuration">
              <MetricCard label="Hotkey" value={settings.hotkey} hint="Hold to talk" />
              <MetricCard
                label="Engine"
                value={selectedEngine?.label ?? settings.stt_engine}
                hint={selectedModel?.label ?? settings.stt_model_size}
              />
              <MetricCard
                label="Cloud key"
                value={apiKeyStatus?.configured ? 'Stored' : 'Missing'}
                hint={selectedProvider?.label ?? settings.cloud_provider}
              />
              <MetricCard
                label="Dictionary"
                value={terms.length === 0 ? 'Empty' : `${terms.length} terms`}
                hint="Biases domain language"
              />
            </div>
          </aside>
        </header>

        <section className="pipeline-panel">
          <div className="panel-heading">
            <div>
              <p className="section-kicker">Dictation loop</p>
              <h2>What the app is doing right now</h2>
            </div>
            <p className="panel-note">
              From the first keypress to final text injection, the control center mirrors the same
              states shown by the floating Aurora Pill.
            </p>
          </div>

          <div className="pipeline-strip" aria-label="Dictation pipeline states">
            {PIPELINE_STEPS.map((step, index) => {
              const isActive = step === engineState
              const isComplete = PIPELINE_STEPS.indexOf(engineState) > index && engineState !== 'Error'

              return (
                <div
                  key={step}
                  className={`pipeline-step${isActive ? ' is-active' : ''}${isComplete ? ' is-complete' : ''}`}
                >
                  <span className="pipeline-index">{String(index + 1).padStart(2, '0')}</span>
                  <strong>{STATE_META[step].label}</strong>
                  <p>{STATE_META[step].detail}</p>
                </div>
              )
            })}
          </div>
        </section>

        {(error || notice || errorMessage) && (
          <section className="notice-stack" aria-live="polite">
            {error && (
              <div className="notice notice-error" role="alert">
                <div>
                  <strong>Settings error</strong>
                  <p>{error}</p>
                </div>
                <button type="button" onClick={() => setError(null)} aria-label="Dismiss error">
                  Dismiss
                </button>
              </div>
            )}

            {errorMessage && (
              <div className="notice notice-error" role="alert">
                <div>
                  <strong>Runtime error</strong>
                  <p>{errorMessage}</p>
                </div>
              </div>
            )}

            {notice && (
              <div className="notice" role="status">
                <div>
                  <strong>Updated</strong>
                  <p>{notice}</p>
                </div>
              </div>
            )}
          </section>
        )}

        <section className="workspace-grid">
          <article className="control-card control-card--wide">
            <div className="card-header">
              <div>
                <p className="section-kicker">Speech engine</p>
                <h3>Recognition path</h3>
              </div>
              <span className="card-tag">
                {selectedEngine?.label ?? settings.stt_engine} /{' '}
                {selectedModel?.label ?? settings.stt_model_size}
              </span>
            </div>

            <fieldset>
              <legend>Engine mode</legend>
              <div className="choice-grid choice-grid--triple">
                {ENGINE_OPTIONS.map((option) => (
                  <button
                    type="button"
                    className={settings.stt_engine === option.value ? 'is-selected' : ''}
                    key={option.value}
                    onClick={() => updateSettings({ stt_engine: option.value })}
                  >
                    <strong>{option.label}</strong>
                    <span>{option.detail}</span>
                  </button>
                ))}
              </div>
            </fieldset>

            <fieldset>
              <legend>Model size</legend>
              <div className="choice-grid choice-grid--five">
                {MODEL_OPTIONS.map((option) => (
                  <button
                    type="button"
                    className={settings.stt_model_size === option.value ? 'is-selected' : ''}
                    key={option.value}
                    onClick={() => updateSettings({ stt_model_size: option.value })}
                  >
                    <strong>{option.label}</strong>
                    <span>{option.detail}</span>
                  </button>
                ))}
              </div>
            </fieldset>
          </article>

          <article className="control-card">
            <div className="card-header">
              <div>
                <p className="section-kicker">Controls</p>
                <h3>Hotkey and overlay</h3>
              </div>
            </div>

            <div className="field-stack">
              <label className="input-block">
                <span>Hold-to-talk hotkey</span>
                <input
                  value={settings.hotkey}
                  placeholder="Super+Space"
                  onChange={(event) => updateSettings({ hotkey: event.target.value })}
                />
              </label>

              <label className="input-block">
                <span>Pill style</span>
                <select
                  value={settings.pill_style}
                  onChange={(event) =>
                    updateSettings({ pill_style: event.target.value as PillStyle })
                  }
                >
                  {PILL_STYLE_OPTIONS.map((option) => (
                    <option value={option.value} key={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </label>

              <div className="micro-list">
                {PILL_STYLE_OPTIONS.map((option) => (
                  <div className="micro-list-row" key={option.value}>
                    <strong>{option.label}</strong>
                    <span>{option.detail}</span>
                  </div>
                ))}
              </div>

              <label className="toggle-row">
                <input
                  type="checkbox"
                  checked={settings.pill_visible}
                  onChange={(event) => updateSettings({ pill_visible: event.target.checked })}
                />
                <div>
                  <strong>Show Aurora Pill</strong>
                  <span>Keep the floating state indicator visible above other apps.</span>
                </div>
              </label>

              <label className="toggle-row">
                <input
                  type="checkbox"
                  checked={settings.launch_at_login}
                  onChange={(event) => updateSettings({ launch_at_login: event.target.checked })}
                />
                <div>
                  <strong>Launch at login</strong>
                  <span>Start wisprflow with Windows so dictation is always ready.</span>
                </div>
              </label>
            </div>
          </article>

          <article className="control-card">
            <div className="card-header">
              <div>
                <p className="section-kicker">Cloud fallback</p>
                <h3>Provider access</h3>
              </div>
              <span
                className={`status-chip${apiKeyStatus?.configured ? ' status-chip--good' : ''}`}
              >
                {apiKeyStatus?.configured ? 'Key stored' : 'Needs key'}
              </span>
            </div>

            <div className="field-stack">
              <label className="input-block">
                <span>Provider</span>
                <select
                  value={settings.cloud_provider}
                  onChange={(event) => {
                    setApiKeyInput('')
                    updateSettings({ cloud_provider: event.target.value as CloudProvider })
                  }}
                >
                  {PROVIDER_OPTIONS.map((option) => (
                    <option value={option.value} key={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </label>

              <form className="field-stack" onSubmit={(event) => void onSaveApiKey(event)}>
                <label className="input-block">
                  <span>
                    API key
                    <small>
                      {apiKeyStatus?.configured
                        ? 'Stored securely in Windows Credential Manager'
                        : 'No key stored yet'}
                    </small>
                  </span>
                  <input
                    type="password"
                    value={apiKeyInput}
                    placeholder="Paste provider key"
                    onChange={(event) => setApiKeyInput(event.target.value)}
                  />
                </label>

                <div className="inline-actions">
                  <button type="submit" disabled={!apiKeyInput.trim()}>
                    Save key
                  </button>
                  <button type="button" onClick={() => void onDeleteApiKey()}>
                    Delete key
                  </button>
                </div>
              </form>
            </div>
          </article>

          <article className="control-card control-card--wide">
            <div className="card-header">
              <div>
                <p className="section-kicker">Custom dictionary</p>
                <h3>Vocabulary biasing</h3>
              </div>
              <span className="card-tag">
                {terms.length === 0 ? 'No terms yet' : `${terms.length} tracked`}
              </span>
            </div>

            <p className="card-copy">
              Feed wisprflow the acronyms, product names, and team jargon your workflow depends on.
            </p>

            <form className="term-form" onSubmit={(event) => void onAddTerm(event)}>
              <input
                value={termInput}
                placeholder="Add product names, acronyms, or specialist terminology"
                onChange={(event) => setTermInput(event.target.value)}
              />
              <button type="submit" disabled={!termInput.trim()}>
                Add term
              </button>
            </form>

            <div className="term-list" aria-label="Dictionary terms">
              {terms.length === 0 ? (
                <p className="empty-state">
                  No custom terms yet. Add one before testing niche or domain-heavy dictation.
                </p>
              ) : (
                terms.map((term) => (
                  <div className="term-row" key={term.id}>
                    <span>{term.term}</span>
                    <button
                      type="button"
                      aria-label={`Remove ${term.term}`}
                      onClick={() => void removeDictionaryTerm(term.id)}
                    >
                      Remove
                    </button>
                  </div>
                ))
              )}
            </div>
          </article>

          <article className="control-card control-card--wide">
            <div className="card-header">
              <div>
                <p className="section-kicker">Validation</p>
                <h3>Readiness checks</h3>
              </div>
              <div className="validation-summary">
                <SummaryPill label="Pass" value={validationSummary.pass} tone="pass" />
                <SummaryPill label="Warn" value={validationSummary.warn} tone="warn" />
                <SummaryPill label="Fail" value={validationSummary.fail} tone="fail" />
              </div>
            </div>

            <div className="validation-layout">
              <div className="validation-card">
                <div className="subsection-head">
                  <div>
                    <p className="section-kicker">Automated</p>
                    <h4>Runtime checks</h4>
                  </div>
                  <button
                    type="button"
                    disabled={isValidating}
                    onClick={() => void onRunValidationChecks()}
                  >
                    {isValidating ? 'Checking...' : 'Run checks'}
                  </button>
                </div>

                {validationReport ? (
                  <div className="validation-checks">
                    {validationReport.checks.map((check) => (
                      <ValidationRow check={check} key={check.id} />
                    ))}
                  </div>
                ) : (
                  <p className="empty-state">
                    Run the automated checks to verify tray setup, worker binaries, pill window,
                    cloud key state, and audio disk hygiene.
                  </p>
                )}
              </div>

              <div className="validation-card">
                <div className="subsection-head">
                  <div>
                    <p className="section-kicker">Manual</p>
                    <h4>Windows sweep</h4>
                  </div>
                </div>

                <div className="manual-checklist" aria-label="Manual QA checklist">
                  {MANUAL_QA_CHECKS.map((item) => (
                    <label className="manual-check" key={item}>
                      <input type="checkbox" />
                      <span>{item}</span>
                    </label>
                  ))}
                </div>
              </div>
            </div>
          </article>
        </section>
      </section>
    </main>
  )
}

function MetricCard({ label, value, hint }: { label: string; value: string; hint: string }) {
  return (
    <div className="metric-card">
      <span>{label}</span>
      <strong>{value}</strong>
      <small>{hint}</small>
    </div>
  )
}

function SummaryPill({
  label,
  value,
  tone,
}: {
  label: string
  value: number
  tone: 'pass' | 'warn' | 'fail'
}) {
  return (
    <div className={`summary-pill summary-pill--${tone}`}>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  )
}

function ValidationRow({ check }: { check: ValidationCheck }) {
  return (
    <div className="validation-row">
      <div className={`validation-badge is-${check.status}`}>{check.status}</div>
      <div>
        <strong>{check.label}</strong>
        <p>{check.detail}</p>
      </div>
    </div>
  )
}
