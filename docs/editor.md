# VS Code 与预览

[返回项目首页](../README.md)

## VS Code

安装 XRack 扩展后，可以在 `.xr` 中输入 `Math.`、`Strings.`、`Model.`、`Colors.`、`Dates.`、`I18n.` 和 `Log.` 查看内建函数及常量。扩展提供悬停、参数提示与诊断。 [XRack 扩展文档](../crates/xrack-lsp/vscode/README.md)

打开仓库根目录，安装推荐的 Rust Analyzer 和 CodeLLDB 扩展。用 **Terminal → Run Task → Bazel: Generate rust-project.json** 生成本机项目图，或在终端运行：

```sh
bazel run //:rust_project -- //... //tests:gpu_test
```

Rust Analyzer 读取 Bazel 的 crate 图、标准库源码、生成的 Rust 文件和过程宏。项目图包含本机路径，由 Git 忽略。更改 `BUILD.bazel`、依赖或工具链后重新生成；如果编辑器仍显示旧图，运行 **Rust Analyzer: Reload Workspace**。

C++ 使用 clangd。运行 **Bazel: Generate compile_commands.json** 或 `python3 tools/compile_commands.py`，从 Bazel 的编译动作导出 MLIR 桥接和方言源码的参数。脚本会先生成 TableGen 文件，并保留 Bazel 的头文件路径、宏、C++20 和 macOS SDK 参数。数据库包含本机路径，由 Git 忽略。

本工作区从 `PATH` 选择 clangd。macOS 上请使用与当前 Xcode / SDK 兼容的 clangd，可用 `clangd --version` 检查。过旧的 clangd 可能无法解析 SDK 的 libc++ 头文件，即使 Bazel 构建正常也会出现连带诊断。更改 clangd 路径后，重启 VS Code 的 clangd 服务即可重新诊断。

默认构建任务是 **Bazel: Build Counter**；**Bazel: Run Counter** 打开窗口，**Bazel: Test workspace** 运行常规测试，**Bazel: Test native GPU** 运行需要显卡和字体的测试。F5 使用 **XRack: Debug Counter** 构建调试版本并启动 CodeLLDB。保存时不会执行 Cargo 检查。

### 调试生成的 MLIR

安装推荐的 LLVM MLIR 扩展，运行 **Bazel: Build XRack MLIR LSP**。工作区的 `mlir.server_path` 指向 `bazel-bin/mlir/xrack-mlir-lsp-server`。Windows 在该路径末尾加 `.exe`。在工作区打开 dump，MLIR 扩展会启动这个由 Bazel LLVM 构建的服务。

```sh
mkdir -p .tools/ir
bazel run //:xrackc -- --emit mlir examples/counter/counter.xr -o .tools/ir/counter.mlir
```

打开 `.tools/ir/counter.mlir` 可查看组件大纲、SSA 跳转及引用、方言补全和模块级诊断。点击组件、属性、回调和有名称的控件引用可跳到 IR 声明，保留的 `loc` 也可跳回原始 `.xr`。将 `--emit mlir` 换成 `--emit mlir-raw` 可查看优化前的输出。

### 实时预览

在仓库中用 `bazel build //crates/xrack-preview` 构建预览宿主，或安装平台 VSIX。打开 `.xr` 文件后运行 **XRack: Open Live Preview**。宿主会读取当前编辑缓冲区和打开的导入文件，执行校验及优化，再由解释器创建组件。可点击按钮，也可点击文本框后输入或粘贴。预览支持 PNG、SVG、九宫格图像和 GIF 播放。后续编辑复用宿主和 GPU 渲染器，语法错误时保留上一个可交互的画面。使用解释器尚未覆盖的控件、资源或操作时，扩展使用生成 Rust 的预览路径。

解释器预览提供 **Inspect** 按钮。点击可查看当前窗口的场景树、元素 ID、可见边界、禁用状态、当前焦点和主组件的公开属性清单。密码控件的文字不会包含在检查结果中。

Rust 宿主可用 `xrack_interpreter::load_mlir_file` 加载文本 MLIR。图片、字体和翻译清单按 MLIR 文件所在目录解析，Fluent 文件按清单所在目录解析。用 `Program::create` 创建独立实例，再通过 `Instance::get`、`set`、`on`、`invoke` 操作公开接口。动态实例实现 `xrack_runtime::Component`。宿主回调需要访问组件时，可捕获 `Instance::as_weak()` 返回的弱引用，并在执行时用 `upgrade()` 获取仍存活的实例。宿主已有资源时可通过 `Program::from_module_with_assets` 提供。 用 `Program::create_with_locale` 可在创建时传入共享语言环境。

`Instance::string_model`、`record_model` 和 `touch_points_model` 返回共享模型。宿主可以通过 `RecordList::from_model` 包装已有的记录模型，再用 `Instance::set` 绑定到可写属性，保留模型和行身份。
