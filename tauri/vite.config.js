import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
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
