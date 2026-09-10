import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { CardHeader, DataCardCompact, RefDirectSelect } from '../components/DataCard'
import { ObjectDraftHost } from '../components/ObjectDraftHost'
import { ShortNameContext, ShortNameMenuItem } from '../components/ShortNameContext'
import { EditorLookupController } from './editorLookups'
import { recordShortName, shortNameCandidate, shortNameLabel } from './shortNames'
import type { FieldValue } from '../wire'
import type { FieldCell } from '../bindings/FieldCell'

function makeFieldCell(name: string, value: FieldValue): FieldCell {
  return { name, value, missing: false, annotation: {
    declared_type: value.kind, enum_int_value: null, ref_target_type: null, enum_type: null,
    enum_is_flag: false, nullable: false, read_only: false, item_annotation: null,
    polymorphic_types: [], object_type: null, field_order: [], children: {},
  } }
}

describe('short names', () => {
  it('uses only the selected string field and retains the ID for missing or empty names', () => {
    const fields = [makeFieldCell('name', { kind: 'string', value: 'Sword' }), makeFieldCell('count', { kind: 'int', value: 1n })]
    expect(recordShortName(fields, 'name')).toBe('Sword')
    expect(recordShortName(fields, 'count')).toBeUndefined()
    expect(recordShortName(fields, 'missing')).toBeUndefined()
    expect(recordShortName([{ ...fields[0], missing: true }], 'name')).toBeUndefined()
    expect(recordShortName([makeFieldCell('name', { kind: 'formatted_string', value: { source: 'source', rendered: 'Rendered name' } })], 'name')).toBe('Rendered name')
    expect(recordShortName([makeFieldCell('name', { kind: 'string', value: '' })], 'name')).toBeUndefined()
    expect(shortNameLabel('sword', 'Sword')).toBe('Sword(sword)')
    expect(shortNameLabel('sword', null)).toBe('sword')
    expect(shortNameCandidate(fields, [{ kind: 'field', value: 'count' }])).toBeUndefined()
    expect(shortNameCandidate(fields, [{ kind: 'field', value: 'name' }])).toBe('name')
    expect(shortNameCandidate(fields, [{ kind: 'field', value: 'nested' }, { kind: 'field', value: 'name' }])).toBeUndefined()
  })

  it('renders the configured header and selected menu state while preserving type case', () => {
    const html = renderToStaticMarkup(createElement(ShortNameContext.Provider, {
      value: { fields: { ItemConfig: 'name' }, setField: () => {} },
      children: createElement('div', null,
        createElement(CardHeader, { actualType: 'ItemConfig', recordKey: 'sword', fields: [makeFieldCell('name', { kind: 'string', value: 'Sword' })] }),
        createElement(ShortNameMenuItem, { actualType: 'ItemConfig', field: 'name', onClose: () => {} }),
      ),
    }))
    expect(html).toContain('Sword(sword)')
    expect(html).toContain('>ItemConfig</span>')
    expect(html).toContain('aria-checked="true"')
    expect(html).toContain('取消缩略名')
  })

  it('displays the short name for reference chips and selected references', async () => {
    const lookups = new EditorLookupController({
      getEnumVariants: async () => [],
      getRefTargets: async () => [{ coordinate: { actual_type: 'Item', key: 'sword' }, file_path: 'data/other.cfd', short_name: 'Sword' }],
      makeDefaultObject: async () => ({ kind: 'option_none' }),
      createRecordDraft: async () => ({ actual_type: 'Item', fields: [] }),
    })
    lookups.adopt({ sessionId: 1, revision: 1 })
    await lookups.loadRefTargets('Item')
    const html = renderToStaticMarkup(createElement(ObjectDraftHost, {
      lookups, generationKey: '1:1', onOpenReference: () => {},
      children: createElement('div', null,
        createElement(DataCardCompact, { value: { kind: 'ref', value: 'sword' }, refTargetType: 'Item' }),
        createElement(RefDirectSelect, { value: { kind: 'ref', value: 'sword' }, targetType: 'Item', onCommit: () => {} }),
      ),
    }))
    expect(html).toContain('class="vc-ref-key">Sword</span>')
    expect(html).toContain('value="Sword"')
  })
})
