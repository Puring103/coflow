import { describe, expect, it } from 'vitest'
import { incidentSnapshot, recordIncident } from './incidentReporter'

describe('incident capture', () => {
  it('preserves the raw stack while displaying context and deduplicating propagation', () => {
    const error = new Error('plugin failure')
    const before = incidentSnapshot().length
    recordIncident('plugin', error, undefined, '插件示例运行失败')
    recordIncident('editor', error, undefined, '上下文中的插件错误')
    const incidents = incidentSnapshot()
    expect(incidents).toHaveLength(before + 1)
    expect(incidents[incidents.length - 1]?.details).toContain(error.stack)
    expect(incidents[incidents.length - 1]?.message).toBe('上下文中的插件错误')
  })
})
