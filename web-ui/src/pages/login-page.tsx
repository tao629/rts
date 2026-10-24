import { BrandPanel } from "@/components/brand-panel"
import { LoginForm } from "@/components/login-form"
import { ThemeLogo } from "@/components/theme-logo"

export default function LoginPage() {
  return (
    <div className="min-h-svh bg-white lg:grid lg:grid-cols-[minmax(0,1.05fr)_minmax(0,1fr)]">
      <BrandPanel />

      <main className="flex min-h-svh flex-col items-center justify-center px-6 py-12 sm:px-10">
        {/* 移动端顶部的紧凑品牌条 */}
        <div className="mb-8 flex w-full max-w-[26rem] items-center gap-3 lg:hidden">
          <ThemeLogo />
          <div className="leading-tight">
            <p className="text-base font-semibold text-ink-900">RTS 搬运工</p>
            <p className="text-xs text-ink-500">任务调度 · 链路监控</p>
          </div>
        </div>

        <LoginForm />
      </main>
    </div>
  )
}
