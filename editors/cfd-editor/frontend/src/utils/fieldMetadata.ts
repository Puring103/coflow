export function fieldMetadataTitle(name: string, description?: string): string {
  return [
    `实际名称：${name}`,
    description ? `描述：${description}` : null,
  ].filter(Boolean).join('\n')
}
