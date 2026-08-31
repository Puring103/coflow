import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { clampDiagnosticsHeight, diagnosticsHeightFromStorage } from './DiagnosticsPanel'
import { DiagnosticsPanel } from './DiagnosticsPanel'

describe('diagnostics panel height', () => {
  it('keeps the panel within its minimum and the available editor height', () => {
    expect(clampDiagnosticsHeight(40, 600)).toBe(112)
    expect(clampDiagnosticsHeight(260, 600)).toBe(260)
    expect(clampDiagnosticsHeight(900, 600)).toBe(480)
  })

  it('keeps the panel usable in a very short editor', () => {
    expect(clampDiagnosticsHeight(200, 180)).toBe(112)
  })

  it('uses the intended default when no saved height exists', () => {
    expect(diagnosticsHeightFromStorage(null)).toBe(200)
    expect(diagnosticsHeightFromStorage('not-a-number')).toBe(200)
    expect(diagnosticsHeightFromStorage('264')).toBe(264)
  })
})

describe('diagnostics panel list', () => {
  it('renders diagnostics directly without filters or file grouping', () => {
    const html = renderToStaticMarkup(createElement(DiagnosticsPanel, {
      diagnostics: [{
        severity: 'error',
        code: 'CFD001',
        stage: 'check',
        message: 'Invalid value',
        file_path: 'data/table/RegionConfig.cfd',
        actual_type: 'RegionConfig',
        record_key: 'Region_01',
        field_path: 'Value',
        range: null,
        contexts: [],
      }],
    }))

    expect(html).toContain('Invalid value')
    expect(html).not.toContain('diag-toolbar')
    expect(html).not.toContain('diag-group-head')
    expect(html).not.toContain('按文件分组')
  })
})
