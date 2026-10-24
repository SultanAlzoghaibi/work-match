import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: {
    // 1420 for instance 1; instance 2 sets VITE_PORT=1421 (see package.json).
    port: Number(process.env.VITE_PORT) || 1420,
    strictPort: true,
  },
  build: {
    rollupOptions: {
      // Two webviews: the main panel and the match popup.
      input: {
        main: "index.html",
        match: "match.html",
      },
    },
  },
});
