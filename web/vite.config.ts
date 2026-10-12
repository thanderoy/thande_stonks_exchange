/**
 * Base Vite config. The static adapter config in `adapters/static/` extends it
 * for the production build.
 */
import { defineConfig, type UserConfig } from "vite";
import { qwikVite } from "@builder.io/qwik/optimizer";
import { qwikCity } from "@builder.io/qwik-city/vite";
import tsconfigPaths from "vite-tsconfig-paths";

/** Where `npm run dev` and `npm run preview` forward API and WebSocket calls. */
const API_TARGET = process.env.API_TARGET ?? "http://127.0.0.1:8080";

const proxy = {
  "/v1": {
    target: API_TARGET,
    changeOrigin: true,
    ws: true,
  },
};

export default defineConfig((): UserConfig => {
  return {
    plugins: [
      qwikCity(),
      // Fonts are self-hosted; let unicode-range pick the subsets a page needs
      // instead of preloading every weight and subset up front.
      qwikVite({ disableFontPreload: true }),
      tsconfigPaths({ root: "." }),
    ],
    build: {
      // Keep small font subsets as files instead of data: URIs, so the CSP
      // can stay `font-src 'self'`.
      assetsInlineLimit: (file: string) =>
        /\.woff2?$/.test(file) ? false : undefined,
    },
    server: {
      proxy,
      headers: {
        "Cache-Control": "public, max-age=0",
      },
    },
    preview: {
      proxy,
      headers: {
        "Cache-Control": "public, max-age=0",
      },
    },
  };
});
