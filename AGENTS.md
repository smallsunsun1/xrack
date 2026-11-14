# XRack 开发约定

本文件适用于整个仓库。开始修改前用 `git status --short` 检查工作区，保留其他任务的改动。

## 开发流程

| 改动 | 使用的仓库 skill |
| --- | --- |
| 新增或修改组件、控件和可观察的 UI 行为 | [xrack-component-parity](.agents/skills/xrack-component-parity/SKILL.md) |
| 新增或修改 `.xr` 语法、类型、内建函数与值、公开控件接口 | [xrack-language-lsp](.agents/skills/xrack-language-lsp/SKILL.md) |

- 同一个组件功能要同时覆盖解释器和 Rust AOT，复用运行时语义。用同一份 `.xr` 对照属性、事件和交互结果。预览的 AOT 回退不计入解释器验证。
- 新增 `.xr` 语法或公开接口时，同步更新 `.xr` LSP 和受影响的 VS Code 语法高亮。类型、内建签名和名称解析规则优先由共享定义驱动。会改变控件语言接口的任务同时使用两个 skill。
- 新增编译或运行时能力时，保留绑定惰性、短路求值、回调读写顺序和重复行身份。根据影响范围补回归，覆盖可观察结果与相关失败路径。

## 代码入口

| 任务 | 入口 |
| --- | --- |
| DSL、AST、内建函数 | `grammar/Xrack.g4`、`crates/xrack-parser/src/antlr.rs`、`crates/xrack-syntax/src/{ast,builtins}.rs` |
| 类型检查、HIR、编译 CLI | `crates/xrack-ir/src/{semantic,debug_hir}.rs`、`crates/xrack-compiler/src/{lib,main}.rs` |
| MLIR 降低与验证 | `crates/xrack-mlir/src/lowering.rs`、`mlir/Xrack.td`、`mlir/xrack_dialect.cc` |
| MLIR 优化 | `mlir/XrackPasses.td`、`mlir/xrack_passes.cc`、`mlir/xrack_visual_cache.cc`、`mlir/xrack_scene_plan.cc` |
| Rust AOT、解释器 | `crates/xrack-codegen/src/lib.rs`、`crates/xrack-interpreter/src/{load,evaluate}.rs` |
| 状态、布局、绘制、原生宿主 | `xrack-runtime`、`xrack-layout`、`xrack-renderer-femtovg`、`xrack-platform-winit` |
| `.xr` LSP、预览、MLIR LSP | `crates/xrack-lsp/`、`crates/xrack-preview/`、`mlir/xrack_lsp_document.cc`、`.vscode/` |

## 构建与代码约定

- 使用 Bazel 构建和测试。Rust 使用 edition 2024，C++ 使用 C++20。工具链版本以 `.bazelversion`、`rust-toolchain.toml`、`Cargo.toml` 和 `MODULE.bazel` 为准，升级时同步检查两个锁文件和相关 `BUILD.bazel`。
- MLIR、TableGen、clang/libclang 和 SDK 由 Bazel LLVM 模块提供。目标和执行阶段使用 `@llvm//toolchain:all`。增加 macOS framework 时检查 `MODULE.bazel` 的 framework 和 re-export 依赖。
- 修改 ANTLR、ODS 或 C 接口时改源定义，由 Bazel 生成 Rust parser、TableGen 输出和 C bindings。Bazel Rust 目标依赖 `//grammar:xrack_antlr_rust` 与 `//mlir:xrack_mlir_bindings`。诊断保留源码位置，CLI 参数用 `clap` derive，编译错误用 `thiserror`。
- 手写 Rust 模块通过 `mod` 和 crate 根的公开接口组织。手写模块禁止用 `#[path]` 或 `include!` 引入。生成代码可按构建系统的入口引入。保持 Bazel `srcs` 与模块路径同步。生成代码使用标准库和运行时绝对路径，以覆盖 DSL 声明与 Rust 类型同名的情况。
- MLIR 通过 `mlir/c_api.h` 的 C ABI 暴露给 Rust，操作和 SSA 引用受拥有 context 的模块生命周期约束。用 `--emit mlir-raw`、`--emit mlir` 对照优化前后，用 `--emit hir` 检查前端结果。应用通过 `xr_ui_library` 声明 `sources` 和 `assets`。
- 布局、命中测试、绘制和无障碍使用同一份准备好的场景，布局使用逻辑像素。组件状态和绘制留在 UI 线程。应用可用 Tokio 执行异步 I/O，再通过 `spawn_local` 等待或使用 `invoke_from_event_loop` 投递结果。
- 保留 `.bazelrc` 的磁盘缓存并用 `bazel info bazel-bin` 定位产物。Cargo 作为依赖管理、打包或静态检查入口时沿用 `.tools/cargo-build/`，避免根目录 `target/`。Bazel 报告模块锁的隔离模式不一致时，核对记录并按需传 `--repo_env=CARGO_BAZEL_ISOLATED=false`。

## 验证

按改动运行对应目标。常规全量验证、Rust 格式和 Clippy 命令见 [项目构建与验证](docs/development.md)。

| 改动 | Bazel 目标 |
| --- | --- |
| 解析器、语义、新表达式 | `//crates/xrack-parser:parser_test`、`//tests:compiler_test`、`//tests:expressions_test` |
| MLIR 优化与 MLIR 编辑器 | `//tests:mlir_pass_test`、`//tests:mlir_filecheck_test`、`//tests:mlir_lsp_test` |
| 组件与执行一致性 | `//tests:interpreter_parity_test`、`//crates/xrack-interpreter:interpreter_test`、`//tests:ui_interaction_test` |
| 模型、绑定、布局和图片 | `//crates/xrack-runtime:property_test`、`//crates/xrack-layout:layout_test`、`//crates/xrack-assets:assets_test` |
| `.xr` LSP、预览和桌面 | `//crates/xrack-lsp:xrack_lsp_test`、`//crates/xrack-preview:preview_test`、`//crates/xrack-platform-winit:async_test` |

GPU 测试带有 `manual` 标签，按需显式运行 `bazel test //tests:gpu_test` 并使用可用的图形适配器和系统字体。仅由 Bazel 管理的手写 Rust 示例和测试文件可单独用 `rustfmt --edition 2024 --config skip_children=true --check` 验证。

依赖或构建规则变化后，用 `bazel run //:rust_project -- //... //tests:gpu_test` 更新 Rust Analyzer 项目图。C++ 编译动作变化后运行 `python3 tools/compile_commands.py`。编辑器配置与静态检查环境见 [VS Code 与预览](docs/editor.md) 和 [项目构建与验证](docs/development.md)。
