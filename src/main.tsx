import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@tabler/icons-webfont/dist/tabler-icons.min.css'
import './styles.css'
import App from './App.tsx'
import LogViewer from './LogViewer.tsx'
import { applyTheme, watchSystemTheme } from './theme'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { LanguageProvider } from './i18n'
import { initializeMainWindowSize } from './windowSize'

applyTheme()        // 渲染前套用已保存主题，避免闪烁
watchSystemTheme()  // 「跟随系统」时实时响应明暗变化

const currentWindow = getCurrentWindow()
const isLogViewer = currentWindow.label === 'live-log'

if (!isLogViewer) {
  await initializeMainWindowSize(currentWindow).catch((error) => {
    console.warn('Unable to restore the saved main-window size.', error)
  })
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <LanguageProvider>
      {isLogViewer ? <LogViewer /> : <App />}
    </LanguageProvider>
  </StrictMode>,
)
