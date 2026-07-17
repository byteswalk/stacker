import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@tabler/icons-webfont/dist/tabler-icons.min.css'
import './styles.css'
import App from './App.tsx'
import LogViewer from './LogViewer.tsx'
import { applyTheme, watchSystemTheme } from './theme'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { LanguageProvider } from './i18n'

applyTheme()        // 渲染前套用已保存主题，避免闪烁
watchSystemTheme()  // 「跟随系统」时实时响应明暗变化

const isLogViewer = getCurrentWindow().label === 'live-log'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <LanguageProvider>
      {isLogViewer ? <LogViewer /> : <App />}
    </LanguageProvider>
  </StrictMode>,
)
