import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@tabler/icons-webfont/dist/tabler-icons.min.css'
import './styles.css'
import App from './App.tsx'
import { applyTheme, watchSystemTheme } from './theme'

applyTheme()        // 渲染前套用已保存主题，避免闪烁
watchSystemTheme()  // 「跟随系统」时实时响应明暗变化

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
