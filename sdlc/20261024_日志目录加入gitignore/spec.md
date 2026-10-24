# 需求规格说明书（Spec）: 日志目录加入 gitignore

依据：`intent.md`（Finalize）。状态：Finalize |

## 1. 范围

在仓库根的 `.gitignore` 中新增 `/logs/`，让日志模块默认产生的日志目录不污染工作树。

不在本次范围：调整日志模块的默认目录（改为写到别处）、忽略其它目录、任何代码改动。

## 2. 变更规格

`.gitignore`：

```gitignore
/target
/dist/
/logs/
```

- 位置：追加在末尾，与既有条目同为根目录锚定风格（`/target`、`/dist/`）。
- 语义：忽略**仓库根目录**下的 `logs/` 及其全部内容；子目录中的同名目录不受影响。

## 3. 验证方式

1. 在仓库根执行 `cargo run -p rts-core --example log_demo`（该示例默认写到 `./logs`）。
2. `git status --porcelain` 输出为空（`logs/` 被正确忽略）。
3. `logs/` 目录本身仍正常生成，`logs/app.log` 与 `logs/archive/` 内容不受影响。
4. `cargo build --workspace` 通过（本次未改代码，仅作回归确认）。

## 4. 不做（重申）

- 不改日志模块的默认目录行为。
- 不忽略其它目录。
- 不改动任何 Rust 代码。
