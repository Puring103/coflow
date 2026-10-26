import { DiagnosticCenter } from './features/diagnostics/components/DiagnosticCenter'
import { EditorErrorBoundary } from './features/diagnostics/components/EditorErrorBoundary'
import { installGlobalErrorHandlers } from './features/diagnostics/incidentReporter'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClientProvider } from '@tanstack/react-query'
import App from './app/App'
import { queryClient } from './app/queryClient'

installGlobalErrorHandlers()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <EditorErrorBoundary><App /></EditorErrorBoundary>
      <DiagnosticCenter />
    </QueryClientProvider>
  </StrictMode>
)
