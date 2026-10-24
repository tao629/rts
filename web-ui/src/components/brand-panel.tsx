import { ThemeLogo } from "./theme-logo"

const STAGES = [
  { label: "receive", desc: "接收" },
  { label: "transform", desc: "转换" },
  { label: "sink", desc: "投递" },
]

/**
 * 登录页左侧深色品牌区：产品定位 + 任务流水线示意 + 服务状态。
 * 纯装饰性内容对辅助技术隐藏，仅保留可读的文本层级。
 */
export function BrandPanel() {
  return (
    <section
      aria-hidden="true"
      className="relative hidden overflow-hidden bg-panel-950 lg:flex lg:flex-col lg:justify-between lg:p-12"
    >
      {/* 网格纹理 */}
      <div
        className="absolute inset-0 opacity-70 [background-image:linear-gradient(to_right,rgba(148,163,184,0.07)_1px,transparent_1px),linear-gradient(to_bottom,rgba(148,163,184,0.07)_1px,transparent_1px)] [background-size:48px_48px]"
      />
      {/* 品牌色光晕 */}
      <div className="rts-drift absolute -top-24 -left-20 size-[30rem] rounded-full bg-brand-500/20 blur-3xl" />
      <div className="rts-drift absolute -right-32 -bottom-32 size-[26rem] rounded-full bg-brand-400/10 blur-3xl [animation-delay:-6s]" />

      <div className="relative flex items-center gap-3">
        <ThemeLogo className="size-10" />
        <div className="leading-tight">
          <p className="text-lg font-semibold tracking-wide text-white">
            RTS 搬运工
          </p>
          <p className="font-mono text-[11px] tracking-[0.18em] text-brand-300/80">
            RTS TASK ORCHESTRATOR
          </p>
        </div>
      </div>

      <div className="relative max-w-lg">
        <h2 className="text-[2.1rem] leading-[1.25] font-semibold text-white short-screen:hidden">
          任务调度
          <br />
          <span className="text-brand-400">链路监控</span>
        </h2>
        <p className="mt-4 max-w-md text-sm leading-relaxed text-slate-400 short-screen:hidden">
          以流水线的方式组织 receive → transform → sink
          ，让每一次数据搬运都有迹可循。
        </p>

        {/* 任务流水线 */}
        <div className="relative mt-10 overflow-hidden rounded-card border border-white/10 bg-white/[0.04] p-5 backdrop-blur-sm short-screen:hidden">
          <span className="rts-sweep pointer-events-none absolute inset-y-0 -left-1/3 w-1/3 bg-gradient-to-r from-transparent via-brand-300/20 to-transparent" />
          <div className="relative flex items-center gap-3">
            {STAGES.map((stage, index) => (
              <div key={stage.label} className="flex items-center gap-3">
                <div className="flex-1 rounded-field border border-white/10 bg-panel-900/70 px-3.5 py-3">
                  <p className="font-mono text-[13px] text-brand-300">
                    {stage.label}
                  </p>
                  <p className="mt-0.5 text-xs text-slate-400">{stage.desc}</p>
                </div>
                {index < STAGES.length - 1 && (
                  <svg
                    viewBox="0 0 24 24"
                    className="size-4 shrink-0 text-brand-400/70"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <path d="m9 6 6 6-6 6" />
                  </svg>
                )}
              </div>
            ))}
          </div>
        </div>
      </div>

      <div className="relative flex items-center gap-2.5 text-xs text-slate-400">
        <span className="rts-blink size-2 rounded-full bg-emerald-400 shadow-[0_0_12px_rgba(52,211,153,0.9)]" />
        <span>服务正常</span>
        <span className="text-slate-600">·</span>
        <span className="font-mono">v1.0.0</span>
      </div>
    </section>
  )
}
