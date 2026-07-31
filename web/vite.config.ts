import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: {
    // The 500 KB default warned on the app chunk on every build, which is how
    // a warning stops being read. At 1200 the app chunk (~970 KB) is quiet and
    // the only thing that trips it is monaco, which is 3.3 MB and always will
    // be. That single named warning is the useful state: it says "the big
    // chunk is the one we know about, and nothing else has grown".
    chunkSizeWarningLimit: 1200,
    rollupOptions: {
      output: {
        // Pins monaco into one identifiable chunk. Without this it smears
        // across the app chunk and a broken lazy boundary looks like ordinary
        // growth instead of the mistake it is.
        manualChunks: { monaco: ["monaco-editor"] },
      },
    },
  },
  server: {
    proxy: {
      "/api": "http://127.0.0.1:4820",
      "/ws": { target: "ws://127.0.0.1:4820", ws: true },
    },
  },
});
