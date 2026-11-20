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

预览 `SoundEffect` 或 `MediaPlayer` 时，从源码用 `bazel build --config=media //crates/xrack-preview` 构建宿主。平台 VSIX 已内置媒体后端。打开 [SoundEffect 示例](../examples/sound_effect/sound_effect.xr)，等待音效加载后点击按钮，或聚焦预览面板并按 F2。

解释器预览提供 **Inspect** 按钮。点击可查看当前窗口的场景树、元素 ID、可见边界、禁用状态、当前焦点和主组件的公开属性清单。密码控件的文字不会包含在检查结果中。

Rust 宿主可用 `xrack_interpreter::load_mlir_file` 加载文本 MLIR。图片、字体和翻译清单按 MLIR 文件所在目录解析，Fluent 文件按清单所在目录解析。用 `Program::create` 创建独立实例，再通过 `Instance::get`、`set`、`on`、`invoke` 操作公开接口。动态实例实现 `xrack_runtime::Component`。宿主回调需要访问组件时，可捕获 `Instance::as_weak()` 返回的弱引用，并在执行时用 `upgrade()` 获取仍存活的实例。宿主已有资源时可通过 `Program::from_module_with_assets` 提供。 用 `Program::create_with_locale` 可在创建时传入共享语言环境。

`Instance::string_model`、`record_model` 和 `touch_points_model` 返回共享模型。宿主可以通过 `RecordList::from_model` 包装已有的记录模型，再用 `Instance::set` 绑定到可写属性，保留模型和行身份。

### 检查运行中的应用

以热重载模式启动应用，在 VS Code 运行 **XRack: Sync Editor to Running App**。组件树可以查看组件属性、值的类型与可写性，以及记录和模型的采样。属性旁的 **Definition** 跳到声明。**Recent calls** 保留最近完成的 XR 回调和控件处理器，可查看调用链、耗时、参数类型，以及当前源码版本对应的调用位置和定义位置。

用 Bazel 运行仓库异步搜索示例时，可以显式指定会话发现目录。

```sh
XRACK_LIVE_PROJECT="$PWD" XRACK_DEBUG_VALUES=1 bazel run //examples/async_demo:async_demo_live
```

用 `XRACK_DEBUG_VALUES=1` 启动应用可额外捕获回调实参、返回值和已有缓存的私有属性值。未求值或已失效的私有绑定显示为无缓存。调用历史有容量上限。热重载前的帧仍可查看，源码跳转仅开放给当前版本。这个面板展示解释器里的 XR 调用帧。Rust 和 Python 宿主内部的堆栈及断点由各自的调试器提供。

在 macOS 或 Linux 上，可以把同一个开发会话接到 MCP 客户端。配置 `stdio` 启动命令及参数，例如：

```json
{
  "command": "node",
  "args": [
    "/path/to/xrack/crates/xrack-lsp/vscode/xrack-mcp.js",
    "--project",
    "/path/to/workspace"
  ]
}
```

先用 `list_sessions` 选择应用，再用 `get_ui_tree` 或 `get_node` 查值。用 `read_model` 分页读取公开的字符串或记录模型，继续翻页时核对 `modelRevision` 是否一致。用 `get_calls` 和 `get_call_stack` 查最近的调用，用 `get_diagnostics` 核对热重载错误和应用中的源码版本。这些查询工具只读取工作区里正在运行的开发会话。开启值捕获时，请使用适合共享给调试客户端的数据。

### 录制交互和分析更新

启动热重载示例时开启录制和采样。文本录制额外要求 `XRACK_DEBUG_VALUES=1`，会跳过密码输入。

```sh
XRACK_LIVE_PROJECT="$PWD" XRACK_RECORD_UI=1 XRACK_PROFILE_UI=1 XRACK_DEBUG_VALUES=1 \
  bazel run //examples/async_demo:async_demo_live
```

在 **Sync Editor to Running App** 面板点 **Copy Recording**，或用 MCP 的 `get_recording` 导出 JSON。首版记录主窗口中 ID 唯一的按钮点击、文本框的提交文本，以及成功应用的热重载代次。重复行、触摸手势和未命名目标需要单独验证。录制达到上限会标记为截断。在开发应用上额外设置 `XRACK_CONTROL_UI=1`，MCP 可按当前源码代次调用 `click` 和 `fill`。动作通过窗口的命中测试和输入交互处理。用 `wait_for_property` 等待属性值或模型行数达到预期，也可以把断言写进回放。

```sh
bazel test //tests:ui_replay_test
XRACK_REPLAY_FILE=/absolute/path/recording.json \
  bazel run //tests:ui_replay_test -- replay_recorded_actions
```

### 回放异步搜索场景

异步搜索的 [回归场景](../tests/async_search_scene.json) 先后查询 `cloud` 和 `sensor`，再把结果行从 30px 改成 42px。场景先交付请求 2 的结果，然后交付迟到的请求 1。两种后端在每步都对照查询、加载状态与结果。在解释器中，源码版本 1 保存重载后的 42px 布局。Rust AOT 执行相同的输入和业务断言，使用它构建时生成的布局。

```sh
mkdir -p .tools
XRACK_REPLAY_FILE="$PWD/tests/async_search_scene.json" \
XRACK_REPLAY_REPORT="$PWD/.tools/async-search-replay.html" \
  bazel run //tests:ui_replay_test -- replay_recorded_actions --exact
```

用浏览器打开生成的 HTML。拖动滑块或按左右方向键，查看同一步的两个后端、实际物化结果的标签和行高。报告里的列表根据走过布局流程的节点生成，适合核对文本和状态。

展开 **Changes and update counters**，可以把当前操作与前后的 `query`、`loading`、`result-query` 和 `results` 关联起来。报告同时记录该步骤的实际属性求值、缓存命中、无窗口布局次数、行实例化和命名视觉绑定读取。旧请求被拒绝的那一步可以直接检查属性差异为空，且没有重新布局。

**Structured events** 显示同一步收到的 `tracing` 事件。可以沿 `request_id` 查询请求发出与处理时的源码代次、处理结果、宿主调用了哪些属性写入，以及对应的更新计数。写入事件记录属性名和 Rust 调用位置，实际前后值从步骤快照读取。

可从 **Copy Recording** 导出的 JSON 开始制作新场景。将 `version` 设为 `2`，给每次 `complete_search` 指定从 1 开始的请求编号，按想复现的顺序填入 `rows`。用 `expect_searches` 确认请求顺序，再加上 `expect_property` 和 `expect_results` 断言。给 `reload_layout` 写入 `edits` 列表，每项用 `before` 和 `after` 指定一个在当前源码中唯一的替换位置。保存 JSON 即可重新执行，也可以作为测试夹具提交。原来的版本 1 录制文件仍可直接回放。

### 检查实时搜索请求

```sh
XRACK_LIVE_PROJECT="$PWD" XRACK_TRACE_UI=1 bazel run //examples/async_demo:async_demo_live
```

附加运行中的应用后，在 **Request traces** 里选择请求编号，检查后台完成、UI 处理结果和属性写入。终端会输出同一个 `xrack::diagnostics` 事件流。窗口合并多次结果写入时，一条 UI 更新会列出当批次关联的请求编号。该模式也会开启窗口更新采样。事件字段包括请求编号、源码代次、处理结果及写入的属性名和源码位置。

### 查看动效逐帧布局

动效时间轴使用 [AnimationsDemo](../examples/layout/layout.xr) 中的线性条和延迟缓动条。XRack 按 10 毫秒采样过渡，检查两个后端的布局坐标一致，再用 GPU 导出解释器画面。拖动滑块可以查看该时刻的原始渲染和两个元素的布局边界。

```sh
mkdir -p .tools
XRACK_MOTION_REPORT="$PWD/.tools/motion-timeline.html" \
  bazel run //tests:ui_motion_timeline_test -- sample_animation_timeline --exact
```

每个采样帧的表格也会显示插值、布局、绘制与 GPU 读回、PNG 编码的耗时。绘制与读回按整段操作的墙钟时间计，包括 CPU 提交和等待。修改示例中 `animate` 的时长、延迟或 `cubic-bezier` 后，可重新生成报告进行对照。调用常规 `bazel test //tests:ui_motion_timeline_test` 会验证采样坐标。

**UI updates** 和 MCP 的 `get_update_profile` 显示当前源码版本的 MLIR 计划和窗口更新样本。选择节点后可以查看该 ID 对应的编译决策和最近更新中的命名视觉绑定读取、行实例化数。窗口样本还包含属性实际求值数、缓存命中数、布局执行与复用次数，以及更新阶段的 CPU 耗时。视觉绑定读取包含取缓存的调用，CPU 耗时不含 GPU 绘制。列表行计数累加本次布局的实例化工作，同一行可能在多次准备中出现。

也可从编译器直接导出计划，带回源位置按控件核对重绘标记、静态子树和行缓存决策。

```sh
bazel run //:xrackc -- --emit update-plan examples/async_demo/async.xr
```
