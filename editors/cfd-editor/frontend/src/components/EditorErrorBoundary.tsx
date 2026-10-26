import { Component, type ReactNode, type ErrorInfo } from 'react'
import { recordIncident } from '../diagnostics'
export class EditorErrorBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false }
  static getDerivedStateFromError() { return { failed: true } }
  componentDidCatch(error: Error, info: ErrorInfo) { recordIncident('react_render', { message: error.message, stack: error.stack, componentStack: info.componentStack }) }
  render() {
    if (this.state.failed) return <main role="alert"><h2>编辑器界面遇到错误</h2><p>请先通过“日志与反馈”保存诊断信息。重新打开窗口前，请确认未保存内容。</p></main>
    return this.props.children
  }
}
