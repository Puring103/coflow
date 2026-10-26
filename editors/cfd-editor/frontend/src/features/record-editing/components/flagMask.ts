// 位标记枚举的纯计算，不依赖字段编辑组件。
export function toggleFlagMask(currentMask: bigint, bit: bigint): bigint {
  if (bit === 0n) return 0n
  return (currentMask & bit) === bit ? currentMask & ~bit : currentMask | bit
}

export function everyFlagMask(
  variants: readonly { value: bigint }[],
): bigint {
  return variants.reduce((mask, variant) => mask | variant.value, 0n)
}

export function selectedFlagVariantNames(
  variants: readonly { name: string, value: bigint }[],
  mask: bigint,
): string[] {
  return variants
    .filter(variant => variant.value === 0n ? mask === 0n : (mask & variant.value) === variant.value)
    .map(variant => variant.name)
}
