import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { EditorErrorBoundary } from '../../src/features/diagnostics/components/EditorErrorBoundary'
import '../../src/style.css'

function CrashFixture() {
  const [failed, setFailed] = useState(false)
  if (failed) throw new Error('private-project-value')
  return <button onClick={() => setFailed(true)}>触发界面崩溃</button>
}

createRoot(document.getElementById('root')!).render(<EditorErrorBoundary><CrashFixture /></EditorErrorBoundary>)
