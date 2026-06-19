import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import compression from "vite-plugin-compression";
import { visualizer } from "rollup-plugin-visualizer";
import path from "node:path";
import { readFileSync } from "node:fs";

const packageJson = JSON.parse(
  readFileSync(new URL("./package.json", import.meta.url), "utf8")
) as { version: string };

// Vite + React config for TokenLens (Tauri v2).
// Note: Tauri expects a fixed dev port (1420) unless overridden.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  define: {
    __APP_VERSION__: JSON.stringify(packageJson.version),
  },
  plugins: [
    react(),
    // Brotli (~70% size reduction) and gzip fallbacks. Tauri serves assets
    // over the `tauri://` custom protocol and decompresses on the fly.
    compression({
      algorithm: "brotliCompress",
      ext: ".br",
      threshold: 1024,
    }),
    compression({
      algorithm: "gzip",
      ext: ".gz",
      threshold: 1024,
    }),
    // Bundle visualizer — only emits a stats.html when ANALYZE=1.
    process.env.ANALYZE === "1" &&
      visualizer({
        filename: "dist/bundle-stats.html",
        template: "treemap",
      }),
  ].filter(Boolean),
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  // Prevent Vite from obscuring rust errors
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 5174,
        }
      : undefined,
    watch: {
      // Don't watch src-tauri (Tauri handles rebuilds)
      ignored: ["**/src-tauri/**"],
    },
  },
  // Env variables starting with TAURI_ are exposed
  envPrefix: ["VITE_", "TAURI_ENV_*", "ANALYZE"],
  build: {
    target: "es2022",
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    cssCodeSplit: true,
    // Manual chunking keeps the initial bundle (which is parsed on every
    // page load) small by hoisting heavy libraries into separate chunks.
    // The recharts group is only used on data-heavy pages; the radix
    // group is small but pulled together; everything else is per-route.
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes("node_modules")) {
            if (id.includes("recharts")) return "vendor-charts";
            if (id.includes("@radix-ui")) return "vendor-radix";
            if (id.includes("lucide-react")) return "vendor-icons";
            if (
              id.includes("/react/") ||
              id.includes("/react-dom/") ||
              id.includes("/scheduler/")
            ) {
              return "vendor-react";
            }
            if (id.includes("@tauri-apps")) return "vendor-tauri";
            return "vendor-misc";
          }
          return undefined;
        },
        // Stable, content-hashed chunk filenames improve HTTP cache reuse
        // across releases.
        chunkFileNames: "assets/[name]-[hash].js",
        entryFileNames: "assets/[name]-[hash].js",
      },
    },
  },
});
