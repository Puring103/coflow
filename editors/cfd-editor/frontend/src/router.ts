import { useState, useCallback } from "react";

export type Route =
  | { view: "table"; file: string; typeFilter?: string }
  | { view: "record"; file: string; recordKey: string }
  | { view: "graph"; file: string };

interface RouterState {
  history: Route[];
  index: number;
}

export function useRouter(initial?: Route) {
  const [state, setState] = useState<RouterState>(() => ({
    history: initial ? [initial] : [],
    index: initial ? 0 : -1,
  }));

  const current: Route | null = state.index >= 0 ? state.history[state.index] : null;
  const canBack = state.index > 0;
  const canForward = state.index < state.history.length - 1;

  const push = useCallback((route: Route) => {
    setState(s => {
      const newIndex = s.index + 1;
      return { history: [...s.history.slice(0, newIndex), route], index: newIndex };
    });
  }, []);

  const replace = useCallback((route: Route) => {
    setState(s => {
      const h = [...s.history];
      h[s.index] = route;
      return { history: h, index: s.index };
    });
  }, []);

  const back = useCallback(() => {
    setState(s => s.index > 0 ? { ...s, index: s.index - 1 } : s);
  }, []);

  const forward = useCallback(() => {
    setState(s => s.index < s.history.length - 1 ? { ...s, index: s.index + 1 } : s);
  }, []);

  const reset = useCallback(() => {
    setState({ history: [], index: -1 });
  }, []);

  return { current, push, replace, back, forward, canBack, canForward, reset };
}
