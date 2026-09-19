import { resolve } from "node:path";
import { defineConfig } from "vite";

const root = resolve(import.meta.dirname);

export default defineConfig({
  root,
  publicDir: false,
  build: {
    outDir: resolve(root, "dist"),
    emptyOutDir: false,
    lib: { entry: resolve(root, "src/content/index.ts"), formats: ["iife"], name: "stackerContent", fileName: () => "content.js" },
  },
});
