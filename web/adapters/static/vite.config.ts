import { staticAdapter } from "@builder.io/qwik-city/adapters/static/vite";
import { extendConfig } from "@builder.io/qwik-city/vite";
import baseConfig from "../../vite.config";

/**
 * Static site generation (SSG). Every route is prerendered to HTML at build
 * time; there is no Node server in production. Caddy serves `dist/`.
 */
export default extendConfig(baseConfig, () => {
  return {
    build: {
      ssr: true,
      rollupOptions: {
        input: ["@qwik-city-plan"],
      },
    },
    plugins: [
      staticAdapter({
        origin: process.env.SITE_ORIGIN ?? "https://tse.roythan.de",
      }),
    ],
  };
});
