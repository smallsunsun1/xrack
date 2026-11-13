# XRack 工作约定

本文件适用于整个仓库。开始修改前用 `git status --short` 检查工作区，保留与当前任务无关的改动。生成文件由对应的 Bazel 规则或工具更新。

## 代码入口

| 任务 | 入口 |
| --- | --- |
| DSL 语法与诊断 | `grammar/Xrack.g4`、`crates/xrack-parser/src/antlr.rs`、`crates/xrack-syntax/src/ast.rs` |
| 类型检查与 HIR 调试输出 | `crates/xrack-ir/src/semantic.rs`、`crates/xrack-ir/src/debug_hir.rs` |
| 编译流程与 CLI | `crates/xrack-compiler/src/lib.rs`、`crates/xrack-compiler/src/main.rs` |
| MLIR 降低与校验 | `crates/xrack-mlir/src/lowering.rs`、`mlir/Xrack.td`、`mlir/xrack_dialect.cc` |
| MLIR 优化 | `mlir/XrackPasses.td`、`mlir/xrack_passes.cc`、`mlir/xrack_visual_cache.cc`、`mlir/xrack_scene_plan.cc` |
| Rust 代码生成 | `crates/xrack-codegen/src/lib.rs` |
| 属性、控件与绘制 | `xrack-runtime`、`xrack-layout`、`xrack-renderer-femtovg`、`xrack-platform-winit` |
| VS Code 与 `.xr` 语言服务 | `.vscode/`、`crates/xrack-lsp/` |

## 构建与工具链

- 使用 Bazel 构建应用、编译器和测试。版本以 `.bazelversion`、`rust-toolchain.toml`、`Cargo.toml` 和 `MODULE.bazel` 为准。Rust 使用 edition 2024，原生 C++ 使用 C++20。升级 Rust 时同步 `rust-toolchain.toml`、`Cargo.toml` 的 `rust-version` 和 `MODULE.bazel` 的 toolchain 配置，并检查 `Cargo.lock` 和 `MODULE.bazel.lock`。
- MLIR 和 TableGen 由 Bazel `llvm` 模块导出的 `@llvm-project` 构建。目标与执行阶段都使用 `@llvm//toolchain:all`，bindgen 的 libclang 和头文件也由 Bazel 提供。macOS 新增系统 framework 依赖时，检查 `MODULE.bazel` 的 `osx.frameworks` 是否包含链接所需的 framework 及其 re-export 依赖。
- ANTLR grammar 位于 `grammar/Xrack.g4`。JDK 只在构建时运行生成器，`xrack-parser` 用 `antlr4rust` 从生成的 Rust 规则上下文直接构造 AST。修改 grammar 后由 Bazel 更新生成的源码。
- 用 `bazel info bazel-bin` 定位构建产物。保留 `.bazelrc` 的磁盘缓存 `~/.cache/bazel/xrack`。Rust 格式检查和 Clippy 使用 Bazel。Cargo 用于依赖管理、打包和可选类型检查，沿用 `.cargo/config.toml` 的 `.tools/cargo-build/` 输出路径，避免在仓库根目录生成 `target/`。
- 新增 crate 或依赖时同步检查 Cargo workspace 与相应的 `BUILD.bazel`。ANTLR Rust 源码、TableGen 输出和 MLIR C bindings 都由 Bazel 生成，改动写在生成规则或源文件中。

## Rust 模块与编译链

- 手写模块使用 Rust 的常规模块路径。库 crate 在 `src/lib.rs` 用 `mod` 声明顶层模块，子模块由父模块声明。同 crate 内用 `crate::...` 引用，跨 crate 接口由 `lib.rs` 用 `pub mod` 或 `pub use` 导出。禁止用 `#[path]` 或 `include!` 引入手写模块。
- 例如，`xrack-platform-winit/src/lib.rs` 声明 `mod accessibility_text;`，`accessibility.rs` 用 `crate::accessibility_text` 访问所需符号。移动或新增模块时，检查 Bazel 的 `srcs` 是否覆盖新路径，尤其是只匹配 `src/*.rs` 的规则。
- 库 crate 的单元测试可由 `#[cfg(test)] mod tests;` 接入。独立的 Bazel 集成测试通过 `rust_test` 的 `deps` 声明跨 crate 依赖，测试内部的共享模块按 `tests/ui_driver.rs` 的方式用 `mod` 声明并加入 `srcs`。
- Bazel 主构建通过 `//grammar:xrack_antlr_rust` 和 `//mlir:xrack_mlir_bindings` 两个 `rust_library` 目标依赖生成代码。Cargo 静态检查使用 `XRACK_ANTLR_DIR` 和 `XRACK_MLIR_BINDINGS` 指向 Bazel 生成的源码。启用发布版 `xrack` 的 `interpreter` feature 时，由 Cargo 选择 `bundles/mlir` 对应的原生产物包。
- 编译器 CLI 参数沿用 `clap` derive，编译错误沿用 `thiserror`。解析和语义诊断保留源码位置。
- 编译链为 `.xr → ANTLR 解析树 → Rust AST → 语义分析 → xr MLIR → 优化 pass → Rust TokenStream`。`xr` 方言承载 UI IR。操作和类型在 `mlir/Xrack.td` 定义，通过 TableGen 生成的 builder 构造原生模块。`xrack-codegen` 通过 MLIR C API 读取优化后的操作和 SSA 值，用 `quote!` 生成 `proc_macro2::TokenStream`，由 `xrackc` 输出 Rust 源码。
- 生成的 Rust 对标准库和运行时类型使用绝对路径，例如 `::core::option::Option` 和 `::xrack_runtime::Node`。DSL 可以声明 `Option`、`String` 等同名组件。`//tests:expressions_test` 覆盖这类命名。
- `mlir/bridge.cc` 负责原生模块构建、方言注册和 pass 调用。扩展接口时沿用 `mlir/c_api.h` 的 C ABI 和 Bazel 生成的绑定。模块拥有 MLIR context，Rust 端的操作和 SSA 引用应受模块生命周期约束。AST、HIR 和 MLIR 的文本输出用于诊断和调试，编译阶段之间传递类型化数据或 MLIR 句柄。
- MLIR pass 在 `mlir/XrackPasses.td` 声明，由 `mlir/xrack_passes.cc` 编排。改动需保留动态绑定的惰性求值、短路和回调中的读写顺序。用 `xrackc --emit mlir-raw` 和 `--emit mlir` 对照原始降低结果与优化结果。用 `--emit hir` 查看前端完成名称解析和类型检查后的调试视图。
- 示例和应用通过 `xr_ui_library` 编译 `.xr`。用 `sources` 声明直接和间接的 `.xr` 导入，用 `assets` 声明图像和内嵌字体等资源。应用依赖生成的 Rust 库，生成代码依赖 `xrack-runtime`。

## 控件与运行时

- 控件变更时检查 `xrack-ir` 的语义规则、MLIR 校验与代码生成、`xrack-runtime` 的状态、`xrack-layout` 的布局与输入、`xrack-renderer-femtovg` 的绘制和 `xrack-platform-winit` 的原生无障碍。语言或控件的公开接口变化时同步检查 `xrack-lsp` 的补全目录。
- `xrack-layout` 使用逻辑像素计算布局、裁剪和命中测试。绘制、输入和无障碍应消费同一份准备好的场景及布局。FemtoVG 通过 wgpu 绘制，窗口、DPI 和原生事件由 winit 宿主管理。
- 原生平台能力在 `xrack-platform-winit` 中按平台编译。把共享状态和事件路由留在宿主层，并区分交叉类型检查和目标系统上的实际交互验证。
- 属性更新沿用运行时的依赖追踪和惰性失效机制。改动绑定或模型时，覆盖条件依赖切换、未受影响的属性继续命中缓存，以及列表插删和重排后行状态按标识保留等适用场景。
- 组件状态和窗口绘制留在 UI 线程。`spawn_local` 用于需要访问组件的本地 Future。异步 I/O 可交给应用持有的 Tokio runtime，阻塞工作放到工作线程或 Tokio 的 `spawn_blocking`，再通过 `invoke_from_event_loop` 或组件弱句柄的 `upgrade_in_event_loop` 把结果发回 UI 线程，也可以在 `spawn_local` 中等待 Tokio 任务句柄。
- 根据改动添加回归用例，覆盖适用的键盘、鼠标、焦点、禁用状态和双向绑定路径。优先通过 `tests/ui_driver.rs` 驱动生成的组件，用 GPU 测试覆盖依赖真实绘制的行为。

## 验证与编辑器

```sh
bazel build --config=fmt //... //tests:gpu_test
bazel build --config=lint //... //tests:gpu_test
bazel test //...
bazel test //tests:gpu_test
bazel run //examples/counter:counter_headless
```

GPU 测试带有 `manual` 标签，需显式运行，并使用可用的图形适配器和系统字体。可用 `bazel run //examples/counter:counter` 验证原生窗口，用 `bazel run //examples/counter:counter_snapshot -- /tmp/counter.png` 验证离屏绘制。在无 GPU 的环境中，可用 `//tests:ui_interaction_test` 通过确定性的布局验证控件交互。

修改 `tests/` 或 `examples/` 下仅由 Bazel 管理的 Rust 文件时，对这些文件单独运行 `rustfmt --edition 2024 --config skip_children=true --check`。

如果 Bazel 报告 `CARGO_BAZEL_ISOLATED` 与模块锁文件记录不一致，先核对 `MODULE.bazel.lock`。当前记录为 `false`，可为本次命令添加 `--repo_env=CARGO_BAZEL_ISOLATED=false` 后重试。

按改动选用专门的测试目标。

| 改动 | Bazel 目标 |
| --- | --- |
| 解析器与诊断 | `//crates/xrack-parser:parser_test` |
| 语义分析与代码生成 | `//tests:compiler_test`、`//tests:expressions_test` |
| MLIR pass | `//tests:mlir_pass_test` |
| 属性绑定与失效 | `//crates/xrack-runtime:property_test` |
| 图像解码与图标栅格化 | `//crates/xrack-assets:assets_test` |
| 控件布局与输入 | `//crates/xrack-layout:layout_test`、`//tests:ui_interaction_test` |
| 桌面宿主与无障碍 | `//crates/xrack-platform-winit:async_test` |
| `.xr` 编辑器支持 | `//crates/xrack-lsp:xrack_lsp_test` |

更新构建规则或依赖后，用 `bazel run //:rust_project -- //... //tests:gpu_test` 或 VS Code 任务 **Bazel: Generate rust-project.json** 重建 Rust Analyzer 项目图。C++ 编译动作变化后，用 `python3 tools/compile_commands.py` 或 **Bazel: Generate compile_commands.json** 重建 clangd 编译数据库。两份文件包含本机路径，由 Git 忽略。静态检查所需的生成代码和环境变量见 `README.md`。
