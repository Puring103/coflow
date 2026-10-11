// api 兼容层：按域拆分后的统一出口，存量导入路径不变。
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
  FunctionDocumentState,
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
