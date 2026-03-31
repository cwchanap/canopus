import { sveltekit } from "@sveltejs/kit/vite";
import { svelteTesting } from "@testing-library/svelte/vite";
import { defineConfig } from "vitest/config";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [sveltekit(), svelteTesting({ autoCleanup: false, noExternal: false })],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 5183,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    target:
      process.env.TAURI_ENV_PLATFORM === "windows"
        ? "chrome105"
        : process.env.TAURI_ENV_PLATFORM === "linux"
          ? "chrome105"
          : "safari13",
    minify: process.env.TAURI_ENV_DEBUG === "true" ? false : "esbuild",
    sourcemap: process.env.TAURI_ENV_DEBUG === "true",
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/vitest.setup.ts"],
    include: ["src/**/*.test.ts"],
    coverage: {
      provider: "v8",
      reporter: ["text", "html"],
      all: true,
      include: ["src/App.svelte"],
    },
  },
});
