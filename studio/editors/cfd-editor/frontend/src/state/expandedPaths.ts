export type ExpandedPathMap = ReadonlyMap<string, ReadonlySet<string>>

const EMPTY_PATHS: ReadonlySet<string> = new Set()

export function expandedPathsFor(state: ExpandedPathMap, owner: string): ReadonlySet<string> {
  return state.get(owner) ?? EMPTY_PATHS
}

export function updateExpandedPath(
  state: ExpandedPathMap,
  owner: string,
  path: string,
  expanded: boolean,
): ExpandedPathMap {
  const current = state.get(owner) ?? EMPTY_PATHS
  if (current.has(path) === expanded) return state

  const paths = new Set(current)
  if (expanded) paths.add(path)
  else paths.delete(path)

  const next = new Map(state)
  if (paths.size > 0) next.set(owner, paths)
  else next.delete(owner)
  return next
}
