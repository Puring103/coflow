import { Component, type ReactNode, type ErrorInfo } from 'react'
import { recordIncident } from '../incidentReporter'
import { DiagnosticCenter } from './DiagnosticCenter'
export class EditorErrorBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false }
  static getDerivedStateFromError() { return { failed: true } }
  componentDidCatch(error: Error, info: ErrorInfo) { recordIncident('react_render', { message: error.message, stack: error.stack, componentStack: info.componentStack }) }
  render() {
    if (this.state.failed) return <main className="editor-crash">
      <header role="alert"><h2>编辑器界面遇到错误</h2><p>保存所需的诊断信息后，可重新打开编辑器。</p></header>
      <DiagnosticCenter />
    </main>
    return this.props.children
  }
}
