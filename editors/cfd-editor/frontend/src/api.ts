// 编辑器 API 的统一出口。
// 新代码优先从 './api/project|settings|language|data|plugins|events|dialogs|tauriEnv' 直接导入。
export * from './api/tauriEnv'
export * from './api/dialogs'
export * from './api/project'
export * from './api/settings'
export * from './api/language'
export * from './api/data'
export * from './api/plugins'
export * from './api/events'

export type {
  LanguageCompletion,
  LanguageDiagnostic,
  LanguageDocumentState,
  LanguageFormattingResult,
  LanguagePosition,
  LanguageRange,
  LanguageTextEdit,
  DimensionFileRecords,
  DimensionFileRow,
  FrontendPluginBundle,
  FrontendPluginProjectState,
  FrontendPluginState,
  ProjectReloadedEvent,
  ProjectWatchErrorEvent,
} from './api/language-exports'
