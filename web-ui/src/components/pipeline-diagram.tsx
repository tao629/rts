const STAGES = [
  { label: "receive", desc: "接收" },
  { label: "transform", desc: "转换" },
  { label: "sink", desc: "投递" },
]

/**
 * 任务流水线示意图：receive → transform → sink 三段汇聚到下方的 monitor 节点。
 *
 * 布局用等分三列的网格，使每个阶段列的中心恰好落在容器宽度的
 * 1/6、1/2、5/6 处；连接线据此用百分比定位，保证线与节点永远对齐。
 */
export function PipelineDiagram() {
  return (
    <div className="relative overflow-hidden rounded-card border border-white/10 bg-white/[0.04] p-5 backdrop-blur-sm">
      {/* 流动光点 */}
      <span className="rts-sweep pointer-events-none absolute inset-y-0 -left-1/3 w-1/3 bg-gradient-to-r from-transparent via-brand-300/20 to-transparent" />

      {/* 阶段行 */}
      <div className="relative grid grid-cols-3">
        {STAGES.map((stage, index) => (
          <div key={stage.label} className="relative px-2">
            <div className="rounded-field border border-white/10 bg-panel-900/70 px-3 py-3">
              <p className="font-mono text-[13px] text-brand-300">
                {stage.label}
              </p>
              <p className="mt-0.5 text-xs text-slate-400">{stage.desc}</p>
            </div>

            {/* 阶段 → 总线的下行线 */}
            <span className="absolute left-1/2 top-full h-3 w-px -translate-x-1/2 bg-brand-400/60" />

            {/* 阶段之间的箭头 */}
            {index < STAGES.length - 1 && (
              <svg
                viewBox="0 0 24 24"
                className="absolute left-full top-1/2 z-10 size-4 -translate-x-1/2 -translate-y-1/2 text-brand-400/70"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <path d="m9 6 6 6-6 6" />
              </svg>
            )}
          </div>
        ))}
      </div>

      {/* 汇聚总线：两端正对首尾阶段的中心 */}
      <div className="mx-[16.667%] mt-3 h-px bg-brand-400/60" />

      {/* 总线 → monitor */}
      <div className="mx-auto h-3 w-px bg-brand-400/60" />
      <div className="mx-auto h-0 w-0 border-x-[6px] border-t-[6px] border-x-transparent border-t-brand-400/60" />

      <div className="mx-auto mt-1.5 w-[64%] rounded-field border border-brand-400/30 bg-brand-500/10 px-3 py-2.5 text-center">
        <p className="font-mono text-[13px] text-brand-200">monitor</p>
        <p className="mt-0.5 text-xs text-slate-400">链路监控</p>
      </div>
    </div>
  )
}
