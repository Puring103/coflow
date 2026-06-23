import { useReducer, useCallback, useMemo } from 'react'
import type { Route } from '../bindings/index'

interface State {
  stack: Route[]
  cursor: number
}

type Action =
  | { type: 'push';    route: Route }
  | { type: 'replace'; route: Route }
  | { type: 'back' }
  | { type: 'forward' }

function reduce(s: State, a: Action): State {
  switch (a.type) {
    case 'push':
      return { stack: [...s.stack.slice(0, s.cursor + 1), a.route], cursor: s.cursor + 1 }
    case 'replace': {
      const next = [...s.stack]
      next[Math.max(0, s.cursor)] = a.route
      return { ...s, stack: next }
    }
    case 'back':
      return s.cursor > 0 ? { ...s, cursor: s.cursor - 1 } : s
    case 'forward':
      return s.cursor < s.stack.length - 1 ? { ...s, cursor: s.cursor + 1 } : s
  }
}

export interface RouterState {
  current: Route | null
  canBack: boolean
  canForward: boolean
  push: (r: Route) => void
  replace: (r: Route) => void
  back: () => void
  forward: () => void
}

export function useRouter(): RouterState {
  const [s, dispatch] = useReducer(reduce, { stack: [], cursor: -1 })

  const push    = useCallback((r: Route) => dispatch({ type: 'push',    route: r }), [])
  const replace = useCallback((r: Route) => dispatch({ type: 'replace', route: r }), [])
  const back    = useCallback(() => dispatch({ type: 'back' }),    [])
  const forward = useCallback(() => dispatch({ type: 'forward' }), [])

  // Memoize the returned object so consumers that depend on `router` in
  // their own useEffect/useCallback deps don't re-subscribe every render
  // (the previous literal { ... } was a fresh reference each time).
  return useMemo<RouterState>(() => ({
    current:    s.cursor >= 0 ? s.stack[s.cursor] ?? null : null,
    canBack:    s.cursor > 0,
    canForward: s.cursor < s.stack.length - 1,
    push,
    replace,
    back,
    forward,
  }), [s, push, replace, back, forward])
}

