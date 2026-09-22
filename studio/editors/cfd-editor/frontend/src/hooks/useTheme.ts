import { useEffect, useState } from 'react'

export type Theme = 'light' | 'dark'

const STORAGE_KEY = 'cfd-editor-theme'

function getInitial(): Theme {
  const stored = localStorage.getItem(STORAGE_KEY)
  if (stored === 'light' || stored === 'dark') return stored
  return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
}

export function useTheme() {
  const [theme, setTheme] = useState<Theme>(getInitial)

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme)
    localStorage.setItem(STORAGE_KEY, theme)
  }, [theme])

  function toggle() {
    setTheme(t => (t === 'light' ? 'dark' : 'light'))
  }

  return { theme, toggle }
}
