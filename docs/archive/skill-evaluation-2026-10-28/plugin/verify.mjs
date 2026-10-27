import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
const source = await fs.readFile(new URL('./plugins/item-ids/plugin.js', import.meta.url), 'utf8')
const manifest = JSON.parse(await fs.readFile(new URL('./plugins/item-ids/plugin.json', import.meta.url), 'utf8'))
assert.match(manifest.id, /^[A-Za-z0-9_-]+$/)
assert.equal(manifest.entry, 'plugin.js')
const { default: activate } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`)
let contribution, registrations = 0, removals = 0
const elements = []
globalThis.document = { createElement(tag) {
  assert.equal(tag, 'pre')
  const element = { textContent: '', remove() { removals++ } }
  elements.push(element)
  return element
} }
const plugin = activate({ register: { view(value) { contribution = value; registrations++; return () => registrations-- } } })
assert.deepEqual(contribution.types, ['Item'])
assert.equal(contribution.readOnly, true)
const context = (...ids) => ({ records: ids.map(id => ({ coordinate: { id } })) })
const abort = new AbortController()
const outlet = { signal: abort.signal, replace(element) { this.element = element } }
const view = contribution.mount(context('sword', 'shield'), outlet)
assert.equal(outlet.element.textContent, 'sword\nshield')
view.update(context('new-item', '<script>'))
assert.equal(outlet.element.textContent, 'new-item\n<script>')
view.update(context())
assert.equal(outlet.element.textContent, '')
abort.abort()
assert.equal(removals, 1)
view.update(context('ignored'))
assert.equal(outlet.element.textContent, '')
view.dispose()
assert.equal(removals, 1)
const second = contribution.mount(context('sword'), { signal: new AbortController().signal, replace() {} })
plugin.dispose()
assert.equal(registrations, 0)
assert.equal(removals, 2)
second.update(context('ignored'))
assert.equal(elements[1].textContent, 'sword')
console.log('PASS: manifest, Item read-only registration, initial render, context update, empty list, text safety, abort cleanup, view disposal, plugin unload')
