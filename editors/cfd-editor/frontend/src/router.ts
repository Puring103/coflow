import { useState, useCallback } from "react";

export type Route =
  | { view: "table"; file: string; typeFilter?: string }
  | { view: "record"; file: string; recordKey: string }
  | { view: "graph"; file: string };

export function useRouter(initial?: Route) {
  const [history, setHistory] = useState<Route[]>(initial ? [initial] : []);
  const [index, setIndex] = useState(initial ? 0 : -1);

  const current: Route | null = index >= 0 ? history[index] : null;
  const canBack = index > 0;
  const canForward = index < history.length - 1;

  const push = useCallback((route: Route) => {
    setHistory(h => [...h.slice(0, index + 1), route]);
    setIndex(i => i + 1);
  }, [index]);

  const replace = useCallback((route: Route) => {
    setHistory(h => { const n = [...h]; n[index] = route; return n; });
  }, [index]);

  const back = useCallback(() => {
    if (canBack) setIndex(i => i - 1);
  }, [canBack]);

  const forward = useCallback(() => {
    if (canForward) setIndex(i => i + 1);
  }, [canForward]);

  return { current, push, replace, back, forward, canBack, canForward };
}
