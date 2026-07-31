import type { FieldValue } from '../wire'

export type NumericFieldValue = FieldValue & { kind: 'int' | 'float' }

export function scrubNumericValue(
  start: NumericFieldValue,
  deltaX: number,
  modifiers: { shiftKey: boolean; altKey: boolean },
): NumericFieldValue {
  const scale = modifiers.shiftKey ? 10 : modifiers.altKey ? 0.1 : 1
  if (start.kind === 'int') {
    const delta = BigInt(Math.round(deltaX * 0.5 * scale))
    return { kind: 'int', value: start.value + delta }
  }
  const next = start.value + deltaX * 0.1 * scale
  return { kind: 'float', value: Number(next.toPrecision(12)) }
}

export function sameNumericValue(left: NumericFieldValue, right: NumericFieldValue): boolean {
  return left.kind === right.kind && left.value === right.value
}
