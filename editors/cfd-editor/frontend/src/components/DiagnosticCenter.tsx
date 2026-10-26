import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import { useEffect, useState, useSyncExternalStore } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { incidentSnapshot, subscribeIncidents, loggingFailure } from '../diagnostics'
import './DiagnosticCenter.css'
export function DiagnosticCenter() {
  const incidents = useSyncExternalStore(subscribeIncidents, incidentSnapshot)
  const [open, setOpen] = useState(false)
  const [preview, setPreview] = useState('')
  const [failure, setFailure] = useState('')
  const [includeDetails, setIncludeDetails] = useState(false)
  const desktop = '__TAURI_INTERNALS__' in window
  useEffect(() => {
    if (!open || !desktop) return
    let active = true
    void invoke<{ logging_error?: string; environment?: { write_failures: number; dropped_events: number } }>('diagnostic_snapshot').then(status => {
      if (!active) return
      if (status.logging_error) setFailure(`本机日志不可用：${status.logging_error}`)
      else if (status.environment && (status.environment.write_failures || status.environment.dropped_events)) setFailure(`日志写入失败 ${status.environment.write_failures} 次，丢弃事件 ${status.environment.dropped_events} 条。`)
    }).catch(error => { if (active) setFailure(`无法读取日志状态：${String(error)}`) })
    return () => { active = false }
  }, [open, desktop])
  async function prepare() {
    try {
      const host = desktop ? await invoke('diagnostic_snapshot') : { browser: navigator.userAgent }
      setPreview(JSON.stringify({ host, incidents: incidents.map(item => includeDetails ? item : { id: item.id, time: item.time, category: item.category }), reproduction: '请补充操作步骤、预期结果和实际结果' }, null, 2))
      setFailure('')
    } catch (error) { setFailure(`读取诊断失败：${String(error)}`) }
  }
  function download() {
    const url = URL.createObjectURL(new Blob([preview], { type: 'application/json' }))
    const link = document.createElement('a'); link.href = url; link.download = 'coflow-diagnostics.json'; link.click()
    setTimeout(() => URL.revokeObjectURL(url), 1000)
  }
  return <aside className="diagnostic-center">
    <button onClick={() => setOpen(!open)}>日志与反馈{incidents.length ? ` (${incidents.length})` : ''}</button>
    {open && <section aria-label="日志与反馈" role="dialog">
      <header><strong>日志与反馈</strong><button onClick={() => setOpen(false)}>关闭</button></header>
      <p>日志保存在本机。反馈时请说明操作步骤、预期结果和实际结果，并附上预览确认后的诊断文件。</p>
      {loggingFailure && <p role="alert">日志通道暂不可用，以下错误详情仍保留在当前窗口。</p>}
      {failure && <p role="alert">{failure}</p>}
      {desktop && <button onClick={() => { void invoke('open_log_directory').catch(error => setFailure(`打开日志目录失败：${String(error)}`)) }}>打开日志目录</button>}
      {desktop ? <button onClick={() => { void invoke('open_feedback_page').catch(error => setFailure(`打开反馈页面失败：${String(error)}`)) }}>提交问题</button> : <a href="https://github.com/Puring103/coflow/issues/new" target="_blank" rel="noreferrer">提交问题</a>}
      <label><input type="checkbox" checked={includeDetails} onChange={event => { setIncludeDetails(event.target.checked); setPreview('') }} />包含错误文本和堆栈（可能含项目路径或数据，请检查）</label>
      <button onClick={() => { void prepare() }}>生成诊断预览</button>
      {preview && <><textarea aria-label="诊断预览（可编辑脱敏）" value={preview} onChange={event => setPreview(event.target.value)} /><button onClick={download}>保存此预览为文件</button></>}
      <h4>最近问题（最多 100 条）</h4>
      {incidents.length === 0 && <p>当前窗口尚无错误记录。</p>}
      {[...incidents].reverse().map(item => <details key={item.id + item.time}><summary>{item.time} · {item.message}</summary><p>错误编号：{item.id}</p><pre>{item.details}</pre><button onClick={() => { void (desktop ? writeText(`${item.id}\n${item.details}`) : navigator.clipboard.writeText(`${item.id}\n${item.details}`)).catch(error => setFailure(`复制失败：${String(error)}`)) }}>复制详情</button></details>)}
    </section>}
  </aside>
}
