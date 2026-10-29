---
name: intent-driven
description: 开发任务的起点，用于需求分析、功能设计与代码改进。当用户提出新功能、新需求、功能优化、代码重构、安全加固等诉求，或说"帮我做个XX"、"这个功能想改一下"时触发。生成 intent.md，并按 intent.md -> spec.md -> plan.md 推进调研、规范与开发计划。
---

# 流程步骤

严格按`先规划，后设计`进行，规划阶段输出的intent.md 未经用户确认时不进行下一步处理。

## 规划：输出intent.md

1. 提出人用自己的话简要描述要实现的功能，想解决的问题；追问涉及的范围、用户、限制条件以及成功的标准。
2. 按照下方intent.md模板撰写结果报告，报告内容应涵盖问题、预期结果、受影响的用户和系统、限制条件以及未决问题，最后由提出人审阅无问题后定稿（状态：draft）
3. 审定后置为 accepted，进入；不做则置 closed 并留一句原因

### intent.md模板

```markdown
# 意图（Intent）: 简要描述要实现的功能或待解决的问题

Author: <姓名/角色>. 状态：<draft|finalize|discard|finish>

## 问题（Problem）

<功能/需求要解决的问题，影响范围>

## 预期结果（Proposed outcome）

<完成的标识，需详尽说明，能度量最好>

## 受影响的模块（Affected modules）

<功能/需求实现涉及的模块、文件、数据库表等>

## 约束条件（Constraints）

<安全、合规、性能、兼容性；明确不做什么>

## 未解决的问题（Open questions）

<需进一步决策、拍板的事项>
```

## 设计：输出 spec.md、plan.md

### spec.md

研读 intent.md 制定需求功能说明书 `spec.md`，标明与现有约束的冲突之处，最后由发起人评审。

### plan.md

用户评审确认后输出功能开发计划`plan.md`.
