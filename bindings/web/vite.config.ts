import { defineConfig } from "vite";

export default defineConfig({
  server: {
    port: 5173,
    watch: {
      // Atomic-save tools briefly materialize temp files that Node's Windows
      // watcher cannot open; ignoring them keeps the dev server alive.
      ignored: ["**/*.tmp", "**/.index.html.*.tmpdir/**"],
    },
  },
  build: {
    target: "es2022",
  },
});
