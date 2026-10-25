import { CheckCircle2, LogOut } from "lucide-react"
import { useNavigate } from "react-router-dom"

import { useAuth } from "@/auth/auth-context"
import { Button } from "@/components/ui/button"
import { ThemeLogo } from "@/components/theme-logo"

const PLACEHOLDER_CARDS = [
  { label: "运行中任务", hint: "任务调度概览" },
  { label: "链路健康度", hint: "receive → transform → sink" },
  { label: "今日投递量", hint: "sink 侧吞吐统计" },
]

export default function DashboardPage() {
  const { session, logout, loginNotice } = useAuth()
  const navigate = useNavigate()

  const handleLogout = async () => {
    await logout()
    navigate("/login", { replace: true })
  }

  const user = session?.user

  return (
    <div className="min-h-svh bg-ink-100">
      {loginNotice && (
        <div
          role="status"
          className="fixed top-4 left-1/2 z-50 flex -translate-x-1/2 items-center gap-2 rounded-full border border-emerald-200 bg-emerald-50 px-4 py-2 text-sm text-emerald-700 shadow-panel"
        >
          <CheckCircle2 className="size-4" aria-hidden="true" />
          登录成功
        </div>
      )}

      <header className="border-b border-ink-200 bg-white">
        <div className="mx-auto flex h-16 max-w-5xl items-center justify-between px-6">
          <div className="flex items-center gap-3">
            <ThemeLogo />
            <div className="leading-tight">
              <p className="text-sm font-semibold text-ink-900">RTS 搬运工</p>
              <p className="font-mono text-[10px] tracking-[0.16em] text-ink-400">
                RTS TASK ORCHESTRATOR
              </p>
            </div>
          </div>

          <div className="flex items-center gap-4">
            <div className="hidden text-right sm:block">
              <p className="text-sm font-medium text-ink-900">
                {user?.displayName ?? user?.username}
              </p>
              <p className="font-mono text-[11px] text-ink-400">
                {user?.roles.join(" · ")}
              </p>
            </div>
            <Button
              variant="outline"
              size="sm"
              onClick={handleLogout}
              aria-label="退出登录"
            >
              <LogOut className="size-4" aria-hidden="true" />
              退出
            </Button>
          </div>
        </div>
      </header>

      <main className="mx-auto max-w-5xl px-6 py-10">
        <h1 className="text-xl font-semibold tracking-tight text-ink-900">
          控制台
        </h1>
        <p className="mt-1.5 text-sm text-ink-500">
          你好，{user?.displayName ?? user?.username}。这里是登录后的占位首页，
          后续将承载任务调度与链路监控视图。
        </p>

        <div className="mt-8 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {PLACEHOLDER_CARDS.map((card) => (
            <div
              key={card.label}
              className="rounded-card border border-ink-200 bg-white p-5 shadow-xs"
            >
              <p className="text-sm font-medium text-ink-900">{card.label}</p>
              <p className="mt-1 text-xs text-ink-400">{card.hint}</p>
              <div className="mt-5 h-2 w-2/3 animate-pulse rounded-full bg-ink-200" />
            </div>
          ))}
        </div>
      </main>
    </div>
  )
}
