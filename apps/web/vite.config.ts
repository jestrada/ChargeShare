import { fileURLToPath, URL } from "node:url"
import { defineConfig } from "vite"
import react from "@vitejs/plugin-react"
import tailwindcss from "@tailwindcss/vite"

export default defineConfig(({ mode }) => ({
  plugins: [react(), tailwindcss(), ...(mode === "persisted" ? [{
    name: "persisted-entry",
    transformIndexHtml: { order: "pre" as const, handler(html: string) { return html.replace("/src/main.tsx", "/src/persisted-main.tsx").replace(/<title>.*?<\/title>/, "<title>Persisted charging evidence</title>") } },
  }] : [])],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  server: {
    host: "127.0.0.1", port: mode === "persisted" ? 5174 : 5173, strictPort: true,
    proxy: { [mode === "persisted" ? "/api/poc" : "/api"]: mode === "persisted" ? "http://127.0.0.1:8788" : "http://127.0.0.1:8787" },
  },
  build: { outDir: mode === "persisted" ? "dist-persisted" : "dist" },
}))
