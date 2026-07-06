import js from '@eslint/js'
import globals from 'globals'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import tseslint from 'typescript-eslint'
import { defineConfig, globalIgnores } from 'eslint/config'

export default defineConfig([
  globalIgnores([
    'dist',
    'dist-ssr',
    'node_modules',
    'src-tauri/target',
    'src-tauri/target/**',
    'src-tauri/gen/schemas',
    'src-tauri/gen/schemas/**',
  ]),
  {
    files: ['**/*.{ts,tsx}'],
    extends: [
      js.configs.recommended,
      tseslint.configs.recommended,
      reactHooks.configs.flat.recommended,
      reactRefresh.configs.vite,
    ],
    languageOptions: {
      globals: globals.browser,
    },
    rules: {
      // This app loads Tauri state on mount in page components; the rule is too
      // broad for async invoke-driven screens.
      'react-hooks/set-state-in-effect': 'off',
      // Shared UI files export components plus hooks/helpers by design.
      'react-refresh/only-export-components': 'off',
    },
  },
])
