import { defineConfig, type UserConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Vitest reads this file too; its `test` key is not part of Vite's own type, and the
// vitest/config helper pulls in a second copy of Vite's types, so declare it here.
const config: UserConfig & { test: { setupFiles: string[] } } = {
  plugins: [react()],
  test: {
    // Browser APIs jsdom lacks but Ant Design needs.
    setupFiles: ['./vitest.setup.ts'],
  },
  server: {
    watch: {
      ignored: ['**/src-tauri/target/**'],
    },
  },
}

// https://vite.dev/config/
export default defineConfig(config)
