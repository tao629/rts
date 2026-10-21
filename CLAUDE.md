# CLAUDE.md

## 项目概览
这是一个搬运工项目，专注于任务(receive->transform->sink)调度、监控。


## 模块结构
|模块|路径|定位|
|----|----|----|
|服务中心|server/rts-center|收揽平台资源访问接口，通过 REST 服务对其它模块开放|

### rts-center


## 代码开发规约
1. 本项目所有开发、优化、修复、重构等需求，编码之前必须先建立 `sdlc/<yyyyMMdd>_<Slug>/` 目录， 使用 `intent-driven` 技能处理需求，技能输出物存放此目录下。

```text
sdlc/20261015_华为核心网审计日志/
├── intent.md      # 规划阶段(Plan)输出物：需求意图梳理结果
├── spec.md        # 设计阶段(Design)输出物：依据 intent.md 生成的需求规格说明书 
├── plan.md        #构建阶段(Build)输出物：依据 spec.md 需求的可行性方案, 改哪些文件、顺序、风险、验证方式

└── notes.md       # 可选:过程决策、遗留问题
```
1. 严禁在主分支上开发，在新建worktree上进行开发


