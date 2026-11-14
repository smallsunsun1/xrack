---
name: xrack-component-parity
description: 为 XRack 新增或修改组件、控件及可观察的 UI 行为时，同步实现 MLIR 解释器与 Rust AOT，并用同一份 .xr 验证两条路径。
---

# XRack 组件执行一致性

- 先用一个可运行的 `.xr` 用例确定公开属性、默认值、读写权限、回调签名和事件顺序。沿 `xrack-ir`、`mlir/Xrack.td`、`mlir/xrack_dialect.cc` 和 `xrack-mlir` 核对语义检查、降低与验证。
- 在 `xrack-codegen` 与 `xrack-interpreter` 同时实现优化后 IR 的语义。解释器的载入和求值入口分别在 `crates/xrack-interpreter/src/load.rs` 和 `evaluate.rs`。把两条路径共享的默认值、状态更新和事件规则放进已有的运行时或资源层。
- 布局、命中测试、绘制和原生无障碍消费同一份准备好的场景。覆盖受影响的输入路径，例如禁用状态、焦点、键盘、触摸和双向绑定。涉及模型时核对局部失效和行身份，涉及优化时核对惰性求值、短路与副作用顺序。
- 用 `tests/interpreter_parity_test.rs` 或现有专门用例对同一份 `.xr` 比较生成组件与解释器在同一操作序列后的可观察结果。需要布局和输入时复用 `tests/ui_driver.rs`。依赖实际绘制的行为用显式 GPU 目标验证。
- 直接运行解释器以验证支持情况。预览能运行也可能经 AOT 回退。组件或控件的公开 `.xr` 接口变化时，同步执行 [XRack 语法与 LSP](../xrack-language-lsp/SKILL.md) 的编辑器验证。

```sh
bazel test //tests:interpreter_parity_test //crates/xrack-interpreter:interpreter_test
```
