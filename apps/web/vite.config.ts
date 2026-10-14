import { fileURLToPath, URL } from "node:url"
import { defineConfig } from "vite"
import react from "@vitejs/plugin-react"
import tailwindcss from "@tailwindcss/vite"

function persistedApiTarget() {
  const port = process.env.CHARGESHARE_API_PORT ?? "8788"
  const numericPort = Number(port)
  if (!port || /[^0-9]/.test(port) || numericPort < 1 || numericPort > 65535) {
    throw new Error("CHARGESHARE_API_PORT must contain digits for a port from 1 to 65535")
  }
  return `http://127.0.0.1:${numericPort}`
}

export default defineConfig(({ mode }) => ({
  plugins: [react(), tailwindcss(), ...(mode === "persisted" ? [{
    name: "persisted-entry",
    transformIndexHtml: { order: "pre" as const, handler(html: string) { return html.replace("/src/main.tsx", "/src/persisted-main.tsx").replace(/<title>.*?<\/title>/, "<title>Persisted charging evidence</title>") } },
  }] : [])],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  server: {
    host: "127.0.0.1", port: mode === "persisted" ? 5174 : 5173, strictPort: true,
    proxy: { [mode === "persisted" ? "/api/poc" : "/api"]: mode === "persisted" ? persistedApiTarget() : "http://127.0.0.1:8787" },
  },
  build: { outDir: mode === "persisted" ? "dist-persisted" : "dist" },
}))
