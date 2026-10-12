import { defineConfig } from "vitest/config";
import tsconfigPaths from "vite-tsconfig-paths";

// Unit tests cover plain TypeScript modules (API client, formatting), so the
// Qwik plugins from vite.config.ts are left out on purpose.
export default defineConfig({
  plugins: [tsconfigPaths({ root: "." })],
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
