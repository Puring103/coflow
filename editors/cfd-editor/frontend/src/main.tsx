import { DiagnosticCenter } from './components/DiagnosticCenter'
import { EditorErrorBoundary } from './components/EditorErrorBoundary'
import { installGlobalErrorHandlers } from './diagnostics'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClientProvider } from '@tanstack/react-query'
import App from './App'
import { queryClient } from './queryClient'

installGlobalErrorHandlers()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <EditorErrorBoundary><App /></EditorErrorBoundary>
      <DiagnosticCenter />
    </QueryClientProvider>
  </StrictMode>
)
