import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  // Consult https://svelte.dev/docs/introduction#typescript-support
  // for information about TypeScript support
  preprocess: vitePreprocess({
    postcss: true,
  }),

  kit: {
    // adapter-static only supports some environments, see https://svelte.dev/docs/adapter-static for details.
    adapter: adapter({
      pages: "dist",
      assets: "dist",
      fallback: "index.html",
      precompress: false,
      strict: true,
    }),
    alias: {
      $lib: "./src/lib",
    },
    prerender: {
      handleMissingId: "warn",
    },
  },
};

export default config;
