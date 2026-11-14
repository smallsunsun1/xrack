# 项目构建与验证

[返回项目首页](../README.md)

## 构建和运行

要求 Bazel 9.2.0 / Bazelisk 和 JDK 17+（用于 ANTLR generator）。Bazel 通过 Bzlmod 获取 LLVM 提供的 **C++20** 编译器、链接器和目标平台 SDK。首次构建会从源码编译 LLVM/MLIR，耗时明显多于增量构建。

```sh
bazel build //:xrackc
bazel test //...
bazel run //examples/counter:counter
bazel run //examples/counter:counter_headless
bazel run //examples/stepper:stepper
bazel run //examples/painting:gallery
bazel run //examples/media:media
bazel run //examples/layout:layout
bazel run //examples/form:form
bazel run //examples/controls:controls
bazel run //examples/selection:selection
bazel run //examples/navigation:navigation
bazel run //examples/list:list
bazel run //examples/list:list -- --position
bazel run //examples/list:list -- --linked
bazel run //examples/list:list -- --anchor
bazel run //examples/delegates:delegates
bazel run //examples/notes:notes
bazel run //examples/dialog:dialog
bazel run //examples/menu:menu
bazel run //examples/table:table
bazel run //examples/table:table -- --frozen
bazel run //examples/table:table -- --rows
bazel run //examples/table:table -- --row-field
bazel run //examples/table:records
bazel run //examples/split:split
bazel run //examples/tooltip:tooltip
bazel run //examples/tree:tree
bazel run //examples/async_demo:async_demo

# 在真实 GPU 上运行离屏验证和导出 PNG
bazel test //tests:gpu_test --test_output=all
bazel run //examples/counter:counter_snapshot -- /tmp/counter.png

# 默认输出 Rust；-o 指定输出路径
bazel run //:xrackc -- examples/counter/counter.xr -o /tmp/counter.rs
bazel run //:xrackc -- --emit mlir examples/counter/counter.xr
bazel run //:xrackc -- --emit mlir-raw examples/counter/counter.xr
bazel run //:xrackc -- --emit ast examples/counter/counter.xr
bazel run //:xrackc -- --emit hir examples/counter/counter.xr
bazel run //:xrackc -- --check examples/counter/counter.xr
```

系统、`rust-toolchain.toml`、`Cargo.toml` 和 `rules_rust` toolchain 使用 Rust **1.99.1**、edition **2024**；`rules_rust` 固定为 **0.75.0**。依赖版本由 `Cargo.lock` 与 `MODULE.bazel.lock` 锁定。

MLIR 通过 `llvm` module **0.8.26** 导出的 `@llvm-project` 构建，LLVM 版本固定为 **23.1.2**。目标与执行阶段都由 `@llvm//toolchain:all` 提供 C++ 工具链。Linux 使用 glibc 2.28 目标平台，macOS 使用 LLVM module 管理的 Apple SDK，Windows 保持 MSVC ABI。

Windows 的 Bazel genrule 需要 Bash。下载 MSVC runtime 和 Windows SDK 前，请核对 [Microsoft 组件许可要求](https://github.com/hermeticbuild/windows_support)，并在有权使用这些组件时设置 `BAZEL_MSVC_RUNTIME_VISUAL_STUDIO_EULA=1` 和 `BAZEL_WINDOWS_SDK_EULA=1`。

Bazel 使用配套的 Rust 目标生成器和 `antlr4rust 0.6.0` runtime。`.g4` 变化后，Bazel 通过 JDK 重新生成 Rust lexer、parser 和 listener。JDK 只用于生成源码。

Cargo workspace 为依赖管理和可选 Rust 静态检查提供支持。`.cargo/config.toml` 将构建产物放到 `.tools/cargo-build/`。静态检查读取 Bazel 生成的 Rust parser 和 MLIR C bindings。

```sh
bazel build //grammar:generate_rust //mlir:c_bindings_out_dir
export XRACK_ANTLR_DIR="$(pwd)/bazel-bin/grammar/generated/rust"
export XRACK_MLIR_BINDINGS="$(pwd)/bazel-bin/mlir/c_bindings_out_dir"
cargo check --workspace --exclude 'xrack-mlir-bin-*' --locked
```

Rust 格式检查和 Clippy 使用 Bazel 的 Rust 工具链。Clippy 检查手写 Rust，并将 correctness 类诊断作为构建错误。生成的 UI 代码通过编译器测试验证。

```sh
bazel build --config=fmt //... //tests:gpu_test
bazel build --config=lint //... //tests:gpu_test
```

依赖 MLIR 原生库的可执行文件和测试通过 Bazel 构建。Cargo 可用于运行独立的 parser 测试，执行 `cargo test -p xrack-parser --locked` 前先按上文生成 Rust parser 并设置 `XRACK_ANTLR_DIR`。

## Bazel 磁盘缓存

仓库 `.bazelrc` 默认启用 `--disk_cache=~/.cache/bazel/xrack`。此缓存保存构建动作输出，可以跨 Bazel 重启、不同 output base 和兼容的 worktree 复用；它与下载源码包的 repository cache、当前 output base 的增量缓存分开。

本地并发数由 Bazel 根据宿主资源自动确定，需要临时调整时可传 `--jobs=N`。GitHub Actions 为 Linux 和 Windows runner 设置 4 个 job，为 macOS arm64 runner 设置 3 个。

本地回收策略为 `--experimental_disk_cache_gc_max_size=60G` 和 `--experimental_disk_cache_gc_max_age=30d`，给 LLVM/MLIR 及不同目标平台的产物留出空间。GitHub Actions 将 runner 的 Bazel 缓存清理目标设为 100 GiB，实际可用空间由 runner 提供。空间有限时可在命令上覆盖，例如 `--experimental_disk_cache_gc_max_size=30G`。

可用 `bazel info --announce_rc output_base` 检查实际加载的配置。构建摘要里的 `disk cache hit` 表示从共享磁盘缓存取回产物，`action cache hit` 表示当前 output base 已有产物。Bazel 在命中磁盘缓存时也会回放动作保存的编译警告，可用摘要里的 sandbox 执行数确认实际执行量。切换编译模式、目标平台、工具链或编译选项会形成不同的动作缓存键。

GitHub Actions 由 `main` 的 push 构建保存缓存，PR 和发布 tag 可以恢复该缓存。合并后的首次 `main` 构建负责填充缓存。CI 的 Bazel 构建使用默认 `opt` 模式，Cargo 运行时测试和独立消费示例使用 `--release`。

## Bazel 中使用 UI 文件

```python
load("//bazel:xrack.bzl", "xr_ui_library")

xr_ui_library(
    name = "counter_ui",
    src = "counter.xr",
)
```

该宏创建代码生成 action 和正常的 `rust_library`。Rust 应用依赖 `:counter_ui`，使用 `counter_ui::Counter`；不需要 `build.rs` 或手动管理生成路径。生成代码只依赖 `xrack-runtime`。

带图片的目标在 `xr_ui_library` 里用 `assets = ["assets/icon.svg"]` 声明每个资源。Bazel 会在资源内容变化时重新运行代码生成。编译后的程序可以脱离源目录运行。

`import { Card as BaseCard } from "widgets.xr";` 可导入相对路径文件中导出的组件。Bazel 目标用 `sources = ["widgets.xr"]` 声明每个直接或间接导入文件。自定义组件可继承导入的组件，复用子树并覆盖可写输入与根元素绑定。

组件实例的 `in-out` 属性支持双向链接，例如 `editor := Editor { draft <=> root.document; }`。初值来自右侧，父组件、子组件的控件事件和后续赋值都通过同一属性读写。也可链接到兄弟实例的可写属性，或在多层封装中继续传递链接。编译器要求两端类型一致，并拒绝循环。单向输入仍可按原有方式绑定和覆盖。

声明也可以用 `in-out property <string> draft <=> entry.text;` 把内部属性公开为组件 API。目标可以是静态可寻址的可写声明，或文本框、双态复选框和开关、数值框及滑块的编辑属性。别名继承目标的默认值；调用方传入绑定时，编译器将其转发到最终存储位置。多个别名可以公开同一属性，同一个实例初始化该存储位置时只接受一个输入绑定。

虚拟列表和网格中的组件实例可将输入链接到可写模型的字符串项、整条记录或记录字段，例如 `for task in root.tasks : Editor { draft <=> task.name; }`。写入按行标识定位并合并到当前记录，保留其他字段。排序、插删和模型替换后，旧场景的事件会核对行标识。运行 `bazel run //examples/stepper:stepper -- --linked` 体验跨组件编辑和模型行编辑。

## 测试

`bazel test //crates/xrack-parser:parser_test` 对照迁移时从 C++ 前端采集的完整 AST 和诊断快照。用例覆盖表达式、Unicode、BOM、CRLF 和错误恢复。JSON 快照只用于回归测试。

`bazel test //tests:mlir_pass_test` 对同一原生模块断言优化前后的 SSA 和操作结构。用例覆盖常量折叠、CSE、动态分支及跨赋值的读取顺序。

`bazel test //tests:ui_interaction_test` 用控件 ID 驱动点击、键盘导航和文本输入，在确定性的布局下验证计数器、模态对话框与选择控件。该目标可在无 GPU 的 CI 中运行。`bazel test //tests:gpu_test` 额外使用原生图形适配器验证离屏像素和绘制路径。
