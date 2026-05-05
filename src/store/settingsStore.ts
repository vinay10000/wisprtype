import { useCallback, useEffect, useMemo, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'

export type ModelSize = 'tiny' | 'base' | 'small' | 'medium' | 'large'
export type SttEngine = 'local' | 'cloud' | 'auto'
export type CloudProvider = 'gladia' | 'openai' | 'groq' | 'deepgram'
export type PillStyle = 'aurora' | 'minimal'

export interface AppSettings {
  stt_model_size: ModelSize
  stt_engine: SttEngine
  cloud_provider: CloudProvider
  hotkey: string
  pill_visible: boolean
  pill_style: PillStyle
  launch_at_login: boolean
}

export interface DictionaryTerm {
  id: number
  term: string
  created_at: string
}

export interface CloudApiKeyStatus {
  provider: string
  configured: boolean
}

export type ValidationStatus = 'pass' | 'warn' | 'fail'

export interface ValidationCheck {
  id: string
  label: string
  status: ValidationStatus
  detail: string
}

export interface ValidationReport {
  checks: ValidationCheck[]
}

export const DEFAULT_SETTINGS: AppSettings = {
  stt_model_size: 'base',
  stt_engine: 'auto',
  cloud_provider: 'gladia',
  hotkey: 'Super+Space',
  pill_visible: true,
  pill_style: 'aurora',
  launch_at_login: true,
}

export function useSettingsStore() {
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS)
  const [terms, setTerms] = useState<DictionaryTerm[]>([])
  const [apiKeyStatus, setApiKeyStatus] = useState<CloudApiKeyStatus | null>(null)
  const [validationReport, setValidationReport] = useState<ValidationReport | null>(null)
  const [isValidating, setIsValidating] = useState(false)
  const [isLoading, setIsLoading] = useState(true)
  const [isSaving, setIsSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const loadDictionary = useCallback(async () => {
    const nextTerms = await invoke<DictionaryTerm[]>('list_dictionary_terms')
    setTerms(nextTerms)
  }, [])

  const refreshApiStatus = useCallback(async (provider: CloudProvider) => {
    const status = await invoke<CloudApiKeyStatus>('cloud_api_key_status', {
      provider,
    })
    setApiKeyStatus(status)
  }, [])

  const load = useCallback(async () => {
    setIsLoading(true)
    setError(null)
    try {
      const loadedSettings = await invoke<AppSettings>('get_settings')
      setSettings({ ...DEFAULT_SETTINGS, ...loadedSettings })
      await Promise.all([
        loadDictionary(),
        refreshApiStatus(loadedSettings.cloud_provider ?? DEFAULT_SETTINGS.cloud_provider),
      ])
    } catch (err) {
      setError(errorMessage(err))
    } finally {
      setIsLoading(false)
    }
  }, [loadDictionary, refreshApiStatus])

  useEffect(() => {
    queueMicrotask(() => void load())
  }, [load])

  const saveSettings = useCallback(async () => {
    setIsSaving(true)
    setError(null)
    try {
      const saved = await invoke<AppSettings>('save_settings', { settings })
      setSettings(saved)
      await refreshApiStatus(saved.cloud_provider)
    } catch (err) {
      setError(errorMessage(err))
      throw err
    } finally {
      setIsSaving(false)
    }
  }, [refreshApiStatus, settings])

  const saveSettingsPatch = useCallback(
    async (patch: Partial<AppSettings>) => {
      const nextSettings = { ...settings, ...patch }
      setSettings(nextSettings)
      setIsSaving(true)
      setError(null)
      try {
        const saved = await invoke<AppSettings>('save_settings', { settings: nextSettings })
        setSettings(saved)
        await refreshApiStatus(saved.cloud_provider)
        return saved
      } catch (err) {
        setError(errorMessage(err))
        throw err
      } finally {
        setIsSaving(false)
      }
    },
    [refreshApiStatus, settings],
  )

  const runValidationChecks = useCallback(async () => {
    setIsValidating(true)
    setError(null)
    try {
      const report = await invoke<ValidationReport>('run_validation_checks')
      setValidationReport(report)
      return report
    } catch (err) {
      setError(errorMessage(err))
      throw err
    } finally {
      setIsValidating(false)
    }
  }, [])

  const updateSettings = useCallback((patch: Partial<AppSettings>) => {
    setSettings((current) => ({ ...current, ...patch }))
  }, [])

  const addDictionaryTerm = useCallback(
    async (term: string) => {
      setError(null)
      try {
        await invoke<DictionaryTerm>('add_dictionary_term', { term })
        await loadDictionary()
      } catch (err) {
        setError(errorMessage(err))
        throw err
      }
    },
    [loadDictionary],
  )

  const removeDictionaryTerm = useCallback(
    async (id: number) => {
      setError(null)
      try {
        await invoke<void>('remove_dictionary_term', { id })
        await loadDictionary()
      } catch (err) {
        setError(errorMessage(err))
        throw err
      }
    },
    [loadDictionary],
  )

  const storeApiKey = useCallback(
    async (provider: CloudProvider, apiKey: string) => {
      setError(null)
      try {
        await invoke<void>('store_cloud_api_key', { provider, apiKey })
        await refreshApiStatus(provider)
      } catch (err) {
        setError(errorMessage(err))
        throw err
      }
    },
    [refreshApiStatus],
  )

  const deleteApiKey = useCallback(
    async (provider: CloudProvider) => {
      setError(null)
      try {
        await invoke<void>('delete_cloud_api_key', { provider })
        await refreshApiStatus(provider)
      } catch (err) {
        setError(errorMessage(err))
        throw err
      }
    },
    [refreshApiStatus],
  )

  return useMemo(
    () => ({
      addDictionaryTerm,
      apiKeyStatus,
      deleteApiKey,
      error,
      isLoading,
      isSaving,
      isValidating,
      load,
      refreshApiStatus,
      removeDictionaryTerm,
      runValidationChecks,
      saveSettings,
      saveSettingsPatch,
      settings,
      setError,
      storeApiKey,
      terms,
      updateSettings,
      validationReport,
    }),
    [
      addDictionaryTerm,
      apiKeyStatus,
      deleteApiKey,
      error,
      isLoading,
      isSaving,
      isValidating,
      load,
      refreshApiStatus,
      removeDictionaryTerm,
      runValidationChecks,
      saveSettings,
      saveSettingsPatch,
      settings,
      storeApiKey,
      terms,
      updateSettings,
      validationReport,
    ],
  )
}

function errorMessage(err: unknown) {
  return err instanceof Error ? err.message : String(err)
}
