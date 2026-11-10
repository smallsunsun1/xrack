# XRack

类似 Slint 的声明式 UI 编译器原型，使用 **ANTLR4 Rust runtime + MLIR + Rust**，以 **Bazel** 为正式构建入口。

生成的组件可以直接运行在 winit 窗口中。布局和输入处理使用逻辑像素，FemtoVG 通过 wgpu 绘制背景、圆角、边框、文本、图像和矢量路径。组件通过 `Node` 场景树和 `Component` 接口与平台解耦。

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
export OUT_DIR="$(pwd)/bazel-bin/mlir/c_bindings_out_dir"
cargo check --workspace --locked
```

Rust 格式检查和 Clippy 使用 Bazel 的 Rust 工具链。Clippy 检查手写 Rust，并将 correctness 类诊断作为构建错误。生成的 UI 代码通过编译器测试验证。

```sh
bazel build --config=fmt //... //tests:gpu_test
bazel build --config=lint //... //tests:gpu_test
```

依赖 MLIR 原生库的可执行文件和测试通过 Bazel 构建。Cargo 可用于运行独立的 parser 测试，执行 `cargo test -p xrack-parser --locked` 前先按上文生成 Rust parser 并设置 `XRACK_ANTLR_DIR`。

## 在 Cargo 项目中使用 XRack

生成的组件只依赖 `xrack-runtime`。需要开原生窗口时，应用另加 `xrack-platform-winit`，并调用 `xrack_platform_winit::run(MyComponent::new())`。桌面宿主会通过 Cargo 拉入布局、渲染和资源 crate。编译器 `xrackc` 是单独安装的宿主工具，生成 Rust 时使用。Cargo 不会构建 LLVM、MLIR 或 ANTLR 生成器。

[Cargo 示例](examples/cargo-consumer) 用只依赖 Rust 标准库的 `build.rs` 调用 `xrackc --cargo ui/counter.xr -o "$OUT_DIR/ui.rs"`。编译器输出 Cargo 的文件变更指令，覆盖入口、间接导入的 `.xr`、内嵌图像和字体。使用同一版本的 `xrackc` 和运行时，示例固定为 `0.1.0`。也可以预生成 `.rs` 并随应用源码提交，此时构建应用无需安装 `xrackc`。

以下命令用本地运行时运行示例，适合 crate 尚未发布时验证。

```sh
bazel build --repo_env=CARGO_BAZEL_ISOLATED=false //:xrackc
export XRACKC="$(bazel info bazel-bin)/crates/xrack-compiler/xrackc"
cargo run --manifest-path examples/cargo-consumer/Cargo.toml \
  --config "patch.crates-io.xrack-runtime.path=\"$(pwd)/crates/xrack-runtime\""
```

### 准备发布包

`python3 tools/package_xrackc.py` 用 Bazel 构建当前宿主平台的 `xrackc`，在 `.tools/dist/` 生成压缩包和 SHA-256 校验文件。解包后将 `XRACKC` 指向其中的可执行文件。压缩包包含编译器及其依赖的许可证。

```sh
cargo package --locked -p xrack-runtime -p xrack-assets -p xrack-layout \
  -p xrack-renderer-femtovg -p xrack-platform-winit
```

Cargo 会验证打包后的源码，并在发布时按依赖顺序上传这些 crate。Cargo 版桌面宿主使用上游 AccessKit，Bazel 版带有 macOS 三态复选框无障碍值修补。

手动运行 [Release XRack](.github/workflows/release-crates.yml) 并输入 workspace 版本号，可执行 dry-run。正式发布时，在 `main` 上选取已合并的提交，推送与 workspace 版本一致的 tag，例如 `v0.1.0`。发布任务先验证 Cargo 包，再运行各平台 CI，核对所有 `xrackc` 归档及校验文件。随后将五个 Rust crate 发布到 crates.io，将 `xrackc` 归档和校验文件附到同一 tag 的 GitHub Release。

在 GitHub 的 `crates-io` 环境配置 `CARGO_REGISTRY_TOKEN` secret，并允许发布 tag 使用该环境。首次发布使用允许发布新 crate 的 token。上传中断后，可从同一次 workflow 运行重试，脚本会跳过同一提交已发布的版本。升版本时同步更新 workspace 版本、内部 crate 依赖、`xrackc` 的 Bazel 版本和 Cargo 消费示例的版本。

### GitHub Actions

[CI](.github/workflows/ci.yml) 在 macOS arm64、Linux x64 与 arm64、Windows x64 与 arm64 上测试 Cargo 桌面依赖栈与 Bazel 编译器。每个平台用归档中的 `xrackc` 运行仓库外 Cargo 示例。成功的任务上传对应的编译器归档和校验文件，Linux x64 任务额外上传通过 Cargo 验证的 5 个 `.crate`。真实 GPU 测试使用本地显式目标 `//tests:gpu_test`。

编译器任务通过 `setup-bazel` 缓存 Bazelisk、依赖下载和编译产物。缓存按操作系统和架构隔离。依赖下载缓存跟踪 Bazel 版本、`MODULE.bazel` 和 `Cargo.lock`，编译产物缓存额外跟踪 Bazel 构建规则。Windows 编译器任务从仓库的 Actions variables 读取上述两个 Microsoft 许可变量。

## VS Code

打开仓库根目录，安装推荐的 Rust Analyzer 和 CodeLLDB 扩展。用 **Terminal → Run Task → Bazel: Generate rust-project.json** 生成本机项目图，或在终端运行：

```sh
bazel run //:rust_project -- //... //tests:gpu_test
```

Rust Analyzer 读取 Bazel 的 crate 图、标准库源码、生成的 Rust 文件和过程宏。项目图包含本机路径，由 Git 忽略。更改 `BUILD.bazel`、依赖或工具链后重新生成；如果编辑器仍显示旧图，运行 **Rust Analyzer: Reload Workspace**。

C++ 使用 clangd。运行 **Bazel: Generate compile_commands.json** 或 `python3 tools/compile_commands.py`，从 Bazel 的编译动作导出 MLIR 桥接和方言源码的参数。脚本会先生成 TableGen 文件，并保留 Bazel 的头文件路径、宏、C++20 和 macOS SDK 参数。数据库包含本机路径，由 Git 忽略。

本工作区从 `PATH` 选择 clangd。macOS 上请使用与当前 Xcode / SDK 兼容的 clangd，可用 `clangd --version` 检查。过旧的 clangd 可能无法解析 SDK 的 libc++ 头文件，即使 Bazel 构建正常也会出现连带诊断。更改 clangd 路径后，重启 VS Code 的 clangd 服务即可重新诊断。

默认构建任务是 **Bazel: Build Counter**；**Bazel: Run Counter** 打开窗口，**Bazel: Test workspace** 运行常规测试，**Bazel: Test native GPU** 运行需要显卡和字体的测试。F5 使用 **XRack: Debug Counter** 构建调试版本并启动 CodeLLDB。保存时不会执行 Cargo 检查。

## Bazel 磁盘缓存

仓库 `.bazelrc` 默认启用 `--disk_cache=~/.cache/bazel/xrack`。此缓存保存构建动作输出，可以跨 Bazel 重启、不同 output base 和兼容的 worktree 复用；它与下载源码包的 repository cache、当前 output base 的增量缓存分开。

本地并发数由 Bazel 根据宿主资源自动确定，需要临时调整时可传 `--jobs=N`。GitHub Actions 为 Linux 和 Windows runner 设置 4 个 job，为 macOS arm64 runner 设置 3 个。

本地回收策略为 `--experimental_disk_cache_gc_max_size=60G` 和 `--experimental_disk_cache_gc_max_age=30d`，给 LLVM/MLIR 及不同目标平台的产物留出空间。GitHub Actions 将 runner 的 Bazel 缓存清理目标设为 100 GiB，实际可用空间由 runner 提供。空间有限时可在命令上覆盖，例如 `--experimental_disk_cache_gc_max_size=30G`。

可用 `bazel info --announce_rc output_base` 检查实际加载的配置。构建摘要里的 `disk cache hit` 表示从共享磁盘缓存取回产物，`action cache hit` 表示当前 output base 已有产物。Bazel 在命中磁盘缓存时也会回放动作保存的编译警告，可用摘要里的 sandbox 执行数确认实际执行量。切换编译模式、目标平台、工具链或编译选项会形成不同的动作缓存键。

GitHub Actions 由 `main` 的 push 构建保存缓存，PR 和发布 tag 可以恢复该缓存。合并后的首次 `main` 构建负责填充缓存。CI 的 Bazel 构建使用默认 `opt` 模式，Cargo 运行时测试和独立消费示例使用 `--release`。

## Crate 分层

| Crate | 责任 |
| --- | --- |
| `xrack-syntax` | AST、类型、源码位置与诊断 |
| `xrack-parser` | 将 ANTLR4 生成的 Rust 规则上下文转换成拥有所有权的 AST |
| `xrack-ir` | 前端符号解析、类型检查、循环绑定检测与 HIR 调试视图 |
| `xrack-mlir` | 将语义分析后的 AST 成员降到 `xr`，运行 MLIR pass 并提供借用原生模块的只读视图 |
| `xrack-codegen` | 读取优化后的 MLIR 操作与 SSA use-def，用 `quote!` 生成 Rust token stream |
| `xrack-compiler` | 编译阶段编排与 `xrackc` CLI |
| `xrack-runtime` | 惰性属性绑定、重复项缓存和 `Node` 场景快照，无 native 编译器依赖 |
| `xrack-assets` | PNG 解码、SVG 栅格化和 SVG 路径命令解析 |
| `xrack-layout` | 逻辑像素布局、容器排布、裁剪、命中测试、鼠标捕获与键盘焦点；文字测量由后端实现 |
| `xrack-renderer-femtovg` | 字体发现与回退、文字测量、FemtoVG 绘制、wgpu 纹理与 PNG 读回 |
| `xrack-platform-winit` | 原生窗口、输入事件、DPI、交换链和按需重绘 |

MLIR 的 native 实现在 `mlir/`。模块及其 context 由 `xrack-mlir` 管理，操作和 SSA 值引用只在模块存活期间有效。

## 语言示例

窗口可设置 `color-scheme: system;` 跟随宿主系统主题，或用 `light`、`dark` 固定应用内容的配色。`accent-color` 可绑定到应用状态，用于按钮、滑条、焦点和选中态。主题同时应用到窗口内的菜单和弹层。控件显式指定的背景和前景会覆盖调色板默认值。省略主题属性时沿用原有配色。运行 `bazel run //examples/controls:controls -- --theme` 可实时切换三种模式。

```text
export component Counter inherits Window {
    in-out property <int> count: 0;
    out property <int> doubled: count * 2;
    callback increment(step: int);
    increment(step) => { root.count += step; }

    width: 320px;
    height: 180px;
    label := Text {
        text: "计数：" + root.count;
    }
    add := Button {
        text: "加一";
        clicked() => { root.count += 1; }
    }
}
```

支持多个组件定义，属性前向引用，`int` / `float` / `bool` / `string` / `length` / `color`，`px` 单位，JSON 风格字符串转义与 Unicode，`#RGB` / `#RGBA` / `#RRGGBB` / `#RRGGBBAA`，注释、括号、算术、比较、逻辑运算与三元表达式。减法建议写为 `a - b`，因为标识符可以包含连字符，`a-b` 是一个完整标识符。

字符串 `+` 支持拼接 `int`、`float` 和 `bool`。类型检查显式插入 `xr.stringify`，Rust 后端在求值时格式化标量。

`root.property` 引用当前组件的根属性，`self.property` 引用当前元素声明的属性，`id.property` 引用指定元素或组件实例声明的属性。无前缀的属性名从当前元素向外查找，回调参数优先。子元素可以声明自己的属性。

同一文件内可以实例化用户定义的组件，例如 `left := Stepper { step: root.step; }`。实例内的绑定可设置 `in` 和 `in-out` 输入，父组件可以读取非私有属性。组件内部可通过事件写自己的 `out`、`in-out` 和私有属性。每个实例有独立的属性状态和事件路径，嵌套实例的按钮可通过 `dispatch("left/add", "clicked")` 调用。双计数器示例演示了父组件读数、惰性输入绑定与实例隔离。

内建元素包括 `Window`、`Rectangle`、`Text`、`Button`、`TouchArea`、`TextInput`、`LineEdit`、`TextEdit`（也可写 `TextArea`）、`CheckBox`、`Switch`、`Slider`、`SpinBox`、`ComboBox`、`RadioGroup`、`TabWidget`、`ListView`、`Dialog`、`DialogButton`、`ProgressIndicator`、`ScrollView`、`VerticalLayout`、`HorizontalLayout`、`VerticalBox`、`HorizontalBox`、`GridLayout`、`Image` 和 `Path`。声明事件处理器的子元素需要显式 ID。

`HorizontalLayout` 和 `VerticalLayout` 支持四边 `padding-*`、`spacing`、`min-*`、`preferred-*`、`max-*`、双轴伸缩与对齐。组件实例可以在使用处声明这些尺寸约束和子项属性，实例上的绑定通过 `xr.layout_bind` 进入场景树。主轴对齐支持 `stretch`、`start`、`center`、`end`、`space-between`、`space-around` 和 `space-evenly`。交叉轴可用 `cross-axis-self-alignment` 覆盖单个子项。`layout-order` 改变视觉位置，键盘焦点按声明顺序移动。

`GridLayout` 支持 `Row`、`row`、`col`、`rowspan`、`colspan` 和分轴间距。尺寸绑定可使用相对父容器内容区域的百分比，例如 `width: 50%;`。`animate width { duration: 220ms; }` 可对相同类型的视觉属性值做线性过渡。

同一过渡可以用 `animate x, width` 应用于多个属性。`duration` 和 `delay` 用毫秒整数声明，默认均为 `0ms`。`easing` 支持 `linear`、`ease`、`ease-in`、`ease-out`、`ease-in-out` 和 `cubic-bezier(x1, y1, x2, y2)`。自定义曲线的两个 x 坐标均须在 0 到 1 之间。`enabled` 可绑定应用状态，关闭后属性直接显示目标值。相同类型且色标数量一致的渐变也可参与过渡。

每个属性保留自己的开始和结束时间。目标改变时从当前显示值起步，其他属性继续原有过渡。窗口在延迟期间可等待下一次唤醒。运行 `bazel run //examples/layout:layout -- --animations`，可对比线性与缓动，在过渡中反转目标或只更新旁边的计数。

`states [ alarm when root.level >= 2: { indicator.width: 280px; } ]` 可在一个状态里覆盖多个视觉属性。每个状态组按声明顺序选择第一个满足 `when` 的状态。没有匹配的覆盖时读取原绑定，即使原绑定依赖的值在状态激活期间变化也会恢复到最新结果。状态覆盖可作用于声明元素及同一组件、同一重复上下文内的后代。

状态可以改变组件实例的 `in`、`in-out` 输入及调用方设置的布局属性，例如 `badge.caption: "告警"; badge.width: 280px;`。输入覆盖参与组件属性的惰性求值，内部控件和外部 getter 会读到相同的值。状态退出时恢复调用方的最新绑定，或使用子组件的默认值。显式写入会接管这个输入，重置后恢复状态绑定。状态可以声明在实例体内，重复实例会按当前行独立选择状态。编译器检查循环依赖和与双向链接的冲突。运行 `bazel run //examples/layout:layout -- --component-states` 查看状态联动和布局转场。

状态内部的 `in`、`out` 和 `in-out` 可以声明 `animate indicator.width { duration: 300ms; }`。转场属性需要一个显式的原绑定。`animate *` 为该状态修改的可插值属性提供默认时序，显式声明优先。同一属性同时命中进入和离开时序时采用进入时序。运行 `bazel run //examples/layout:layout -- --states`，在正常、预警和告警之间切换，观察中途反转与恢复。

`GroupBox` 为控件提供标题和边框，直接子项沿用垂直布局的间距、对齐与尺寸约束。`content-padding` 设置默认内边距，四边 `padding-*` 可单独覆盖。设置 `checkable: true;` 后，标题可用鼠标、键盘或辅助技术勾选，`checked` 可双向绑定，用户切换时触发 `toggled(bool)`。

取消勾选会保留内容和布局，禁用组内输入、快捷键和弹层。普通输入焦点会回到仍可用的分组标题。运行 `bazel run //examples/controls:controls -- --groups` 体验。

`FlowLayout` 按子项的首选尺寸自动换行，设置 `orientation: vertical;` 可改为纵向排列并换列。每一行或列独立应用 `alignment` 和 `cross-axis-alignment`，子项仍可使用尺寸约束、`layout-order` 和 `cross-axis-self-alignment`。用 `horizontal-spacing` 和 `vertical-spacing` 分别控制两个方向的间距，默认沿用 `spacing`。隐藏和零尺寸子项不参与排布。运行 `bazel run //examples/layout:layout -- --flow` 后调整窗口宽度，可观察按钮换行和下方状态文本的位置变化。

`FlexboxLayout` 支持 `flex-direction` 的 `row`、`row-reverse`、`column` 和 `column-reverse`，以及 `flex-wrap` 的 `wrap`、`no-wrap` 和 `wrap-reverse`。`alignment: stretch;` 按主轴伸缩权重分配每行的剩余空间，并遵守最大尺寸。空间不足时按首选尺寸比例收缩，受最小尺寸限制。`cross-axis-line-alignment` 分配行与行之间的空间，`cross-axis-alignment` 和子项的 `cross-axis-self-alignment` 对齐行内元素。`spacing-horizontal` 和 `spacing-vertical` 可分别设定两个方向的间距。Tab 和无障碍按布局方向及 `layout-order` 遍历。运行 `bazel run //examples/layout:layout -- --flex` 体验。

`Label` 默认不自动换行，适合标题和状态文字。`Text` 保留现有的默认换行。两者都支持 `wrap`、`horizontal-alignment`、`vertical-alignment`、`overflow` 和 `max-lines`。`start` 和 `end` 按段落方向解释，`left` 和 `right` 指物理边缘。`overflow: elide;` 按字素簇缩短超宽文字，隐藏后续行时在最后一个完整可见行加省略号。辅助技术仍可读取完整文本。运行 `bazel run //examples/layout:layout -- --labels` 体验。

`StyledText` 接收 `styled-text` 值并复用标签的布局属性。``@markdown("**粗体**、*斜体*和 `xrackc`")`` 在编译时生成文字和字素对齐的样式区间。支持段落、标题、粗体、斜体、删除线、代码和链接。图片和 HTML 会产生诊断。Rust 可用 `StyledText::from_markdown` 解析动态内容，也可用 `StyledText::from_parts` 提供有样式区间和链接的文本。
Markdown 链接用 `[文案](目标)` 声明，默认显示下划线并可配置 `link-color`。鼠标和 Tab / Enter 均可触发 `link-activated(target)`，应用收到字符串后决定如何处理。布局截断和祖先裁剪会同步限制链接的命中范围和焦点。运行 `bazel run //examples/layout:layout -- --styled-text` 体验换行和截断。

窗口可绑定 `default-font-family`、`default-font-size`、`default-font-weight` 和 `default-font-italic`。文字、按钮和输入控件可分别用 `font-family`、`font-size`、`font-weight` 和 `font-italic` 覆盖窗口设置。字体可用系统字体族名或 `monospace`。缺少所选字形时继续用系统回退字体。字重使用 100 到 900，400 为常规字重，0 沿用默认值。可变字体使用支持的字重和斜体轴。布局、GPU 绘制、编辑器光标和无障碍文字范围共用字体选择。运行 `bazel run //examples/layout:layout -- --fonts`，切换等宽字体和字号，观察单行输入和多行换行。

在 `.xr` 文件顶部写 `import "fonts/example.ttf";` 可内嵌本地 TTF、OTF 或 TTC 字体。路径相对于声明所在的 `.xr` 文件，Bazel 目标通过 `xr_ui_library` 的 `assets` 列出字体文件。通过字体内部的字体族名设置 `font-family` 或 `default-font-family`。内嵌字体优先于系统同名字体，TTC 可按字重和斜体选择文件中的字体变体。

编译器校验字体并把字节嵌入生成的 Rust。同一文件在组件导入链中出现多次时只嵌入一次。窗口使用各自的字体上下文，Rust 的离屏渲染也可以选择 `TextSystem::bundled_only()`。运行 `bazel run //examples/layout:layout -- --bundled-fonts` 可查看内嵌的两款字重和一个使用内嵌字体的输入框。

`TextInput` 和 `LineEdit` 提供单行键盘编辑、选区、IME 预编辑和提交。`text <=> root.query;` 与 `checked <=> root.flag;` 分别把文本输入和复选框与可写属性相连。条件子元素用 `if condition : Element { ... }` 表达，隐藏时保留组件的内部状态。重复元素用 `for item[index] in root.items : Text { ... }` 表达，接受整数计数、`[string]` 和命名记录列表，以匿名、无状态的显示模板生成纵向列表。

在单行输入中设置 `input-type: password;` 后，绘制和光标按字素簇使用圆点，IME 预编辑也会遮罩。应用通过 `text` 绑定读写真值。设置 `password-visible: true;` 可显式显示原文，剪贴板复制、剪切仍被禁用，无障碍树仍只发布遮罩后的值和选区。可粘贴到可编辑的密码字段。运行 `bazel run //examples/form:form -- --password` 体验。

`TextInput` 和 `LineEdit` 的 `input-type: number;` 接受 ASCII 数字，`decimal` 允许前置正负号和一个小数分隔符。`decimal-separator` 默认为 `"."`，也可设为 `","`。空文本、单独的符号或分隔符可作为编辑中间态，按 Enter 提交需至少有一位数字。`input-type: search;` 将单行输入向辅助技术标记为搜索框。

单行和多行编辑器可用 `maximum-length` 限制 Unicode 字素簇数，负值表示不限。键盘、粘贴、IME 提交和无障碍替换都校验整次编辑。无效输入会保留原值、选区和撤销记录。应用写入的值保持原样，超长文本可逐步删到限制以内。运行 `bazel run //examples/form:form -- --validation` 体验。

`ScrollView`、`ListView`、`TreeView` 和 `TableView` 在内容溢出时显示可拖动的纵向滚动条。`ScrollView` 和 `TableView` 也提供横向滚动条。拖动滑块定位，点击轨道按视口大小翻页。滚动条覆盖在内容边缘并共享滚轮的滚动位置，表头与表身继续同步。可通过 `vertical-scroll-bar-policy` 和 `horizontal-scroll-bar-policy` 设置各滚动轴的 `as-needed`、`always-on` 或 `always-off`。拖动滚动条保留当前焦点，虚拟视图继续只创建视口内的行。

`ScrollBar { target: canvas; orientation: horizontal; }` 和 `ScrollIndicator { target: canvas; }` 可以在视口外独立布局，`target` 引用同组件中的视口 ID。方向默认为纵向，`policy` 默认为 `as-needed`，也可设为 `always-on` 或 `always-off`。滚动条支持鼠标、触摸、Tab 焦点、方向键、PageUp/PageDown、Home/End 和原生无障碍操作。指示器只绘制滑块，指针事件可以穿过它。内建滚动条、外置滚动条和指示器共享视口偏移，用户移动外置滚动条时照常写回 `content-x/y` 绑定并触发 `scrolled()`。运行 `bazel run //examples/list:list -- --scroll-bars` 体验。

附着模式的 `position` 和 `size` 是只读的归一化视口数据，分别为实际偏移和可见长度占内容总长度的比例。没有溢出时返回 `0` 和 `1`，视口尚未布局或被隐藏时都返回 `0`。滚动条的显示策略和最小滑块尺寸独立于这两个值。可以用这些读数驱动文本、颜色或另一个滚动指示器。编译器会检查它们流入当前布局阶段的尺寸或位置时是否形成反馈。

省略 `target` 即可使用独立滚动控件。`size` 是可见部分占总内容的比例，取值为 `0..1`，`position` 的有效范围为 `0..1-size`。用 `position <=> root.offset` 接收滚动条输入，或者在表达式中读取 `bar.position` 和 `bar.size` 来移动自定义内容或驱动指示器。`size` 改变时，有效位置会重新夹紧，原绑定值会保留。用户移动滑块时会写回位置并触发 `changed(position)`。`step-size` 控制方向键和无障碍增减的步长，默认 `0.1`。两种模式都支持 `minimum-size` 设置最小滑块比例，滚动条可用 `interactive: false` 切换为被动显示。运行 `bazel run //examples/list:list -- --standalone-scroll` 体验。

两种滚动条都支持 `snap-mode: ScrollSnapMode.always` 和 `ScrollSnapMode.on-release`，默认值是 `none`。设置正的 `step-size` 后，滑块拖动按可移动行程的比例吸附，起点和终点始终可达。附着式滚动条的方向键和无障碍增减会把显式 `step-size` 换算为总内容长度的比例，省略时仍以 28 逻辑像素为步长。轨道点击保留翻页行为，滚轮和程序定位继续使用视口偏移。取消拖动时保留已经提交的位置。可在 `--standalone-scroll` 示例中勾选 **Snap on release** 体验。

`ScrollView` 根据直接子项的位置和布局尺寸计算内容范围。`content-width` 和 `content-height` 可以用长度或视口百分比指定最小内容尺寸。子项的百分比尺寸以显式内容尺寸或视口为基准。`viewport-height` 仍可用于既有的最小内容高度绑定。`Shift` 加滚轮可横向滚动，嵌套视图会将各轴剩余滚动量交给外层。运行 `bazel run //examples/layout:layout` 可体验二维画布。

`Flickable` 是可拖动并带惯性的底层视口。它默认裁剪内容，滚动条可通过 `ScrollBar { target: canvas; }` 独立放置。`content-width` 和 `content-height` 给出各轴的显式内容范围，省略时按子项推导。`flick-direction` 可设为 `both`、`horizontal` 或 `vertical`，`interactive: false` 暂停拖动和惯性输入，子控件、滚轮和 `scroll-to()` 仍可操作。`dragging`、`flicking` 和 `moving` 反映当前消费位移的视口。`flicked()` 在用户改变偏移时触发，`cancel-flick()` 停止该视口正在参与的惯性滚动。运行 `bazel run //examples/layout:layout -- --flickable` 体验。

`bounds-behavior` 默认是 `FlickBoundsBehavior.stop-at-bounds`。可分别启用拖动越界 `drag-over-bounds`、惯性越界 `overshoot-bounds`，或同时启用 `drag-and-overshoot-bounds`。嵌套视口先消费合法范围内的位移，剩余部分才交给最外层启用弹性的视口。`horizontal-overshoot` 和 `vertical-overshoot` 是带符号的逻辑像素，`content-x/y` 仍表示合法的内容偏移。设 `bounds-movement: FlickBoundsMovement.stop-at-bounds` 可让内容停在边界，用读数驱动自定义效果。`rebound-duration` 默认 180 毫秒，`rebounding` 和 `moving` 在回弹期间为真。也可调用 `return-to-bounds()` 主动开始回弹。示例中的 **Elastic content** 切换内容是否随越界移动。

`ScrollView` 和虚拟视图可以在回调中调用 `view.scroll-to(96px, 60px)`，参数为从内容原点量起的逻辑像素。列表、网格、树和表格也可用 `view.position-view-at-index(9000, ScrollAlignment.center)`。索引从零开始，树视图使用当前展开后的可见行序列。对齐方式有 `beginning`、`center`、`end`、`visible` 和 `contain`。`visible` 在目标已有一部分可见时保留位置，`contain` 尽量完整显示目标行。

定位命令在回调结束并更新模型和布局后执行，结果限制在可滚动范围内。越界行索引会被忽略，定位本身不改变选择。读取 `content-x` 和 `content-y` 得到内容相对视口的实际位置，向右或向下滚动时为负值。`visible-width` 和 `visible-height` 是只读视口尺寸。表格的视口尺寸以表身为准。

可用 `content-y: root.offset;` 提供响应式初值，或用 `content-y <=> root.offset;` 把滚动位置链接到可写的 `length` 属性。`ScrollView` 和表格的横轴也支持同样的绑定。单向绑定沿用其他可编辑控件的规则，视口首次主动移动后保留自己的位置。双向链接中的属性表示目标位置，各视口独立裁剪，内容尺寸变化本身不会写回。用户滚动和定位命令实际移动视口时，只将移动的轴写回绑定源。绑定纵轴的虚拟视图以目标位置为准，键盘导航可以更新它。运行 `bazel run //examples/list:list -- --linked` 可比较不同内容长度的联动画布。

`scrolled()` 在滚轮、拖动、惯性、键盘或辅助功能实际移动视口后触发。回调可读取布局更新后的 `self.content-x` 和 `self.content-y`。一次斜向滚动对每个移动的视口触发一次，已到边界的视口和程序化定位不触发。运行 `bazel run //examples/list:list -- --position` 可比较手动滚动和按钮跳转时的回调计数。

可读取已布局元素的 `layout-x`、`layout-y`、`layout-width` 和 `layout-height`。这些值使用逻辑像素，位置相对布局父元素。带 `window-bounds-` 前缀的四个值描述变换后的窗口坐标包围盒，包含滚动位移和绘制变换。元素隐藏或离开场景后，`layout-ready` 变为 `false`，几何值清零。

布局结果可用于读取属性、回调、绘制属性及宽高固定的文字标签。普通内容的布局绑定不能依赖布局结果。`PopupWindow` 和 `Tooltip` 的布局可读取普通内容的几何，编译器会沿属性、纯回调和组件输入追踪依赖。自定义宿主可在准备好场景和布局后调用 `LayoutObservers::sync` 发布这些值。运行 `bazel run //examples/layout:layout -- --geometry` 可调整窗口大小并打开随面板尺寸变化的浮层。

`StandardListView` 可直接绑定 `[string]`，也可绑定带字符串字段的记录列表。记录默认读取 `text` 字段，可用常量 `text-role: "label";` 指定其他字段。默认行复用虚拟布局、滚动条和键盘选择，只读取可见项。辅助技术可读取列表总项数和可见项的绝对位置。

`current-index` 支持双向绑定，默认 `-1` 表示未选择。程序改变选中项时，列表会把它滚入视口。手动滚动后会保留位置，直到选择再次变化。设置 `follow-current: false;` 可关闭程序选中时的自动滚动。运行 `bazel run //examples/list:list -- --standard` 体验字符串分类和五万行记录。

虚拟视图可用 `preserve-scroll-anchor: true;` 保留阅读位置。模型插入、删除、排序、树折叠或网格列数变化时，视口优先保留首个可见记录及其相对视口的位置。该记录消失后，尝试保留原来同屏的下一条记录。无可用记录时沿用现有偏移并按新边界裁剪。选中项跟随和显式定位仍可移动视口。绑定了 `content-y` 的轴由绑定源决定，行锚定本身也不会触发 `scrolled()`。默认关闭。运行 `bazel run //examples/list:list -- --anchor` 可比较两种行为。

`GridView` 接收一个 `for` 模板，支持整数、字符串列表和记录模型，也支持使用自定义组件作为单元格。用 `cell-width`、`cell-height` 和 `spacing` 设置单元格，布局从左到右逐行排列，列数随视口宽度变化。只构建与垂直视口相交的网格行。自定义组件的状态按模型记录标识保存，滚出视口或在模型中移动后仍属于原记录。

`SwipeDelegate` 用一个 `SwipeContent` 声明行内容，再用 `SwipeLeft` 和 `SwipeRight` 声明一侧或两侧的操作区。操作区可设宽度，也可按子项尺寸测量。触摸或鼠标从行内容横向拖动时会预览操作区，松手时越过半宽才展开。纵向手势交给外层列表，预览期间操作按钮不能点击。聚焦行后可用左右方向键滑动，Escape 收起。行末的三点按钮也可用鼠标或辅助技术展开和收起，优先展开右侧。`open-side` 可双向绑定 `SwipeSide`，用户提交切换时触发 `changed(side)`。虚拟列表中的展开状态随记录标识保留。运行 `bazel run //examples/delegates:delegates -- --swipe` 体验。

`current-index` 可双向绑定，点击单元格或按方向键、Home / End、Page Up / Page Down 选择。用户改选时触发 `changed(index)`。默认在索引变化或列数变化后滚入选中项，可用 `follow-current: false;` 关闭。辅助技术可访问当前可见项及其内部控件。运行 `bazel run //examples/list:list -- --grid` 体验五万张卡片，勾选收藏后滚动或调整窗口宽度可查看状态是否保留。

滚动容器可用单指拖动内容，桌面也可以按住鼠标中键拖动。快速松手时，内容按松手前的速度继续滚动并减速。手势锁定起始视图，嵌套视图沿用滚轮的逐轴交接规则。短触仍按按钮等控件的点击处理。惯性在边界停下，也可以再次按下停止。

视口可以设置 `mouse-drag-pan-enabled: true;` 启用左键拖动，默认关闭。短按按原控件的点击处理，超过拖动阈值后视口接管，剩余滚动量只交给也启用左键平移的祖先视口。文本选择、滑块、滚动条和拖放仍使用各自的手势。运行 `bazel run //examples/list:list -- --position` 可在画布按钮和列表上试用。

`TextEdit` 支持多行文本、`text <=>`、`edited(string)`、`read-only`、占位文本和 IME。`wrap` 接受 `word-wrap`、`char-wrap` 和 `no-wrap`。Enter 插入换行，Home 和 End 按当前显示行移动，Ctrl 或 Command 加 Home 和 End 移到文档首尾。鼠标拖动或 Shift 加方向键可跨行选择，滚轮浏览内容，编辑和键盘移动时视口跟随光标。

单行和多行文本控件支持鼠标选区和系统剪贴板。Ctrl 或 Command 加 C、X、V 分别复制、剪切、粘贴，只读字段可以复制。Ctrl 或 Command 加 Z 撤销，加 Shift 和 Z 或按 Ctrl+Y 重做。连续输入和删除可以合并，粘贴、剪切及 IME 提交各自形成一笔。历史按编辑控件保存，虚拟列表内按稳定行标识定位。当前文本与历史记录不一致时会重建历史起点，避免撤销覆盖外部更新。剪贴板回执只作用于原来的编辑目标和选区。

回调可以对当前聚焦的文本框调用 `select-all()`、`clear-selection()`、`set-selection-offsets(anchor, caret)`、`cut()`、`copy()`、`paste()`、`undo()` 和 `redo()`。目标可以是单行、多行编辑器、`SearchField` 或可编辑的 `ComboBox`。偏移以 UTF-8 字节计，超界会裁剪，落在字素内部会移到其起点。先调用 `entry.focus()` 再发选择命令，窗口会按顺序应用；工具栏按钮也可用 `focus-on-click: false;` 保留当前选区。剪切和粘贴异步等待系统剪贴板，回执仍须匹配编辑目标、文字和选区。运行 `bazel run //examples/form:form -- --editor-commands` 体验。

混排希伯来文、阿拉伯文、拉丁字母和数字时，点击、左右方向键、选区高亮和 IME 光标都使用实际排字的字形位置。一个逻辑偏移在方向边界可以有两个视觉位置，连续的逻辑选区也可以分成多个高亮段。编辑和剪贴板继续使用原文顺序。多行编辑器上下移动保留屏幕横坐标，软换行后的方向解析以单个视觉行为单位。运行 `bazel run //examples/form:form -- --bidi` 体验。

`Dialog` 是同一窗口内的模态弹层。用 `open <=> root.dialog-open;` 双向绑定打开状态。窗口居中显示对话框，内容按垂直布局排列。`DialogButton` 的 `role` 取 `accept` 或 `reject`，关闭后分别触发 `accepted()` 或 `rejected()`。Escape 默认取消，也可通过 `close-on-escape` 关闭此行为。启用 `close-on-outside` 后，在遮罩上按下并松开会取消。最近打开的对话框接收输入，Tab 在其中循环，关闭后恢复原焦点。

`PopupWindow` 在当前窗口内显示，直接父元素是定位锚点。`placement` 默认是 `below`，也可选 `above`、`left`、`right` 或 `offset`。空间不足时会尝试换到对侧并限制在窗口边缘，`x`、`y` 可提供额外偏移。锚点滚出可见范围时，弹层随之隐藏。`open` 可双向绑定，也可调用 `show()` 和 `close()`，操作会触发 `opened()` 和 `closed()`。

弹层默认非模态，点击外部会收起最上层弹层，并把这次按下交给背景。点直接触发按钮时由按钮处理。设置 `modal: true;` 会阻断背景输入并默认显示遮罩。`focus`、`close-on-outside` 和 `close-on-escape` 可控制焦点及关闭方式。弹层内按 Tab 循环，关闭后恢复此前焦点。运行 `bazel run //examples/dialog:dialog -- --popup` 体验。

`StandardButton` 的 `kind` 使用 `StandardButtonKind`，例如 `kind: save;`。控件有默认英文标签，也可用 `text` 覆盖。独立使用时通过 `clicked()` 处理操作。在 `Dialog` 中，确认和取消类按钮派发 `accepted()` 或 `rejected()`。`apply`、`reset` 和 `help` 分别触发 `applied()`、`reset()` 和 `help-requested()` 并保持对话框打开。`discard` 使用危险样式，明确激活后关闭并触发 `discarded()`。默认 Enter 只走可用的确认操作，丢弃按钮不会自动获得初始焦点。`primary` 强调按钮，并在初始焦点落在按钮区时优先选择可用的确认按钮。

`DialogButtonBox` 接收 `StandardButton`、`DialogButton` 或普通 `Button`。它按角色排列按钮，确认和取消的默认相对顺序随平台变化。空间不足时会换行，Tab 和无障碍按布局顺序遍历。可用 `layout-order` 覆盖单个按钮的位置，或设置 `wrap: false;`。运行 `bazel run //examples/dialog:dialog -- --standard` 体验。

`Drawer` 是贴靠窗口边缘的模态抽屉。`edge` 可取 `left`、`right`、`top` 或 `bottom`，`open` 可双向绑定。设置 `drag-margin: 18px;` 后，可从窗口边缘向内滑动打开，从面板内侧边缘反向滑动关闭。桌面可用中键执行同样的手势，滑动达到阈值后在松手时提交。手势开启触发 `opened()`，手势、Escape 或遮罩关闭触发 `closed()`。遮罩关闭默认启用。抽屉与对话框共用模态栈和焦点恢复，关闭时延迟构建抽屉内容。运行 `bazel run //examples/dialog:dialog` 体验。

`Page` 可以单独使用，也可以放进 `SwipePage` 或 `StackPage`。直接声明可选的 `PageHeader` 和 `PageFooter`，各最多一个。两端占满页宽，默认按内部垂直布局的首选高度排布，也可设置 `height`。普通子项在中间区域按局部坐标布局，`padding` 作用于正文，`spacing` 留出正文与可见页眉和页脚之间的间距。正文可放一个 `width: 100%; height: 100%;` 的 `ScrollView`，滚动时两端保持固定。`title` 为无障碍提供页面名称。Tab 和无障碍按页眉、正文、页脚遍历，各区内部保持声明顺序。运行 `bazel run //examples/navigation:navigation -- --page` 体验。

`SwipeView` 的直接子元素为静态 `SwipePage`。用 `current-index <=> root.page;` 双向绑定声明顺序中的页索引，通过 `orientation` 选择横向或纵向。可用触摸、中键拖动或聚焦后按方向键切页，Home 和 End 跳到首尾可用页。手势在松手时切页，页内同轴滚动优先，滚到边界后下一次滑动可以切页。`interactive: false;` 暂停手势和键盘切页，应用仍可写入页索引。

隐藏或禁用的页面在导航中跳过。索引不可用时显示首个可用页，用户切页后才写入新索引并触发 `changed(index)`。只有当前页的内容参与场景求值，页面里的组件状态会保留。运行 `bazel run //examples/navigation:navigation` 可体验。

`PageIndicator` 用 `count` 和 `current-index` 表示页数与当前页。它可以独立使用，也可以和 `SwipeView` 双向绑定到同一个页索引。设置 `interactive: true;` 后可点击点位，或聚焦后用方向键和 Home / End 切页，用户切页触发 `changed(index)`。`orientation` 控制横排或竖排，`dot-size`、`spacing`、`color` 和 `current-color` 控制外观。空间不足时，点位围绕当前页显示一段连续索引，最多绘制 64 个点，两端缩小的点提示还有页。点击仍使用点位对应的原始页索引。

`StackView` 的直接子元素为带 ID 的静态 `StackPage`。例如 `home := StackPage { ... }` 的路由是 `"home"`。`initial-page` 设定初始路由，默认使用首个页面。事件处理器可调用 `navigation.push("details")`、`pop()`、`replace("summary")`、`pop-to("home")` 或 `reset("home")`。导航经 `xr.navigate` 校验，成功时触发 `changed(page, depth)`。同一路由在栈中占一个位置，根页可用 `reset` 更换。Rust 可通过 `navigate_stack(id, action)` 和 `stack_snapshot(id)` 操作和读取导航栈。

`navigation.current-page`、`navigation.depth` 和 `navigation.can-go-back` 是可在绑定及纯回调中读取的实时状态。读取经 `xr.stack_read` 保留在 MLIR 中，源码语义检查会拒绝 `initial-page` 经属性或回调形成的初始化环。一次导航前后的读取各自获取当时的栈状态。

`navigation.reset-to(["home", "details", "preview"])` 用完整路径恢复导航栈，`push-pages([...])` 则把一段路径追加到现有栈。路径可以来自 `[string]` 属性。整段路径都有效且路由唯一时才提交一次并触发一次 `changed`，当前只求值最终的栈顶页面。

只生成栈顶页面的场景，已声明页面的组件状态在返回时可继续使用。焦点位于导航栈内时，切页会记住原页面的焦点。Alt+左方向键或系统返回键可返回上一层。返回请求携带场景中的栈版本，提交时会核对实时版本。运行 `bazel run //examples/navigation:navigation -- --stack` 体验。

在 `StackView` 设置 `transition-duration: 220;` 可启用 220 毫秒的横向切页过渡。返回到当前路径上的祖先页面时从左侧进入，其他切页从右侧进入。离场页沿用已布局的绘制内容，栈顶状态和键盘焦点立即切换。过渡期间暂停该区域的鼠标与触摸操作，连续导航会从正在显示的页面片段开始下一次过渡。默认立即切页，时长为零或负数时也是如此。过渡期间视口几何变化会直接显示当前页。

`Slider` 的 `value` 可双向绑定到 `float` 属性。鼠标拖动连续更新数值，方向键按 `step` 调整，Home 和 End 移到端点。`changed(value)` 只在交互写入新值时调用，`released(value)` 在鼠标或调节键松开后调用。`orientation: vertical;` 将最小值放在下方。`Switch` 支持 `checked` 双向绑定与 `toggled()`。`ProgressIndicator` 接收 `0..1` 或百分比进度，`indeterminate: true;` 显示运动指示条。

`Spinner` 使用圆形轨道显示同样的 `progress` 和 `indeterminate` 状态。`BusyIndicator` 在 `running: true;` 时显示旋转的弧线，停止时隐藏并保留布局占位。三种指示器都支持 `color`、`track-color` 和 `animated: false;`，关闭动画会保留可见状态和无障碍进度。圆形指示器还支持 `stroke-width`。

运行 `bazel run //examples/controls:controls -- --indicators` 比较三种指示器，或运行 `bazel run //examples/async_demo:async_demo` 查看与后台任务绑定的加载状态。离屏验证可用 `Renderer::snapshot_at` 指定指示器的采样时间。

`CheckBox` 支持布尔 `checked`，也可使用类型化的 `check-state`。三种状态是 `CheckState.unchecked`、`CheckState.partially-checked` 和 `CheckState.checked`。设置 `tristate: true;` 后，用户交互会循环三种状态。默认交互会把部分选中切到已选中。三态模式选择 `check-state` 或 `tristate`，布尔模式使用 `checked`。

`check-state: expression;` 保留单向绑定，交互通过 `check-state-changed(state)` 报告请求的状态，适合汇总子项选择。`check-state <=> root.state;` 直接写入可写属性，也可绑定表格或树行的 `CheckState` 字段。运行 `bazel run //examples/controls:controls -- --check-state` 体验全选和独立三态循环。

`RangeSlider` 使用 `lower-value` 和 `upper-value` 选择一个区间，两端可分别双向绑定到不同的 `float` 属性。Tab 在两个手柄之间移动焦点，方向键按 `step` 调整，Home 和 End 移到当前手柄可达的边界。点击轨道移动最近的手柄。手柄相遇时，向较小值拖动下限，向较大值拖动上限。横向和纵向都使用 `changed(lower, upper)` 和 `released(lower, upper)` 报告有效区间。外部写入交叉值时，显示的下限收拢到上限，下一次有效交互会写回有序区间。运行 `bazel run //examples/controls:controls` 体验。

`Dial` 的 `value` 同样可双向绑定。输入模式使用 `DialInputMode`，可取 `circular`、`horizontal` 或 `vertical`。圆形模式按角度定位，直线模式按水平或垂直位移相对调节。方向键按 `step` 调节，Home 和 End 移到端点。默认使用 280 度限位圆弧，`wrap: true;` 使用整圈并允许跨过端点。可用 `start-angle` 和 `end-angle` 自定义以十二点方向为零、顺时针递增的角度。事件 `changed(value)` 和 `released(value)` 与 `Slider` 相同。

`SpinBox` 的 `value` 是可双向绑定的整数。它支持 `minimum`、`maximum`、`step-size`、`wrap` 和 `read-only`。方向键和步进按钮立即提交，直接输入的数字在 Enter 或移开焦点时提交，Escape 丢弃草稿。交互改值触发 `edited(int)`。

`DoubleSpinBox` 使用相同的交互，`value` 和步进范围使用 `float`，改值触发 `edited(float)`。`decimals` 指定 0–15 位小数，默认 2 位，步进与提交都按此精度舍入。`decimal-separator` 可设为 `"."` 或 `","`，显示和输入使用同一分隔符。编辑期间外部更新数值会使旧草稿失效。在表格和树视图里可双向绑定可写记录的浮点字段，提交时保留同一行其他字段的更新。运行 `bazel run //examples/controls:controls -- --decimal-spin` 体验。

`Tumbler` 用 `model: [string]` 显示标签，或用 `count` 生成从 0 开始的数字。选中项居中，`current-index` 可双向绑定，用户提交时触发 `changed(index)`。`visible-item-count` 取 1–15 之间的奇数，默认 5。默认在模型项多于可见项时循环，也可显式设置 `wrap`。

鼠标或触摸拖动时预览，松手吸附并提交。滚轮按项移动，触控板像素滚动累积到一行后提交。方向键、Home、End 和 Page Up / Page Down 支持键盘选择。字符串模型只读取视口内的标签，模型重排或替换会使旧手势失效。运行 `bazel run //examples/controls:controls -- --tumbler` 体验小时和分钟选择。

`DelayButton` 在鼠标、触摸或键盘持续按住达到 `delay` 毫秒后设置 `checked`，并触发一次 `activated()`。默认延时 3000 毫秒。进度颜色可用 `progress-color` 设置，`checked` 可双向绑定。松手、移出、失焦或禁用会取消仍在等待的确认。已确认的按钮再次点击时复位。

通过辅助技术操作时，第一次激活开始等待，提示就绪后第二次激活提交。提前再次激活或按 Escape 会取消。运行 `bazel run //examples/controls:controls -- --delay` 体验。

`ComboBox` 从 `[string]` 模型选择一项，可绑定 `current-index` 或 `current-value`，用户选择时触发 `selected(string)`。按值绑定取首个匹配项。弹层在窗口顶层绘制，只绘制当前可见行，支持滚轮、方向键、Home、End、PageUp、PageDown、Enter 与 Escape。点击弹层之外会关闭弹层并消费本次点击。

记录模型可用 `text-role` 指定显示字段，用 `value-role` 指定业务值字段。角色由编译器校验，`current-value` 和 `value-selected` 采用该字段的类型，支持字符串、整数、浮点数、布尔值、`Date` 和 `Time`。`selected(string)` 返回显示标签。按值选择会在模型重排后查找相同业务值，弹层高亮随行标识移动。可编辑的记录下拉框按显示标签匹配，提交未匹配的自由文本时可在 `accepted(string)` 中处理。运行 `bazel run //examples/selection:selection -- --record-combo` 体验。

记录下拉框可以用 `enabled-role: "access.available";` 指定布尔字段。禁用项仍显示，鼠标、键盘、补全和辅助技术会跳过它，提交时重新核对当前模型。程序可保留指向禁用项的当前值。非编辑下拉框支持按前缀定位，短时间连续输入会合成前缀，重复按同一个字符会在匹配项之间循环。弹层关闭时直接选中，打开时只移动高亮，按 Enter 才提交。

`editable: true;` 让 `ComboBox` 的文字区域支持输入、IME、剪贴板和撤销。草稿可用 `edit-text <=> root.draft;` 双向绑定，修改时触发 `edited(string)`。点击箭头或按 F4 可打开列表，列表优先定位到与草稿精确匹配或具有相同前缀的项。关闭列表后按 Enter 提交草稿并触发 `accepted(string)`。精确匹配时更新选项，自定义文本以 `current-index: -1` 表示或写入 `current-value`。运行 `bazel run //examples/selection:selection -- --editable-combo` 体验。

`auto-complete: true;` 可在编辑区显示模型中首个前缀匹配项的后缀。光标位于草稿末尾、没有选区且 IME 未预编辑时才显示。已有精确匹配时保留当前文字。按右方向键插入可见后缀并形成一笔可撤销的编辑，按 Escape 收起提示。模型变化会使过期候选失效。按 Enter 仍提交草稿，Tab 仍切换焦点。

`SearchField` 支持 `text` 双向绑定，搜索图标提交查询，清空图标和选中候选都可撤销。候选由应用通过 `suggestions` 提供，类型为 `[string]`。可用 `suggestions-for` 标记生成这批结果的查询，只有标记与当前输入一致时才显示。方向键高亮候选，Enter 选中高亮项或提交当前查询，Escape 收起候选。候选变化使旧的高亮失效。

`accepted(string)` 报告显式提交的查询，`selected(string)` 报告选中的候选。两种操作都触发 `search-triggered(string)`。`live: true;` 也会在输入变化时触发它。`bazel run //examples/async_demo:async_demo` 演示用 Tokio 查询并通过 `UiSender` 返回候选，示例单独维护一批候选和虚拟化的结果列表。

`RadioGroup` 接收静态 `RadioButton`，也可以用一个 `for` 生成所有选项。通过组的 `current-index` 或 `current-value` 双向绑定选择，按钮的选中状态由组推导。组支持横竖排列、方向键导航和 `selected(string)`。

组件根部也可声明非可视的 `ButtonGroup`，各个 `RadioButton` 用 `group: group-id;` 加入同一组，可分别放在不同的行、列或条件容器中。`current-index` 按声明顺序分配槽位，隐藏项保留槽位。`RadioButton.value` 可提供稳定的业务值，`current-value` 和 `selected(string)` 使用该值，省略时沿用 `text`。每组在 Tab 顺序中只占一站，方向键在可用选项间循环。设置组的 `enabled: false;` 停用整组，`accessible-name` 会用于无障碍分组。运行 `bazel run //examples/selection:selection -- --radio-groups` 体验。

`ButtonGroup { exclusive: false; }` 可组合 `CheckBox` 和 `Switch`。全选框用 `check-state <=> options.check-state;` 绑定汇总结果，普通成员各自保留 Tab 停靠点。汇总使用声明中的所有成员，包括隐藏或禁用的选项。`options.set-check-state(CheckState.checked)` 和 `options.set-check-state(CheckState.unchecked)` 分别批量选中和清除，传入部分选中值会保留当前成员状态。批量操作通过各成员的绑定更新属性，成员的用户操作回调由各自的交互触发。运行 `bazel run //examples/controls:controls -- --check-group` 体验。

`TabWidget` 接收静态 `Tab` 子元素，可双向绑定 `current-index`，并通过 `orientation` 切换顶部或左侧标签栏。非活动页面保留组件状态，只有活动页构建内容场景。方向键跳过禁用项，Tab 键把每组选项视为一个焦点停靠点。

`TabBar` 独立排列 `TabButton`，可与 `SwipeView` 双向绑定同一个 `current-index`。标签可逐项声明，也可用一个 `for` 从模型生成。横排默认按文字测量宽度，可用 `width` 固定某项。竖排使用 `height`。超出视口时可用滚动条、触摸或沿排列方向滚动浏览。方向键和 Home / End 跳过隐藏或禁用项，绑定变化后会把新的选中项滚入视口。隐藏的标签保留声明索引。运行 `bazel run //examples/navigation:navigation -- --tab-bar` 体验。

`Calendar` 的 `date` 和 `display-month` 使用 `Date { year: 2024, month: 2, day: 29 }`，可分别双向绑定。点击日期或按方向键选择，Home / End 跳到该月可用的首尾日期。翻月按钮和 Page Up / Page Down 更新显示月份并触发 `month-changed(month)`。选中日期变化时触发 `changed(date)`。

日历使用公历，年份范围为 1–9999，`minimum-date` 和 `maximum-date` 可进一步限制选择。默认日期是 `1970-01-01`，应用可传入当天或业务日期。`week-start` 按周一为 1 到周日为 7 编号，`weekday-labels` 接收从周一起排列的自定义星期名。

`DatePickerPopup` 通过 `open` 打开，复用 `Calendar` 的日期范围、星期设置和键盘导航。打开时从 `date` 读取独立草稿，翻月只改变视图。确认时按当前范围调整草稿，写回双向绑定并触发 `accepted(date)`，取消或按 Escape 触发 `canceled()`。`title`、`accept-text` 和 `cancel-text` 可自定义。运行 `bazel run //examples/controls:controls -- --calendar` 体验内联日历和日期弹层。

`TimePicker` 和 `TimePickerPopup` 使用 `Time { hour: 23, minute: 58, second: 41 }`。字段省略时默认为零。构造时小时限制到 0–23，分秒限制到 0–59。`time` 可双向绑定，内联滚轮改值后触发 `changed(time)`。默认显示 24 小时制，设置 `use-24-hour-format: false;` 可显示 12 小时制和 AM/PM。`show-seconds: true;` 显示秒，隐藏时保留原值。滚轮支持鼠标、键盘和辅助技术操作。

`TimePickerPopup` 通过 `open` 打开，打开时读取一次 `time` 作为草稿。确认后写回并触发 `accepted(time)`，取消或按 Escape 触发 `canceled()`。可用 `title`、`accept-text` 和 `cancel-text` 自定义文字。运行 `bazel run //examples/controls:controls -- --time` 体验。

`ColorPickerPopup` 用 RGBA 滑条和 0–255 数值输入编辑 `color`。草稿会更新渐变轨道、棋盘格预览和十六进制显示。`show-alpha: true;` 显示透明度控件，隐藏控件时保留原有 alpha。`open` 和 `color` 支持双向绑定，确认时写回颜色并触发 `accepted(color)`，取消或按 Escape 丢弃草稿并触发 `canceled()`。运行 `bazel run //examples/controls:controls -- --color` 体验。

在组件根部声明命名的 `FileDialog`，从回调调用 `picker.open()`。`mode` 可取 `open-file`、`open-files`、`save-file` 或 `select-folder`。可设置 `directory`、`file-name` 和 `name-filters`，扩展名过滤用 `"Text files (*.txt *.md)"` 这样的形式。宿主异步显示系统面板，并在 UI 线程触发 `accepted([string])`、`canceled()` 或 `failed(string)`。应用收到 UTF-8 本地路径后自行读写文件。同一窗口串行显示面板，同一个 `FileDialog` 的请求未结束时会合并重复调用。发起组件离开活动场景后，宿主丢弃该请求的结果。运行 `bazel run //examples/form:form -- --files` 体验。

`MessageDialog` 也通过 `open()` 打开系统对话框，可设置 `title`、`text`、`informative-text` 和 `level`。`buttons` 可取 `ok`、`ok-cancel`、`yes-no` 或 `yes-no-cancel`。确认和拒绝分别触发 `accepted()` 与 `rejected()`，随后 `finished(StandardButtonKind)` 报告具体结果。系统关闭或取消返回 `cancel`，宿主检测到的打开错误通过 `failed(string)` 返回。文件面板与消息对话框按请求顺序共用窗口的异步队列。运行 `bazel run //examples/form:form -- --message` 体验。

`SplitView` 把直接子元素排成横向或纵向面板，支持嵌套。面板沿分栏方向使用 `min-width`、`preferred-width`、`max-width` 或对应的高度属性。拖动分隔线只调整相邻的可见面板，并在当前实例中保存比例。用 Tab 聚焦分隔线后，可用方向键步进，Home 和 End 到达两个边界。`handle-size` 和 `handle-color` 设置分隔线，`resized()` 在比例更新后触发。隐藏面板不占空间，再显示时会恢复其比例。

`MenuBar` 和 `ContextMenuArea` 分别通过顶部菜单和右键打开 `Menu`。菜单支持嵌套子菜单、禁用项、分隔线和可勾选的 `MenuItem`。`activated()` 在选中可用菜单项时触发。方向键导航，Escape 逐层关闭。右键菜单也可用 Shift+F10 打开。

单行、多行、搜索框、可编辑下拉框、数字输入框和正在编辑的表格或树单元格提供默认编辑菜单。右键、Shift+F10 或辅助技术均可打开。菜单与键盘共用选区、剪贴板和撤销记录。只读框可复制，密码框禁用复制和剪切。数字框的撤销和重做作用于草稿，提交时再更新绑定的数值。`ContextMenuArea` 优先，`context-menu-enabled: false` 可关闭默认菜单。菜单打开后，外部修改当前文本或选区会使旧操作失效。运行 `bazel run //examples/form:form -- --editor-menus` 体验。

`MenuItem` 的 `shortcut: "Primary+Shift+E";` 会在菜单中显示组合键，并与点击共用 `activated()`。`Primary` 在 macOS 上表示 Command，在其他平台表示 Ctrl。也可以组合 `Ctrl`、`Alt`、`Shift` 和 `Super`，主键支持英文字母、数字和 F1 到 F12。字母和数字需要 Ctrl、Alt、Super 或 Primary 中的一个修饰键。无效字符串不会注册快捷键，可将绑定设为空字符串清除快捷键。

菜单栏快捷键在当前窗口或最上层模态区域内生效。打开右键菜单时，只处理该菜单内的快捷键。隐藏和禁用项不参与匹配，同一作用域内的重复组合键不会触发命令。文本编辑保留选择、剪贴板和撤销重做快捷键。运行 `bazel run //examples/menu:menu` 可体验。

`Shortcut` 可声明在布局或对话框内，不占空间。用 `sequence: "Primary+K, Primary+P";` 声明最多四段组合键，相邻两段须在一秒内输入。`sequences` 可提供多个别名，`focus-within: true;` 将范围缩到父元素的焦点子树，`auto-repeat: true;` 允许单段快捷键随按住重复触发。不同命令匹配同一序列时，独立 `Shortcut` 会收到 `activated-ambiguously()`。短序列同时是长序列的前缀时，短序列报告冲突，长序列可以继续输入。用 `action` 复用命令时，未指定序列会采用命令的 `shortcut`，同一命令的多个入口只执行一次。运行 `bazel run //examples/menu:menu -- --shortcuts` 体验。

在组件根部声明命名的 `Action` 后，`Button`、`ToolButton` 和 `MenuItem` 可以通过 `action: command-id;` 共享命令。命令集中声明 `text`、`icon`、`shortcut`、`enabled`、`checkable`、`checked` 和 `activated()`。可勾选命令先切换 `checked` 再执行回调，也可用 `checked <=> root.some-flag;` 关联组件属性。控件可以覆盖显示文本和图标，也可额外限制自身的 `enabled`。同一命令的多个控件共用一个快捷键，按钮单独展示时也可以触发命令。每个组件实例独立持有命令状态。

`ToolBar` 默认横向排列子项，可用 `orientation: vertical;` 改为纵排。设置 `wrap: true;` 后可随可用空间换行或换列。工具按钮支持独立的 `clicked()`，也可绑定命令以同步勾选状态。`Button` 和 `ToolButton` 可用 `icon-size` 和 `icon-only` 控制图标显示。纯图标模式在没有图标时回退到文字。`icon` 使用已声明的 PNG 或 SVG 资源。运行 `bazel run //examples/menu:menu` 可体验共享图标和工具栏换行。

在控件或视觉元素内声明 `Tooltip { text: "操作说明"; }`，可在悬停或通过 Tab 聚焦后显示提示。每个元素可附加一个 `Tooltip`，内容可用纯文本或单个自定义布局。`delay` 默认 600 毫秒，`timeout` 默认持续到离开、失焦或发生输入。提示浮在窗口内，靠近边缘时会避让，点击会穿过浮层。菜单和模态作用域会限制提示显示。

`TableView` 使用静态 `TableColumn` 和一个 `for … : TableRow`。每行按列顺序声明一个展示单元格，支持文本、图像及嵌套布局。列宽支持 `width`、`min-width`、`preferred-width`、`max-width` 和 `horizontal-stretch`。超宽时可用触控板或 Shift+滚轮同步滚动表头和单元格。表头固定在垂直滚动区域上方，只创建可见的行。`current-row` 支持双向绑定，鼠标选择或键盘导航触发 `current-row-changed(int)`。可排序表头交替发出 `sort-ascending(int)` 和 `sort-descending(int)`，应用可用 `VecModel::sort_by` 稳定排序并保留行标识。

表格可用 `movable-columns: true;` 开启表头拖动，默认关闭。拖到目标列的前半或后半时会显示插入位置，松手后提交。靠近表头可见边缘时会自动横向滚动。列宽拖柄和短按排序仍按原操作处理。回调可调用 `table.move-column(0, 2)` 移动显示位置，两个参数都是从零开始的显示索引。`column-moved(column, position)` 给出被移动的声明列索引和新的显示索引。排序、单元格编辑和既有选区保留声明列，键盘导航、Shift 选区和无障碍阅读使用显示顺序。运行 `bazel run //examples/table:table -- --standard` 可试用表头拖动和程序化移动。

表格支持 `frozen-columns: 1;`。它冻结当前显示顺序中的前 N 列，列重排后按新顺序重新计算。冻结列随整表纵向滚动，横向滚动条和翻页只覆盖后面的滚动区。两区共享行标识、选区和编辑状态。窗口较窄且内容溢出时，布局减少实际冻结列数，为滚动区保留空间。默认值为 0。运行 `bazel run //examples/table:table -- --frozen` 可试用，Shift 加滚轮可横向滚动。

表格可用 `resizable-rows: true;` 开启拖动行缝调整行高，默认关闭。回调也可调用 `table.set-row-height(20, 64px)`，按当前模型索引指定行高。行高限制在 12–512 个逻辑像素，传入负长度可恢复该行的默认 `row-height`。调整后的尺寸跟随记录标识，插删和排序后仍属于同一记录，记录移除时清理。列表虚拟化、键盘翻页、行锚定和按索引定位共用这份几何数据。运行 `bazel run //examples/table:table -- --rows` 可试用。

基于记录模型的 `ListView`、`StandardListView`、`TreeView`、`TableView` 和 `StandardTableView` 可以用 `row-height-field: "height";` 指定行高字段。字段名须为常量，字段类型须为 `length`。非正或非有限值使用默认 `row-height`，正值裁剪到 12–512 个逻辑像素。用户拖动或命令设置的表格行高优先，清除覆盖后恢复记录中的最新高度。字段修改只更新对应记录的投影，树按展开后的顺序计算几何。运行 `bazel run //examples/table:table -- --row-field` 可查看共享模型的表格与列表，并对比记录修改和手动覆盖。

`StandardTableView` 可直接绑定记录列表，例如 `model: root.inventory; TableColumn { title: "设备"; field: "name"; }`。`field` 是编译期校验的字段名，支持字符串、整数、浮点数、布尔值、`Date` 和 `Time`。控件为可见行生成只读文本单元格，复用固定表头、列宽调整、选择和排序回调。默认将程序选中的行滚入视口，可通过 `follow-current: false;` 关闭。辅助技术可读取可见的表头、行列位置和单元格值。运行 `bazel run //examples/table:table -- --standard` 体验五万行库存表。

设置 `resizable-columns: true;` 后，可拖动表头分隔线调整列宽。可在单列设置 `resizable: false;` 锁定分隔线。手动宽度保存在当前表格实例，受声明的最小、最大宽度限制，排序和替换行模型时仍然有效。

`multi-select: true;` 启用行多选。Shift+点击或 Shift+方向键选取范围，Ctrl/Cmd+点击或 Space 增减一行，Ctrl/Cmd+Shift 增加范围。Ctrl/Cmd+A 全选，Escape 清空，Ctrl/Cmd+方向键只移动 `current-row`。排序和插入保留已选行的标识，删除会移除对应的选择，替换模型会清空选择。`selection-changed(int)` 报告用户操作后的选中数量。Rust 宿主可通过场景节点的 `table_selection` 查询当前选择，使用 `keys()` 获取行标识或 `indices(&model)` 按模型顺序取得索引。

`selection-target` 接受 `rows`、`cells` 和 `columns`，默认按行选择。单元格模式下，Shift 扩展矩形选区，方向键移动焦点并滚动到当前单元格。列模式可点击表头或表身选择整列，Alt+点击表头排序。`multi-select` 在三种目标下都可启用。`selection-changed(int)` 按当前目标报告行、单元格或列数。Rust 可读取 `current_column()`、`columns()` 或 `cell_keys(column)`。单元格选择跟随记录标识，整列选择在替换行模型后保留。

用 `bazel run //examples/table:table -- --cells` 或 `bazel run //examples/table:table -- --columns` 体验两种选择方式。

表格同时设置 `multi-select: true;` 和 `drag-select: true;` 后，可从表身用鼠标拖出选区。预览随 `selection-target` 显示行、单元格或列范围，靠近边缘时会继续滚动到离屏内容。松手后一次提交，按住 Ctrl 或 Command 可保留原选区。Escape、移出窗口、失去焦点或拖动期间改变行结构和列顺序会取消本次选择。已进入编辑状态的单元格和独立的交互控件仍处理自己的拖动。

表格设置 `copyable: true;` 后，可用 Ctrl 或 Command 加 C 复制选区到系统剪贴板。复制按当前显示列顺序读取模型，包括离屏的已选行和列。单元格选区输出最小矩形，选区内的空洞保留为空字段。文本中的制表符、换行和引号会转义。自定义表格默认复制直接文本和双向绑定的模型值，也可在直接单元格上用 `clipboard-text` 指定导出文本。编辑中的文本选区优先。一次复制最多 100 万个字段或 8 MiB。

`TableCell { text <=> item; }` 可编辑 `for item in root.records` 中可写 `[string]` 模型的一行。双击单元格，或在聚焦后按 Enter / F2 开始编辑。Enter 提交，Tab 提交并移动焦点，Escape 或切换到其他控件会丢弃草稿。应用在编辑期间重排模型时，仍可见的记录会按行标识保留草稿。滚出视口、删除记录或外部改值会结束编辑。文本输入、IME、剪贴板和撤销在草稿上工作，提交时才修改模型。

结构化行可用 `export struct StockRow { name: string, units: int }` 声明，`property <[StockRow]>` 接收 `StockRow { name: "交换机", units: 12 }` 等记录字面量。在 `TableRow` 的直接子元素中，`TableCell { text <=> row.name; }` 编辑字符串，`SpinBox { value <=> row.units; }` 编辑整数，`Slider { value <=> row.load; }` 编辑浮点数，`CheckBox { checked <=> row.available; }` 或 `Switch` 编辑布尔值。提交核对行标识和被编辑字段的原值，再把改动合并进最新记录。Rust 使用生成的 `StockRow` 和 `VecModel<StockRow>`。运行 `bazel run //examples/table:records` 可体验独立字段编辑和按类型排序。

记录字段可使用其他命名记录类型，类型之间不能形成循环。表格和树编辑器可以双向绑定 `row.stock.units` 这样的字段。`TableColumn.field`、`text-role`、`value-role` 和 `row-height-field` 接受点分路径，例如 `"stock.units"`。树的 `key` 和 `parent-key` 可引用嵌套的整数。粘贴解析器返回整段子记录时，撤销历史仍按实际变更的字段保存。运行 `bazel run //examples/table:records -- --nested` 可查看同一库存模型在表格、列表、下拉框和树中的投影。

`TableRow` 与 `TreeRow` 可以在静态子路径使用匿名组件，例如 `NameEditor { value <=> row.name; submitted => { root.save(); } }`。组件只在记录进入视口时创建，滚动、排序和树的折叠保留同一记录的局部状态，删除记录或替换模型会释放对应实例。点击表格单元格内的控件时，表格先更新选区，控件继续处理本次点击。无障碍树公开行列坐标以及编辑器和按钮自己的角色。

直接作为表格单元格的组件可设置 `clipboard-text: row.name;`，从模型复制可见和离屏记录。表格启用 `pasteable` 后，自定义编辑器可用 `clipboard-target: "value";` 指定绑定到当前模型行的 `in-out` 输入，再用 `clipboard-paste(text) => { … }` 返回包含 `accepted: bool` 和同类型 `value` 的记录。每列可在静态子路径配置一个解析器。表达式可读取外层组件属性、当前记录和索引，也可调用纯回调。所有字段通过校验后才一起提交，解析覆盖离屏行。未配置解析器的自定义单元格会使整次粘贴取消。

运行 `bazel run //examples/table:records -- --delegates`，在表格焦点下可粘贴名称、数量和可用状态。数量解析示例同时接受 `12` 与 `12件`。给一行加星后按表头排序，或编辑名称并按 Enter 提交，可体验自定义编辑器的交互。

`TableView` 可用 `pasteable: true;` 开启 Ctrl/Cmd+V 批量粘贴。选中单元格后，从该格沿显示列顺序写入 TSV，行选区从当前行的第一显示列开始。数据通过单元格的双向绑定写入，支持离屏行和包含制表符、换行、双引号的文本字段。数字使用十进制文本，布尔值接受 `true`、`false`、`1` 或 `0`。整块数据校验成功后才写入模型，保留行标识。只读、禁用、隐藏、类型不匹配、越界和过期剪贴板回执会取消整次粘贴。正在编辑文字时，快捷键交给文本编辑器。运行 `bazel run //examples/table:records -- --cells`，选中名称或负责人单元格后可粘贴多行数据。

表格聚焦时用 Ctrl/Cmd+Z 撤销一整次粘贴，用 Ctrl/Cmd+Shift+Z 或 Ctrl+Y 重做。记录按行标识和实际变更字段保存，重排、滚动和列移动后仍可重放。每次重放先核对全部目标行和字段值。若目标行已删除或已改写某个目标字段，整笔历史失效，其余字段保留后续编辑。文本编辑器继续处理自己的撤销历史。每个表格实例最多保留 64 笔、约 8 MiB 的历史数据，超出单笔容量的粘贴仍提交并清空之前的历史。

`background` 和 `border-color` 使用 `brush` 类型，纯色可直接绑定。线性渐变使用 `@linear-gradient(90deg, #536dfe 0%, root.accent 100%)`，径向渐变使用 `@radial-gradient(circle, #fff, #536dfe)`，中心和半径由元素尺寸决定。色标颜色可以引用属性，省略的位置会均匀插值，显式位置须在 0% 到 100% 之间且按升序排列。`Rectangle` 和 `Button` 可以用 `border-top-left-radius` 等属性分别覆盖四角的 `border-radius`。

视觉节点可以用 `z` 控制同一父级下的叠放顺序，默认是 0。同值按声明顺序绘制，指针从最上层开始命中。Tab 遍历、无障碍阅读顺序和组件事件路径保留各自的结构顺序。模态和弹出层由窗口管理打开顺序。

`opacity` 把节点和子节点作为一组淡出，取值限制在 0 到 1。透明度为 0 时跳过绘制，控件仍参与布局并可接收输入。纯透明度更新可复用已准备的布局。运行 `bazel run //examples/layout:layout -- --compositing` 查看重叠淡出与点击顺序。

`Rectangle` 支持 `drop-shadow-*` 与 `inner-shadow-*`，两组都提供 `color`、`offset-x`、`offset-y`、`blur` 和 `spread`。外阴影由背景和边框的透明度生成，沿父容器边界裁剪。内阴影沿矩形的圆角裁切。正扩张增大外阴影并加宽内阴影，负值收缩阴影。外阴影扩张按设备像素取整，范围为 ±192 个设备像素。两种阴影都随节点的 `opacity` 合成，可通过绑定、状态或动画更新。运行 `bazel run //examples/layout:layout -- --shadows`。

`Rectangle { clip: true; }` 按边框内侧的圆角裁剪子树。显式设置 `clip: false;` 允许子控件溢出矩形，仍受更外层的裁剪和窗口范围约束。省略 `clip` 时沿用原有矩形裁剪。绘制和指针命中共用圆角几何，无障碍按其矩形投影报告可见边界。运行 `bazel run //examples/layout:layout -- --clipping`。

普通视觉节点支持 `transform-rotation`、`transform-scale`、`transform-scale-x` 和 `transform-scale-y`。旋转值用顺时针角度，逐轴值可覆盖统一缩放，负值可镜像。`transform-origin-x` 和 `transform-origin-y` 接受长度或百分比，省略时以节点中心为原点。变换作用于已完成布局的子树，绘制、指针定位和裁剪共用变换矩阵。窗口级弹出层继续以窗口坐标定位。运行 `bazel run //examples/layout:layout -- --transforms`。

`ScaleRotateGestureHandler` 把触屏双指手势和 macOS 触控板手势统一成 `started()`、`updated()`、`ended()` 和 `cancelled()`。在 `started()` 保存初始状态，在 `updated()` 读取累计 `scale`、顺时针角度 `rotation` 和局部逻辑像素中心 `center-x/y`，再更新内容变换。手势结束或取消时，`active` 变回 `false`，读数回到默认值。两指须从同一个可用区域开始。第二指接管待提交的单指输入时，会取消原来的点击候选。关闭 `enabled` 会取消该区域的手势，子控件仍可接收单指操作。运行 `bazel run //examples/layout:layout -- --scale-rotate`。

`SwipeGestureHandler` 用 `handle-swipe-left/right/up/down` 选择接收的方向，默认全部关闭。鼠标左键或单指触屏移动达到 8 逻辑像素后，最内层接受主方向的区域接管输入。其他方向可继续由滚动视图处理。`moved()` 中可读 `swiping` 与 `pressed-x/y`、`current-x/y`。坐标以按下时的局部坐标系计算。松手触发 `swiped()`，此时可以读取最终坐标。滑动期间调用 `cancel()`、禁用区域或失去窗口焦点会触发 `cancelled()`。轻触仍可点击内部按钮。运行 `bazel run //examples/layout:layout -- --swipe-gesture`，横拖移动卡片，竖拖滚动画布。

`Image` 的 `source: @image-url("assets/icon.svg");` 引用相对于 `.xr` 的本地 PNG 或 SVG。图片以 `!xr.image` 通过 MLIR，编译器检查资源后把编码字节嵌入 Rust。支持 `image-fit: fill | contain | cover | preserve`。布局内默认 `contain`，其他位置默认 `fill`。SVG 支持独立的矢量图形，由 resvg 按显示尺寸和 DPI 栅格化。SVG 文本和外链资源暂未支持。

`source-clip-x`、`source-clip-y`、`source-clip-width` 和 `source-clip-height` 以源像素选择图块。未指定的宽高延伸到源图边缘，超出源图的部分裁掉。裁剪后的宽高参与隐式尺寸和比例计算，`image` 值的 `.width`、`.height` 仍返回完整资源尺寸。`horizontal-alignment` 和 `vertical-alignment` 决定图像在未平铺轴上的位置，`image-rendering: pixelated;` 使用最近邻采样。普通图像和 GIF 帧也可按轴使用 `horizontal-tile-mode` 和 `vertical-tile-mode`。SVG 直接栅格化裁剪区域，PNG 图块使用独立纹理区域以隔离相邻像素。运行 `bazel run //examples/media:media -- --atlas`。

`Image` 和 `AnimatedImage` 的 `colorize` 接受纯色、线性或径向渐变画刷，并以图像透明度作为遮罩。完全透明的画刷恢复原图。渐变以完整源图坐标计算，裁剪后保留对应的颜色区间，平铺重复该区间，九宫格的各块沿用同一画刷。纯色使用缓存的单通道纹理，渐变在 GPU 上合成。运行 `bazel run //examples/media:media -- --colorize`。

用 `@image-url("assets/frame.svg", nine-slice(4 6 8 10))` 创建九宫格图像。边距按上、右、下、左顺序指定源像素数，也可以给一个统一值，或给纵向和横向两个值。编译器会检查图像是否保留了正尺寸的中心。九宫格填满 `Image`，四角保留逻辑尺寸，空间不足时同轴边距按比例收缩。`horizontal-tile-mode` 和 `vertical-tile-mode` 独立支持 `stretch`、`repeat` 和 `round`，分别拉伸、按原尺寸平铺并裁剪末片、缩放为整数个完整的平铺单元。Rust 可以用 `Image::with_nine_slice` 为已有资源设置边距。运行 `bazel run //examples/media:media -- --nine-slice` 查看对照。

`AnimatedImage` 可播放通过 `@image-url` 内嵌的本地 GIF。编译器合成差分帧并保留帧时长和文件循环次数，普通 `Image` 显示首帧。用 `playing` 开始或停止，停止会回到首帧。用 `paused` 暂停，或用 `speed` 调整播放速率。`loops: 0;` 沿用文件设置，`-1` 持续循环，正数表示总播放轮数。完成有限循环后保留末帧并触发 `finished()`。只读的 `current-frame` 和 `frame-count` 在窗口同步播放状态后可用于绑定。仍在场景中但不可见的控件冻结播放时钟，重新可见时继续。运行 `bazel run //examples/media:media -- --animated` 体验。

`Path` 的 `commands` 接受 SVG 路径字符串，包括相对坐标、直线、曲线、弧线和闭合。可以用 `fill`、`stroke`、`stroke-width` 设置外观，用 `viewbox-x`、`viewbox-y`、`viewbox-width`、`viewbox-height` 和 `fit` 控制适配。默认以非零环绕规则填充并使用 `contain`。

生成的 Rust API：

```rust
let mut ui = Counter::new();
ui.set_count(4);
assert_eq!(ui.get_doubled(), 8);
ui.invoke_increment(2);
ui.dispatch("add", "clicked");
let scene = ui.render();
```

属性 getter 首次读取时求值并缓存结果。运行时记录绑定本次实际读取的属性，依赖变化时沿组件关系标记失效，等下一次读取才重算。条件切换会替换订阅，未执行分支中的属性变化不会使缓存失效。

setter 和 DSL 赋值覆盖绑定并解除订阅，`reset_*()` 恢复默认绑定。已知值相同的写入不会使下游失效。`in` 和 `in-out` 属性提供公开 setter，`out` 提供公开 getter，`private` 仅供组件内部访问。

直接写在元素上的普通计算绑定也使用缓存，例如 `Text { text: "计数：" + root.count; }`。纯回调被 Rust 重新注册时，依赖它的绑定会失效。生成组件通过 `Component::render_shared()` 复用干净的 `Node` 快照。

`for` 按位置保存每行的场景和绑定缓存。替换字符串列表时会比较被使用的行，值相同的位置复用原有绑定。嵌套 `for` 各自跟踪依赖，`if` 隐藏的列表会等到再次显示才求值。普通重复项最多生成 4096 行。

`ListView` 接收一个 `for` 模板，支持整数计数、`[string]` 和命名记录列表，最多寻址 100 万行。固定的 `row-height` 决定滚动位置与行索引，只有与视口相交的行才会生成。`current-index` 可双向绑定，鼠标选择和键盘方向键、Home、End、PageUp、PageDown 会更新索引。选择按位置解释，插入或删除行后仍使用当前索引。

`TreeView` 接收一个重复的 `TreeRow`。在模板中用 `key: record.id; parent-key: record.parent-id;` 把两个整数记录字段声明为树的角色。非负键唯一，找不到父节点的记录显示为根，重复键的记录会被排除。循环在最小键处断开父链接。`current-key` 可双向绑定，展开状态按模型行标识保留，排序不会把展开状态转给别的记录。点击三角或按 Enter、Space 可切换展开，左右键可展开、折叠或移动到父子节点。只有视口内的可见节点会创建行内容。运行 `bazel run //examples/tree:tree` 查看示例。

树拓扑单独订阅模型的结构变化和当前视图使用的键字段。修改名称或数值时，只比较被修改行的键和父键，二者相同就复用拓扑和导航顺序。行内容按视口跟踪，视口外的字段变化可以保留当前帧缓存。插入、删除、排序和父子关系变化会在下次读取时更新拓扑。

`multi-select: true;` 启用多选，选区与 `current-key` 独立。Shift+点击或 Shift+方向键选择当前树顺序中的一段，Ctrl/Cmd+点击或 Ctrl/Cmd+Space 增减一行，Ctrl/Cmd+Shift 增加一段，Ctrl/Cmd+方向键只移动光标。Ctrl/Cmd+A 选择展开后可见的所有行，Escape 清空选区。折叠保留后代的选中状态，排序保留记录标识，删除或替换模型会清除失效的选项。`selection-changed(int)` 报告用户操作改变选区后的选中数量。Rust 宿主可通过场景节点的 `tree_selection` 读取当前业务键及其行标识。

`TreeRow` 可在布局中组合类型化编辑器。`TreeCell { text <=> record.label; }` 编辑字符串，`SpinBox { value <=> record.quota; }` 编辑整数，`Slider { value <=> record.load; }` 编辑浮点数，`CheckBox { checked <=> record.active; }` 或 `Switch` 编辑布尔值。绑定目标是可写列表中当前记录的字段。布局可用 `if` 条件显示，编辑器的路径需保持稳定。

双击文本单元格或按 F2 开始重命名，Enter 提交，Escape 放弃。聚焦树并选定当前行后，按 Tab 可进入该行可用的复选框、数字框或滑块。重排后仍在视口内的记录会保留草稿，折叠隐藏记录或外部修改被编辑字段会丢弃草稿。树和表格的编辑器共用字段提交逻辑，核对行标识和字段原值后再合并到最新记录。

`ListView` 的 `for` 也可直接实例化匿名行组件。组件可接收模型项和索引作为输入，包含输入框、按钮或其他控件。行状态按模型标识保留，滚出视口或因插入而移动时会跟随原行。已访问过的行组件保留到模型删除该行或替换模型。当前行模板以组件作为 `ListView` 的直接子项，支持组件组合，虚拟列表嵌套尚待实现。

Rust 可用 `model_<property>()` 取得列表属性的共享模型，用 `bind_model_<property>(model)` 接入已有的 `VecModel<T>`。`set_row`、`push`、`insert` 和 `remove` 分别通知行数据和结构变化。`get_<property>()`、`set_<property>(Vec<T>)` 和 Rust 回调使用 `Vec<T>` 快照。模型属于 UI 线程，后台任务可以通过 `UiSender` 把行索引和值交给消息处理器。winit 会复用没有受变化影响的视口场景与布局。

```rust
let model = xrack_runtime::VecModel::from(vec!["首行".into(), "第二行".into()]);
ui.bind_model_results(model.clone());
model.set_row(1, "第二行已更新".into());
```

回调可用 `callback score(value: int) -> int;` 声明结果类型，处理器的末尾表达式提供返回值。`.xr` 中可用 `root.score(4)` 或 `child.score(4)` 调用组件回调。属性绑定只能调用标有 `pure` 的回调，其处理器也不能写入属性或调用有副作用的回调。编译器会检查参数和返回类型，并拒绝递归调用以及经回调形成的属性依赖循环。

DSL 调用与 Rust 的 `invoke_*()` 共用同一处理逻辑。注册了 Rust `on_*()` 闭包时，有返回类型的调用返回闭包的结果。纯回调接收 `Fn` 闭包，调用方需保证闭包没有副作用。元素事件通过 `dispatch(id, event)` 分发。

`callback submitted <=> editor.accepted;` 可以把内部控件事件或子组件的回调作为组件 API 公开。别名沿用目标的参数、返回类型和纯度，调用别名与调用目标进入同一个回调。也可以在命名元素上声明自定义回调。组件实例可写 `entry := Editor { submitted => { root.save(); } }`，类型化的处理器可以返回结果。列表和网格的实例处理器可读取当前模型项和索引。

实例处理器和 Rust `on_*()` 设置目标回调的实现。声明上已有的普通 DSL 回调语句仍先执行，返回值以当前注册实现为准。别名上声明的处理器可作为默认实现。纯回调的注册变化会让依赖该回调的绑定失效。运行 `bazel run //examples/stepper:stepper -- --linked`，在共享编辑器或任务行按 Enter 体验转发。

## 焦点作用域

`FocusScope` 可包裹复合控件。用 `initial-focus: editor;` 指定首次进入时优先选择的后代。作用域默认记住最后聚焦的有效子控件，`restore-focus: false;` 可让每次进入都采用初始目标。Tab 和 Shift+Tab 可以离开作用域。`focus-on-tab-navigation` 和 `focus-on-click` 控制作用域本身是否接管这两种入口，子控件保留各自的输入行为。

事件处理器可以调用 `scope.focus()` 或 `scope.clear-focus()`。窗口在回调结束后用当前布局处理命令。隐藏、禁用或被模态弹层挡住的目标不会获得焦点。每个组件实例保存独立的请求身份和焦点记录。运行 `bazel run //examples/layout:layout -- --focus-scopes`，在两个编辑区域之间跳转，按 Escape 清除当前区域的焦点。

`has-focus` 表示当前控件直接持有键盘焦点。`FocusScope` 的 `focus-within` 在作用域或其子控件持有焦点时为真。同一区域内切换子控件时，依赖 `focus-within` 的样式保持有效。窗口失活会清除这两个读值，重新激活后恢复有效的逻辑焦点。`focus-gained(reason)`、`focus-lost(reason)`、`focus-changed-event(reason)` 和 `focus-within-changed(focused, reason)` 可读取有类型的 `FocusReason`。回调运行时，失焦和得焦两侧的读值均已发布。

`capture-key-pressed(event)` 和 `capture-key-released(event)` 沿当前焦点路径从外到内运行。窗口和控件处理后，剩余按键通过 `key-pressed(event)` 和 `key-released(event)` 从内到外传递。返回 `EventResult.accept` 停止传递，返回 `EventResult.reject` 则继续。`KeyEvent` 提供 `key`、`text`、`repeat` 和 `modifiers`。逻辑键名可用 `"F2"`、`"Escape"` 或 `"ArrowLeft"`，`modifiers.primary` 在 macOS 上对应 Command，其他桌面平台对应 Ctrl。

键盘路由以事件开始时的焦点为准。回调改变焦点后，新控件从下一个事件开始接收。模态弹层和菜单限制分发范围，IME 预编辑沿用原输入路径。运行 `bazel run //examples/layout:layout -- --key-routing`，聚焦输入框后比较 `x` 被拦截、`a` 正常输入和 `F2` 冒泡。

## 窗口内拖放

`DragArea` 通过 `data` 绑定字符串载荷，通过 `drag-key` 标记用途。鼠标或触摸移动 6 个逻辑像素后开始拖动并冻结载荷，短距离按放仍可点击子控件。`allow-copy`、`allow-move`、`allow-link` 声明源允许的操作。按 Ctrl 请求复制，按 Shift 请求移动，同时按两键请求链接。源可以用 `drag-finished(DragAction)` 根据最终结果更新模型。取消时结果为 `none`。

`DropArea` 用 `keys` 过滤拖动源，也可以用 `drop-action` 限制操作。嵌套目标优先匹配最深的可接收区域。`entered(string, DragAction)`、`position-changed(length, length)`、`exited()` 和 `dropped(string, DragAction)` 提供有类型的事件，位置坐标相对于目标区域。成功释放时先调用目标的 `dropped`，再通知源。拖动复用窗口的裁剪、弹层范围和虚拟行标识。设置任一区域的 `enabled: false;` 只停用该区域的拖放行为，子控件仍可交互。

运行 `bazel run //examples/form:form -- --drag`，把编号拖到右侧。移动后左侧编号消失，按 Ctrl 拖动则保留编号。当前载荷是窗口内的字符串，适用于卡片、条目和同窗口控件之间的操作。

macOS 上，`DropArea { accept-files: true; }` 也可接收 Finder 或其他应用拖入的本地文件。`file-extensions: ["png", "jpg"];` 可筛选扩展名，匹配忽略大小写，可带前导点。整个批次必须符合规则。`files-entered([string])` 和 `files-dropped([string])` 收到有序的 UTF-8 路径列表。外部文件操作按复制处理，应用按收到的路径自行读取文件。运行 `bazel run //examples/form:form -- --drop-files`，分别向图片区和文档区拖入一个或多个文件。

## 声明式计时器

`Timer` 是组件根部的命名声明，`interval` 使用整数毫秒，默认 1000，非正数暂停调度。`running` 默认关闭，可双向绑定。默认触发一次并在 `triggered()` 前清除 `running`，设置 `repeat: true;` 可循环。`triggered-on-start: true;` 会在启动时额外触发一次。处理器可调用 `timer.start()`、`timer.stop()` 或 `timer.restart()`。

计时器随拥有它的组件进入或离开活动布局而启动或撤销。回调在 UI 线程运行，间隔变化会重新计时，重复计时器错过的周期合并为一次回调。运行 `bazel run //examples/controls:controls -- --timer` 体验倒计时、暂停和单次触发。

## 异步 Rust

`xrack_platform_winit::spawn_local` 在运行 XRack 的 UI 线程上轮询 `Future + 'static`，支持捕获 `Rc` 等 `!Send` 状态。可以从 `run_with_messages` 的 setup、UI 回调或其他本地任务启动。任务返回 `LocalTask<T>`，可 `.await` 结果或调用 `abort()`，丢弃句柄会让任务继续运行。窗口事件循环退出时会取消未完成的本地任务。生成组件的 `as_weak()` 可用于在 `.await` 后重新取得组件。

`run_with_messages(component, setup)` 把 `UiSender<M>` 交给 setup，setup 返回 `FnMut(&mut Component, M)` 消息处理器。工作线程或应用自己的异步运行时可调用 `try_send`。队列满或关闭时会把消息随错误返回。事件循环每批处理最多 64 条消息，再统一刷新场景和布局。默认队列容量是 256，组件状态和 GPU 绘制留在 UI 线程。

`bazel run //examples/async_demo:async_demo` 使用一个由应用持有的多线程 Tokio runtime 运行计时和后台搜索。UI 本地 Future 等待 Tokio 计时任务的 `JoinHandle`，完成后在 UI 线程更新组件。搜索通过 `UiSender` 提交列表和逐行更新，用请求 ID 丢弃过期结果。应用可以按自己的 I/O 需求启用 Tokio 的驱动。

## 多窗口

`Window` 可绑定 `no-frame: true;` 隐藏系统标题栏。`WindowMoveArea` 可包住自定义标题栏，空白位置拖过 4 个逻辑像素后开始系统移动，里面的按钮和其他交互控件仍处理各自的输入。设置 `enabled: false;` 只停用该区域的窗口拖动。打开菜单、下拉框或对话框时会暂停窗口手势。

无边框窗口可用 `resize-border-width: 8px;` 在四边和四角启用缩放命中区。该区域避开可交互控件，`resizable: false;` 关闭缩放。窗口的 `min-width`、`min-height`、`max-width` 和 `max-height` 长度会传给桌面宿主。Windows 和 Linux 使用 winit 的系统边缘缩放，macOS 通过 AppKit 更新窗口边框并保留对侧边缘。运行 `bazel run //examples/layout:layout -- --frameless` 可体验。

回调可以调用 `root.minimize()`、`root.maximize()`、`root.restore()`、`root.toggle-maximized()`、`root.set-full-screen(bool)` 和 `root.toggle-full-screen()`。`restore()` 请求恢复普通窗口。窗口的 `window-active`、`minimized`、`maximized` 和 `full-screen` 是只读的响应式状态，由宿主从原生窗口采样。`minimized-known` 表示当前后端能否报告最小化，例如 Wayland 上该值为 `false`。同一批切换请求按发出顺序解析，退出全屏后需要调整最大化状态时，宿主会等待退出确认。自定义标题栏可用这些属性更新按钮文字和无障碍名称。

`xrack_platform_winit::run_desktop` 在同一个 winit 事件循环中运行多个顶层组件。通过 `Desktop::open` 可在启动时或任意窗口的 UI 回调中创建新窗口。`open_with_messages` 为每个窗口提供独立的 `UiSender` 和 reducer。`DesktopWindow::close` 关闭指定窗口。所有窗口关闭、所有托盘图标隐藏或移除后，事件循环退出。

顶层 `.xr` `Window` 可实现 `close-requested() => { … }`，返回 `true` 允许关闭，返回 `false` 保留窗口。标题栏、Rust 的 `DesktopWindow::close()` 和 `.xr` 的 `root.close()` 都经过这个处理器。`root.close()` 在当前回调结束后提交关闭请求。处理器可先拒绝，再打开 `MessageDialog`，由确认回调清除未保存标记并重新请求关闭。运行 `bazel run //examples/form:form -- --close-guard` 体验。

在根 `Window` 下声明命名的 `SecondaryWindow`，并在其中放一个继承 `Window` 的组件实例。`visible <=> root.show-inspector;` 把显示状态和父组件同步，`show()`、`hide()` 控制显示，`close()` 提交子窗口的关闭请求。子窗口批准关闭后会将 `visible` 写回 `false`。隐藏会释放原生窗口并保留子组件实例，重新显示时可继续编辑。父窗口退出时，宿主先释放它拥有的辅助窗口。运行 `bazel run //examples/counter:counter -- --secondary` 体验。

每个窗口维护自己的焦点、编辑状态、计时器、原生对话框和本地任务。共享的响应式模型变化后，宿主在同一轮事件中刷新依赖失效的窗口，并合并各窗口的唤醒时间。兼容的 surface 复用 GPU 设备。运行 `bazel run //examples/counter:counter -- --windows` 可验证独立计数、共享状态、异步更新、动态开窗和关闭。

### 系统托盘

Linux、macOS 和 Windows 桌面宿主可用 `Desktop::register_tray` 注册继承 `SystemTrayIcon` 的顶层组件。托盘应用可以独立运行，也可从菜单打开窗口。`icon` 接收 PNG 或 SVG 图像，`title` 和 `tooltip` 支持响应式绑定。`visible: false` 隐藏图标，组件和本地任务仍保留，`DesktopTray::close()` 释放它。最后一个窗口和托盘都关闭后，桌面事件循环退出。

托盘可声明一个静态 `Menu`，菜单内容支持子菜单、分隔线、动态可见性、勾选状态和共享 `Action`。在 macOS 上，`icon-template` 启用 AppKit 模板图标，带菜单的图标打开原生菜单，纯图标可处理 `clicked()`。在 Windows 上，左键激活触发 `clicked()`，右键或键盘菜单键打开原生菜单，空的 `tooltip` 使用 `title`，Explorer 重启后会重新注册图标。

Linux 使用会话 D-Bus 上的 StatusNotifierItem 和 DBusMenu，需要支持该协议的面板或桌面扩展。面板请求激活时触发 `clicked()`，菜单由面板呈现。面板稍后启动或重启时，应用会重新注册图标。D-Bus 回调通过事件队列交给 UI 线程，面板缓存的旧菜单命令会在菜单更新后失效。

托盘回调可通过 `spawn_local` 调度 UI 线程任务。运行 `bazel run //examples/tray:tray`，在系统托盘打开 **XRack** 菜单。Linux 可用 `dbus-run-session -- bazel run //crates/xrack-platform-winit:async_test -- status_notifier_round_trip --ignored` 在独立会话中运行协议测试。

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

## MLIR 阶段

```text
.xr → ANTLR4 Rust 解析树 → Rust AST
    → 语义分析 → TableGen builders → xr dialect（UI IR）
    → MLIR verify + XR schema checks
    → xr-fold-constants → xr-simplify-expressions → xr-simplify-visuals
    → canonicalize → CSE → xr-prune-expressions
    → xr-specialize-hidden-visuals → xr-precompile-static-paths → xr-plan-static-grid
    → xr-cache-static-gradients
    → xr-plan-update-effects → xr-cache-static-subtrees
    → xr-hoist-repeat-invariants → xr-hoist-repeat-subexpressions
    → xr-share-visual-bindings → verify
    → 按 operation 和 SSA use-def 生成 proc_macro2::TokenStream
    → xrackc 输出 Rust 源码
```

`xrack-parser` 调用生成的 `document()`，从类型化的规则上下文直接构造 `xrack-syntax` 定义的 `ast::Document`。它收集词法和语法诊断，并保留一基行列号。`--emit ast` 可以查看单文件的 AST，编译入口还会解析导入并合并文档。

`xr` 方言是编译主链路的 UI IR。语义分析验证 AST 成员并交给原生 MLIR builder。`xr.instance` 以符号引用指定组件，`xr.read` 和 `xr.assign` 携带解析后的属性所属元素或实例。MLIR 会核对引用目标、属性类型和访问权限。表达式使用 SSA operand/result 和 `!xr.int` 等类型，源码位置保留为 MLIR location。Rust 后端通过 MLIR C API 查找 operand 的定义，在 getter、绑定或赋值的使用位置生成表达式，以保留惰性求值、短路和回调中的读写顺序。`--emit hir` 提供前端调试视图。

`mlir/Xrack.td` 使用 TableGen ODS 定义 `xr` 方言。Bazel 通过 `mlir-tblgen` 生成操作和类型的 C++ builder 与校验器。Rust 通过 C ABI 传入 SSA 句柄、类型、属性和位置，C++ 调用生成的 `Op::create` 和 `Type::get`。MLIR printer 负责生成 `--emit mlir-raw` 和 `--emit mlir` 的文本。

`xr-fold-constants` 折叠可表示的整数、浮点和长度算术，以及比较、布尔表达式、整数转浮点、百分比转比率、整数和布尔值转字符串及常量字符串拼接。整数溢出、整数除零和产生非有限浮点结果的表达式保留原操作。`xr-simplify-expressions` 去掉 `x + 0`、`x && true`、`!!x` 和空串拼接等恒等操作，同时保留动态操作数的求值。`xr-simplify-visuals` 去掉恒真的元素守卫，并保留元素及其事件路径。

`canonicalize` 简化常量条件选择，`CSE` 合并同类型的相同字面量。`xr-prune-expressions` 清理优化后没有使用者的表达式，例如常量短路跳过的 `xr.call`。独立调用用 `xr.invoke` 表示，动态条件即使两臂相同也会求值。`--emit mlir-raw` 输出优化前的 lowering 结果，`--emit mlir` 输出 Rust 后端消费的优化结果。编译库返回 token stream，`xrackc` 在写入 `.rs` 文件或标准输出时才将其转成文本。

`xr-share-visual-bindings` 在同一组件渲染作用域或同一重复项模板中，为结构相同的标量表达式标记 `xr.visual_cache_group`。例如两个 `Text` 的 `text: "Count " + root.count;` 会共用一个惰性 `Property<String>` 缓存。可共享的表达式由常量、根属性读取和当前行参数构成。重复项的缓存仍按行保存，含回调调用或导航读取的表达式仍独立求值。

`xr-hoist-repeat-invariants` 把完全独立于行参数的视觉绑定放到组件级惰性缓存，同一组件的多个重复项可以共用该缓存。空列表仍然不求值，读取 `item`、`index`、子元素状态或调用回调的表达式保留原有求值位置。`xr-specialize-hidden-visuals` 将恒假条件的渲染降为 `Node::hidden()`，省去生成不可达的渲染子树，并保留节点槽位和事件路径。

`xr-hoist-repeat-subexpressions` 也能提升混合绑定中的公共前缀，例如 `("Caption " + root.caption) + item` 的左侧。生成的缓存仍在原来的分支使用点读取。`xr-plan-update-effects` 标记 `Rectangle.background` 只影响绘制。若帧输入只有已标记的背景变化，且上次布局直接使用原始场景，窗口宿主用新场景绘制并复用原有布局。`xr-cache-static-subtrees` 标记仅包含静态视觉元素的子树，生成代码按线程复用场景节点。静态节点克隆共享底层数据，修改副本时再复制受影响的节点。普通动态节点仍直接持有数据。

`xr-precompile-static-paths` 标记常量 `Path.commands`，资源阶段复用已有解析器生成线段和源坐标边界，并嵌入 Rust。绘制阶段仍根据实际尺寸、`fit`、描边和填色变换路径。动态命令继续按运行时值解析。`xr-plan-static-grid` 对子项结构、可见性和跨度固定的 `GridLayout` 预计算单元格占位。布局仍测量文字、分配轨道大小并保留原场景索引。手写节点、动态网格及被编辑的网格快照继续使用同一套运行时占位算法。

`xr-cache-static-gradients` 标记色标和角度都为常量的渐变。生成的 Rust 在首次使用时分配色标数组，同一绑定的各个组件实例共享这份只读数组，省去各实例的响应式缓存槽。依赖响应式属性的渐变仍随属性变化更新。

Bazel 使用 `rules_rust_bindgen` 和同一份 LLVM 23 的 clang/libclang 生成 `c_bindings.rs`。`xrack_mlir_bindings` 将它作为 crate 根文件，供 `xrack-mlir` 和 Rust Analyzer 的项目图使用。Rust 持有原生模块，构建和优化都在这个模块上完成。C++ 接口负责注册方言、调用生成的 builder、配置 pass 和跨操作校验。

## 测试

`bazel test //crates/xrack-parser:parser_test` 对照迁移时从 C++ 前端采集的完整 AST 和诊断快照。用例覆盖表达式、Unicode、BOM、CRLF 和错误恢复。JSON 快照只用于回归测试。

`bazel test //tests:mlir_pass_test` 对同一原生模块断言优化前后的 SSA 和操作结构。用例覆盖常量折叠、CSE、动态分支及跨赋值的读取顺序。

`bazel test //tests:ui_interaction_test` 用控件 ID 驱动点击、键盘导航和文本输入，在确定性的布局下验证计数器、模态对话框与选择控件。该目标可在无 GPU 的 CI 中运行。`bazel test //tests:gpu_test` 额外使用原生图形适配器验证离屏像素和绘制路径。

## 首版边界

当前的状态转场作用于视觉属性及组件实例的布局属性。标准控件仍在扩充。文件扩展名约定为 `.xr`，语法取 Slint 风格子集。

桌面宿主通过 AccessKit 发布可见控件、当前焦点和活动模态窗口的无障碍树。`accessible-name` 和 `accessible-description` 可补充控件语义，例如纯图标按钮的名称。当前支持聚焦、点击、文本值替换和数值步进。`ComboBox` 和菜单公开弹层内的可见选项，子菜单会跟随当前展开层级。滚动容器公开偏移和范围，可按项、按页或指定偏移滚动。

单行和多行文本框及可编辑 `ComboBox` 公开已提交的文本、光标和选区。辅助技术可读取文本范围并设置选区，操作沿用编辑器的 Unicode 字素边界与视觉换行。可见文本按方向拆分范围并发布字符坐标。IME 预编辑期间保留已提交的文本，选区操作在预编辑结束后恢复。

`xrack-layout` 计算线性布局、绝对坐标和祖先裁剪。隐藏元素不占布局空间。鼠标点击需要在同一目标内按下和释放，禁用或被裁剪的控件不会触发点击。winit 将鼠标、Tab / Shift+Tab 和 Enter / Space 事件交给该状态机。绑定变化、窗口缩放和 DPI 变化会触发重绘。

原生 GPU 路径目前在 macOS / Metal 验证。离屏 GPU 测试需要适用的图形适配器和支持中文的系统字体，需显式运行。单行文本输入已接入 IME 预编辑和提交。
