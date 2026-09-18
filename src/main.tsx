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
import AppErrorBoundary from './AppErrorBoundary'
import { reportFrontendError, reportFrontendWarning } from './invoke'

const diagnosticsWindow = window as Window & { __stackerDiagnosticsInstalled?: boolean }
if (!diagnosticsWindow.__stackerDiagnosticsInstalled) {
  diagnosticsWindow.__stackerDiagnosticsInstalled = true
  window.addEventListener('error', (event) => {
    reportFrontendError('Unhandled frontend error', event.error ?? event.message)
  })
  window.addEventListener('unhandledrejection', (event) => {
    reportFrontendError('Unhandled frontend promise rejection', event.reason)
  })
}

applyTheme()        // 渲染前套用已保存主题，避免闪烁
watchSystemTheme()  // 「跟随系统」时实时响应明暗变化

const currentWindow = getCurrentWindow()
const isLogViewer = currentWindow.label === 'live-log'

if (!isLogViewer) {
  await initializeMainWindowSize(currentWindow).catch((error) => {
    reportFrontendWarning('Unable to restore the saved main-window size.', error)
  })
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <LanguageProvider>
      <AppErrorBoundary>
        {isLogViewer ? <LogViewer /> : <App />}
      </AppErrorBoundary>
    </LanguageProvider>
  </StrictMode>,
)
