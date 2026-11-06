import { useLayoutEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { Icon } from './Icon'

let noticeRegion: HTMLDivElement | null = null
let noticeUsers = 0

/** 通知只共享呈现区域；触发条件、内容与持续时间仍由各业务调用方管理。 */
export function FeedbackNotice({ message, tone, className = '' }: {
  message: string
  tone: 'info' | 'success' | 'warning' | 'error'
  className?: string
}) {
  const [host, setHost] = useState<HTMLDivElement | null>(null)
  useLayoutEffect(() => {
    if (!noticeRegion) {
      noticeRegion = document.createElement('div')
      noticeRegion.className = 'editor-notices'
      document.body.append(noticeRegion)
    }
    noticeUsers++
    setHost(noticeRegion)
    return () => {
      if (--noticeUsers === 0) {
        noticeRegion?.remove()
        noticeRegion = null
      }
    }
  }, [])
  return host && createPortal(
    <div
      className={`feedback-notice feedback-notice-${tone} ${className}`}
      role={tone === 'error' ? 'alert' : 'status'}
      onPointerDown={event => {
        // 提示本体吸收指针操作，保留编辑焦点，避免穿透到弹窗遮罩而触发保存关闭。
        event.preventDefault()
        event.stopPropagation()
      }}
      onClick={event => event.stopPropagation()}
    >
      <Icon name={tone === 'error' ? 'error' : tone === 'warning' ? 'warning' : tone === 'success' ? 'check' : 'info'} size={15} aria-hidden />
      <span>{message}</span>
    </div>, host,
  )
}
