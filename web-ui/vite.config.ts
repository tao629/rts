import { fileURLToPath } from "node:url"

import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig, loadEnv } from "vite"

// 后端 rts-center 的默认监听地址；用 VITE_API_TARGET 覆盖。
const DEFAULT_API_TARGET = "http://127.0.0.1:8080"

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
      // 前端始终同源请求 /api/v1：开发期由 vite 代理转发到 rts-center，
      // 部署期由 nginx 反代。前端因此不需要处理 CORS。
      proxy: {
        "/api": {
          target: env.VITE_API_TARGET || DEFAULT_API_TARGET,
          changeOrigin: true,
        },
      },
    },
  }
})
