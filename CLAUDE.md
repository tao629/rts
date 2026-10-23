# CLAUDE.md

## 项目概览
这是一个搬运工项目，专注于任务(receive->transform->sink)调度、监控。


## 模块结构
|模块|路径|定位|
|----|----|----|
|公共包|base/rts-core|平台封装的公用功能包，如日志、配置等|
|服务中心|server/rts-center|收揽平台资源访问接口，通过 REST 服务对其它模块开放|

### rts-center


## 代码开发规约
* 严格按照 `立项 → 开发 → 验证 → 交付 → 发布`流程进行
* 流程开始前必须先建立 `sdlc/<yyyyMMdd>_<Slug>/` 目录
* 所有需求必须在独立worktree里开发，本机验证通过后进行分支提交并创建PR

### 立项
使用 `intent-driven` 技能处理需求。

```text
sdlc/20261015_华为核心网审计日志/
├── intent.md      # 规划阶段(Plan)输出物：需求意图梳理结果
├── spec.md        # 设计阶段(Design)输出物：依据 intent.md 生成的需求规格说明书 
└── plan.md        # 构建阶段(Build)输出物：依据 spec.md 需求的可行性方案, 改哪些文件、顺序、风险、验证方式
```

### 开发
禁止在主仓checkout上切分支开发，主仓只用于拉取和只读查看，不切分支、不暂存、不提交；所有开发都在独立的`worktree (.claude/worktrees/<slug)/`里开发

