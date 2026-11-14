---
name: xrack-language-lsp
description: 为 XRack 新增或修改 .xr 语法、内建函数与值、类型或公开控件接口时，同步更新编译器、VS Code 语法高亮和 .xr LSP。
---

# XRack 语法与 LSP

- 沿 `grammar/Xrack.g4`、`xrack-parser`、`xrack-syntax`、`xrack-ir` 核对解析、源码位置、名称解析和类型检查。修改 grammar 后用 Bazel 生成 parser，有变化的诊断快照以实际解析结果为准。
- 可复用的内建名称、参数约束、返回类型和文档集中到 `crates/xrack-syntax/src/builtins.rs` 等共享定义。普通内建函数用限定名，`@` 用于专用语法。维护同名用户接收者的解析优先级。新函数沿同一目录接入 LSP 的补全、悬停和签名提示。
- 检查 `crates/xrack-lsp/src` 和 `crates/xrack-lsp/vscode/syntaxes/xrack.tmLanguage.json`。按改动覆盖诊断、高亮、格式化以及可导航符号的跳转、引用和重命名。完整代码优先使用编译器语义，未完成的编辑保留容错路径。跨文件用例覆盖 VS Code 未保存内容优先。
- 在 `crates/xrack-lsp/src/tests.rs` 补可观察的行为，用完整源码和输入到一半的源码分别验证。语法新增运行 parser 测试。若语法会降低到新 IR，比较解释器和 AOT 的执行结果。控件改动可参考 [XRack 组件执行一致性](../xrack-component-parity/SKILL.md)。

```sh
bazel test //crates/xrack-parser:parser_test //crates/xrack-lsp:xrack_lsp_test
```
