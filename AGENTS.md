# XRack 工作约定

本文件适用于整个仓库。

## 构建与工具链

- 使用 Bazel 构建应用、编译器和涉及 MLIR 链接的测试。版本以 `.bazelversion`、`rust-toolchain.toml`、`Cargo.toml` 和 `MODULE.bazel` 为准。Rust 使用 edition 2024，原生 C++ 使用 C++20。
- MLIR、TableGen 和 bindgen 使用 `MODULE.bazel` 配置的 Bazel LLVM 工具链。新增依赖或构建规则时沿用该工具链。
- ANTLR grammar 位于 `grammar/Xrack.g4`。Bazel 用 JDK 运行生成器，`xrack-parser` 用 Rust runtime 从规则上下文构造 AST。生成的 Rust 源码留在 Bazel 输出目录。
- Cargo 用于依赖管理、格式化和可选静态检查。保留 `.cargo/config.toml` 中的 `.tools/cargo-build/` 输出路径和 `.bazelrc` 中的磁盘缓存配置。

## Rust 模块与编译链

- 手写模块在所属 crate 的 `lib.rs` 或父模块中用普通 `mod` 声明，其他模块通过 `crate::...` 引用。按用途选择模块和符号的可见性，避免用 `#[path]` 指向手写源码。
- 编译链为 `.xr → ANTLR 解析树 → Rust AST → 语义分析 → xr MLIR → Rust TokenStream`。新增语言构造时检查语法、类型、MLIR 校验和 Rust 生成各阶段。操作与类型定义放在 `mlir/` 的 TableGen 文件中，Rust 后端通过 MLIR C API 读取优化后的 operation 和 SSA 值。
- 控件变更时检查语义属性、运行时状态、布局与输入、渲染和原生无障碍的对应行为。根据改动添加回归用例，覆盖适用的键盘、鼠标、焦点、禁用状态和双向绑定路径。

## 验证与编辑器

```sh
cargo fmt --all --check
bazel test //...
bazel test //tests:gpu_test
bazel run //examples/counter:counter_headless
```

GPU 测试需显式运行，并使用可用的图形适配器和系统字体。可用 `bazel run //examples/counter:counter` 验证原生窗口，用 `bazel run //examples/counter:counter_snapshot -- /tmp/counter.png` 验证离屏绘制。

按改动选用专门的测试目标。

| 改动 | Bazel 目标 |
| --- | --- |
| 解析器与诊断 | `//crates/xrack-parser:parser_test` |
| MLIR pass | `//tests:mlir_pass_test` |
| 控件布局与输入 | `//crates/xrack-layout:layout_test`、`//tests:ui_interaction_test` |
| 桌面宿主与无障碍 | `//crates/xrack-platform-winit:async_test` |

更新构建规则或依赖后，可用 `bazel run //:rust_project -- //... //tests:gpu_test` 重建 Rust Analyzer 项目图。C++ 编译动作变化后用 `python3 tools/compile_commands.py` 重建 clangd 编译数据库。两份文件包含本机路径，由 Git 忽略。静态检查所需的生成代码和环境变量见 `README.md`。
