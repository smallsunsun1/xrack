# XRack 工作约定

本文件适用于整个仓库。

## 构建与工具链

- 使用 Bazel 构建应用、编译器和测试。版本以 `.bazelversion`、`rust-toolchain.toml`、`Cargo.toml` 和 `MODULE.bazel` 为准。Rust 使用 edition 2024，原生 C++ 使用 C++20。升级时同步检查 `Cargo.lock` 和 `MODULE.bazel.lock`。
- MLIR、TableGen 和 `rules_rust_bindgen` 共用 Bazel `llvm` 模块导出的 `@llvm-project`。原生 MLIR 依赖统一从该仓库解析，生成的 C bindings 通过 Bazel `rust_library` 提供给 `xrack-mlir`。
- ANTLR grammar 位于 `grammar/Xrack.g4`。JDK 只在构建时运行生成器，`xrack-parser` 用 `antlr4rust` 从生成的 Rust 规则上下文直接构造 AST。修改 grammar 后由 Bazel 更新生成的源码。
- 用 `bazel info bazel-bin` 定位构建产物。保留 `.bazelrc` 的磁盘缓存 `~/.cache/bazel/xrack`。Rust 格式检查和 Clippy 使用 Bazel。Cargo 用于依赖管理、打包和可选类型检查，沿用 `.cargo/config.toml` 的 `.tools/cargo-build/` 输出路径，避免在仓库根目录生成 `target/`。
- 新增 crate 或依赖时同步检查 Cargo workspace 与相应的 `BUILD.bazel`。ANTLR Rust 源码、TableGen 输出和 MLIR C bindings 都由 Bazel 生成，改动写在生成规则或源文件中。

## Rust 模块与编译链

- 手写模块使用 Rust 的常规模块路径。在 `lib.rs` 声明同级模块，在父模块中声明其目录下的子模块。用 `crate::...` 引用同一 crate 内的其他模块，跨 crate 使用的接口由 `lib.rs` 导出。避免用 `#[path]` 或 `include!` 引入手写模块。Bazel 生成的 ANTLR 源码和 MLIR 绑定可以用 `include!` 接入。
- 例如，`xrack-platform-winit/src/lib.rs` 声明 `mod accessibility_text;`，`accessibility.rs` 用 `crate::accessibility_text` 访问所需符号。移动或新增模块时，检查 Bazel 的 `srcs` 是否覆盖新路径，尤其是只匹配 `src/*.rs` 的规则。
- 编译器 CLI 参数沿用 `clap` derive，编译错误沿用 `thiserror`。解析和语义诊断保留源码位置。
- 编译链为 `.xr → ANTLR 解析树 → Rust AST → 语义分析 → xr MLIR → 优化 pass → Rust TokenStream`。`xr` 方言承载 UI IR。操作和类型在 `mlir/Xrack.td` 定义，通过 TableGen 生成的 builder 构造原生模块。`xrack-codegen` 通过 MLIR C API 读取优化后的操作和 SSA 值，用 `quote!` 生成 `proc_macro2::TokenStream`，由 `xrackc` 输出 Rust 源码。
- `mlir/bridge.cc` 负责原生模块构建、方言注册和 pass 调用。扩展接口时沿用 `mlir/c_api.h` 的 C ABI 和 Bazel 生成的绑定。模块拥有 MLIR context，Rust 端的操作和 SSA 引用应受模块生命周期约束。AST、HIR 和 MLIR 的文本输出用于诊断和调试，编译阶段之间传递类型化数据或 MLIR 句柄。
- MLIR pass 在 `mlir/XrackPasses.td` 声明，由 `mlir/xrack_passes.cc` 编排。改动需保留动态绑定的惰性求值、短路和回调中的读写顺序。用 `xrackc --emit mlir-raw` 和 `--emit mlir` 对照优化前后的模块。
- 示例和应用通过 `xr_ui_library` 编译 `.xr`。用 `sources` 声明直接和间接的 `.xr` 导入，用 `assets` 声明图像和内嵌字体等资源。应用依赖生成的 Rust 库，生成代码依赖 `xrack-runtime`。

## 控件与运行时

- 控件变更时检查 `xrack-ir` 的语义规则、MLIR 校验与代码生成、`xrack-runtime` 的状态、`xrack-layout` 的布局与输入、`xrack-renderer-femtovg` 的绘制和 `xrack-platform-winit` 的原生无障碍。语言或控件的公开接口变化时同步检查 `xrack-lsp` 的补全目录。
- 属性更新沿用运行时的依赖追踪和惰性失效机制。改动绑定或模型时，覆盖条件依赖切换、未受影响的属性继续命中缓存，以及列表插删和重排后行状态按标识保留等适用场景。
- 组件状态和窗口绘制留在 UI 线程。`spawn_local` 用于需要访问组件的本地 Future。异步 I/O 可交给应用持有的 Tokio runtime，阻塞工作放到工作线程或 Tokio 的 `spawn_blocking`，再通过 `UiSender` 把结果发回 UI 线程。
- 根据改动添加回归用例，覆盖适用的键盘、鼠标、焦点、禁用状态和双向绑定路径。优先通过 `tests/ui_driver.rs` 驱动生成的组件，用 GPU 测试覆盖依赖真实绘制的行为。

## 验证与编辑器

```sh
bazel build --config=fmt //... //tests:gpu_test
bazel build --config=lint //... //tests:gpu_test
bazel test //...
bazel test //tests:gpu_test
bazel run //examples/counter:counter_headless
```

GPU 测试带有 `manual` 标签，需显式运行，并使用可用的图形适配器和系统字体。可用 `bazel run //examples/counter:counter` 验证原生窗口，用 `bazel run //examples/counter:counter_snapshot -- /tmp/counter.png` 验证离屏绘制。

如果 Bazel 报告 `CARGO_BAZEL_ISOLATED` 与模块锁文件记录不一致，先核对 `MODULE.bazel.lock`。当前记录为 `false`，可为本次命令添加 `--repo_env=CARGO_BAZEL_ISOLATED=false` 后重试。

按改动选用专门的测试目标。

| 改动 | Bazel 目标 |
| --- | --- |
| 解析器与诊断 | `//crates/xrack-parser:parser_test` |
| 语义分析与代码生成 | `//tests:compiler_test`、`//tests:expressions_test` |
| MLIR pass | `//tests:mlir_pass_test` |
| 属性绑定与失效 | `//crates/xrack-runtime:property_test` |
| 控件布局与输入 | `//crates/xrack-layout:layout_test`、`//tests:ui_interaction_test` |
| 桌面宿主与无障碍 | `//crates/xrack-platform-winit:async_test` |
| `.xr` 编辑器支持 | `//crates/xrack-lsp:xrack_lsp_test` |

更新构建规则或依赖后，可用 `bazel run //:rust_project -- //... //tests:gpu_test` 重建 Rust Analyzer 项目图。C++ 编译动作变化后用 `python3 tools/compile_commands.py` 重建 clangd 编译数据库。两份文件包含本机路径，由 Git 忽略。静态检查所需的生成代码和环境变量见 `README.md`。
