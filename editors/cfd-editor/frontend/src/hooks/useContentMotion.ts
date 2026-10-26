import { useLayoutEffect, useRef, type RefObject } from 'react'
import { motionDuration, motionToken, reducedMotion } from '../utils/motion'

/** 只响应导航身份变化；数据刷新与输入不重播，也不重新挂载正文。 */
export function useContentMotion(ref: RefObject<HTMLDivElement | null>, identity: string) {
  const previous = useRef(identity)
  useLayoutEffect(() => {
    const changed = previous.current !== identity
    previous.current = identity
    if (!changed || !ref.current || reducedMotion()) return
    const animation = ref.current.animate([{ opacity: .65 }, { opacity: 1 }], {
      duration: motionDuration('enter'), easing: motionToken('enter-ease'),
    })
    return () => animation.cancel()
  }, [identity, ref])
}
