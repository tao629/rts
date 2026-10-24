import { fileURLToPath } from "node:url"

import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig, loadEnv } from "vite"

// https://vite.dev/config/
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "")

  return {
    plugins: [react(), tailwindcss()],
    resolve: {
      alias: {
        "@": fileURLToPath(new URL("./src", import.meta.url)),
      },
    },
    server: {
      // real 模式下把 /api 转发到后端（rts-center），默认直连同源 /api/v1
      proxy: env.VITE_API_TARGET
        ? {
            "/api": {
              target: env.VITE_API_TARGET,
              changeOrigin: true,
            },
          }
        : undefined,
    },
  }
})
