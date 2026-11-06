import './motion'
import { StrictMode, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { FeedbackNotice } from '../../src/shared/ui/FeedbackNotice'

// 复用真实详情编辑器和提交计数，验证通知不会改变焦点、选区或关闭行为。
function NoticeFixture() {
  const [count, setCount] = useState(1)
  Object.assign(window, { noticeTest: { setCount } })
  return <>{Array.from({ length: count }, (_, index) => <FeedbackNotice key={index} tone="error" message={`测试错误提示 ${index}`} />)}</>
}

const mount = document.createElement('div')
document.body.append(mount)
createRoot(mount).render(<StrictMode><NoticeFixture /></StrictMode>)
