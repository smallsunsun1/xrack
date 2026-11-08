# XRack

类似 Slint 的声明式 UI 编译器原型，使用 **ANTLR4 C++ runtime + MLIR + Rust**，以 **Bazel** 为正式构建入口。

生成的组件可以直接运行在 winit 窗口中。布局和输入处理使用逻辑像素，FemtoVG 通过 wgpu 绘制背景、圆角、边框、文本、图像和矢量路径。组件通过 `Node` 场景树和 `Component` 接口与平台解耦。

## 构建和运行

要求 Bazel 9.2.0 / Bazelisk、JDK 17+（仅运行 ANTLR generator），以及支持 **C++20** 的平台 C++ 编译工具链（macOS 需要 Xcode Command Line Tools）。目标与构建工具均使用 C++20。依赖由 Bzlmod 下载；首次构建会从源码编译 LLVM/MLIR，耗时明显多于增量构建。

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
bazel run //examples/delegates:delegates
bazel run //examples/notes:notes
bazel run //examples/dialog:dialog
bazel run //examples/menu:menu
bazel run //examples/table:table
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

MLIR 通过 `llvm` module **0.8.26** 导出的 `@llvm-project` 构建，LLVM 版本固定为 **23.1.2**。不探测 `llvm-config`、Homebrew LLVM 或系统 MLIR，不使用 Melior 的系统链接方式。

ANTLR generator 和 C++ runtime 均为 **4.13.2**。Bazel 在 `.g4` 变化时重新生成 C++ 源码。generator 使用 Bazel Java 工具链，最终编译器通过 C ABI 静态链接 C++ runtime，运行时不需要 Java。

Cargo workspace 为依赖管理和可选 Rust 静态检查提供支持。`.cargo/config.toml` 将构建产物放到 `.tools/cargo-build/`。静态检查读取 Bazel 生成的同一份 MLIR C bindings。

```sh
bazel build //mlir:c_bindings_out_dir
OUT_DIR="$(pwd)/bazel-bin/mlir/c_bindings_out_dir" cargo check --workspace --locked
cargo fmt --all --check
OUT_DIR="$(pwd)/bazel-bin/mlir/c_bindings_out_dir" cargo clippy --workspace --all-targets --no-deps -- -D warnings
```

包含 native bridge 的可执行文件和测试通过 Bazel 构建；单独 `cargo build` / `cargo test` 不负责生成或链接 native 依赖。

## VS Code

打开仓库根目录，安装推荐的 Rust Analyzer 和 CodeLLDB 扩展。用 **Terminal → Run Task → Bazel: Generate rust-project.json** 生成本机项目图，或在终端运行：

```sh
bazel run //:rust_project -- //... //tests:gpu_test
```

Rust Analyzer 读取 Bazel 的 crate 图、标准库源码、生成的 Rust 文件和过程宏。项目图包含本机路径，由 Git 忽略。更改 `BUILD.bazel`、依赖或工具链后重新生成；如果编辑器仍显示旧图，运行 **Rust Analyzer: Reload Workspace**。

C++ 使用 clangd。运行 **Bazel: Generate compile_commands.json** 或 `python3 tools/compile_commands.py`，从 Bazel 的编译动作导出项目内 C++ 源码的参数。脚本会先生成 ANTLR 与 TableGen 文件，并保留 Bazel 的头文件路径、宏、C++20 和 macOS SDK 参数。数据库包含本机路径，由 Git 忽略。

本工作区从 `PATH` 选择 clangd。macOS 上请使用与当前 Xcode / SDK 兼容的 clangd，可用 `clangd --version` 检查。过旧的 clangd 可能无法解析 SDK 的 libc++ 头文件，即使 Bazel 构建正常也会出现连带诊断。更改 clangd 路径后，重启 VS Code 的 clangd 服务即可重新诊断。

默认构建任务是 **Bazel: Build Counter**；**Bazel: Run Counter** 打开窗口，**Bazel: Test workspace** 运行常规测试，**Bazel: Test native GPU** 运行需要显卡和字体的测试。F5 使用 **XRack: Debug Counter** 构建调试版本并启动 CodeLLDB。保存时不会执行 Cargo 检查。

## Bazel 磁盘缓存

仓库 `.bazelrc` 默认启用 `--disk_cache=~/.cache/bazel/xrack`。此缓存保存构建动作输出，可以跨 Bazel 重启、不同 output base 和兼容的 worktree 复用；它与下载源码包的 repository cache、当前 output base 的增量缓存分开。

回收策略为 `--experimental_disk_cache_gc_max_size=30G` 和 `--experimental_disk_cache_gc_max_age=30d`，由 Bazel 在空闲时清理，并非构建期间的硬配额。缓存按输入和工具链签名匹配，修改 C++ 标准、Rust 版本或编译选项后可能需要重新预热。已有历史动作不会因为新增配置就保证全部出现在 disk cache 中，首次执行会逐步填充。

可用 `bazel info --announce_rc output_base` 检查实际加载的缓存配置。缓存目录由 `~` 定位到各开发者自己的主目录，无需修改仓库中的机器路径。

## Crate 分层

| Crate | 责任 |
| --- | --- |
| `xrack-syntax` | AST、类型、源码位置与诊断 |
| `xrack-parser` | 安全封装 ANTLR4 C++ parser 的 C ABI，返回拥有所有权的 Rust AST |
| `xrack-ir` | 前端符号解析、类型检查、循环绑定检测与 HIR 调试视图 |
| `xrack-mlir` | 将语义分析后的 AST 成员降到 `xr`，运行 MLIR pass 并提供借用原生模块的只读视图 |
| `xrack-codegen` | 读取优化后的 MLIR 操作与 SSA use-def，用 `quote!` 生成 Rust token stream |
| `xrack-compiler` | 编译阶段编排与 `xrackc` CLI |
| `xrack-runtime` | 惰性属性绑定、重复项缓存和 `Node` 场景快照，无 native 编译器依赖 |
| `xrack-assets` | PNG 解码、SVG 栅格化和 SVG 路径命令解析 |
| `xrack-layout` | 逻辑像素布局、容器排布、裁剪、命中测试、鼠标捕获与键盘焦点；文字测量由后端实现 |
| `xrack-renderer-femtovg` | 字体发现与回退、文字测量、FemtoVG 绘制、wgpu 纹理与 PNG 读回 |
| `xrack-platform-winit` | 原生窗口、输入事件、DPI、交换链和按需重绘 |

Native 实现在 `parser/` 和 `mlir/`。ANTLR C ABI 返回的 buffer 由 parser 释放。MLIR 模块及其 context 由 `xrack-mlir` 管理，操作和 SSA 值引用只在模块存活期间有效。

## 语言示例

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

`TextInput` 和 `LineEdit` 提供单行键盘编辑、选区、IME 预编辑和提交。`text <=> root.query;` 与 `checked <=> root.flag;` 分别把文本输入和复选框与可写属性相连。条件子元素用 `if condition : Element { ... }` 表达，隐藏时保留组件的内部状态。重复元素用 `for item[index] in root.items : Text { ... }` 表达，接受整数计数、`[string]` 和命名记录列表，以匿名、无状态的显示模板生成纵向列表。

`ScrollView`、`ListView`、`TreeView` 和 `TableView` 在内容溢出时显示可拖动的纵向滚动条。`ScrollView` 和 `TableView` 也提供横向滚动条。拖动滑块定位，点击轨道按视口大小翻页。滚动条覆盖在内容边缘并共享滚轮的滚动位置，表头与表身继续同步。可通过 `vertical-scroll-bar-policy` 和 `horizontal-scroll-bar-policy` 设置各滚动轴的 `as-needed`、`always-on` 或 `always-off`。拖动滚动条保留当前焦点，虚拟视图继续只创建视口内的行。

`ScrollView` 根据直接子项的位置和布局尺寸计算内容范围。`content-width` 和 `content-height` 可以用长度或视口百分比指定最小内容尺寸。子项的百分比尺寸以显式内容尺寸或视口为基准。`viewport-height` 仍可用于既有的最小内容高度绑定。`Shift` 加滚轮可横向滚动，嵌套视图会将各轴剩余滚动量交给外层。运行 `bazel run //examples/layout:layout` 可体验二维画布。

滚动容器可用单指拖动内容，桌面也可以按住鼠标中键拖动。快速松手时，内容按松手前的速度继续滚动并减速。手势锁定起始视图，嵌套视图沿用滚轮的逐轴交接规则。短触仍按按钮等控件的点击处理。惯性在边界停下，也可以再次按下停止。

`TextEdit` 支持多行文本、`text <=>`、`edited(string)`、`read-only`、占位文本和 IME。`wrap` 接受 `word-wrap`、`char-wrap` 和 `no-wrap`。Enter 插入换行，Home 和 End 按当前显示行移动，Ctrl 或 Command 加 Home 和 End 移到文档首尾。鼠标拖动或 Shift 加方向键可跨行选择，滚轮浏览内容，编辑和键盘移动时视口跟随光标。

单行和多行文本控件支持鼠标选区和系统剪贴板。Ctrl 或 Command 加 C、X、V 分别复制、剪切、粘贴，只读字段可以复制。Ctrl 或 Command 加 Z 撤销，加 Shift 和 Z 或按 Ctrl+Y 重做。连续输入和删除可以合并，粘贴、剪切及 IME 提交各自形成一笔。历史按编辑控件保存，虚拟列表内按稳定行标识定位。当前文本与历史记录不一致时会重建历史起点，避免撤销覆盖外部更新。剪贴板回执只作用于原来的编辑目标和选区。

`Dialog` 是同一窗口内的模态弹层。用 `open <=> root.dialog-open;` 双向绑定打开状态。窗口居中显示对话框，内容按垂直布局排列。`DialogButton` 的 `role` 取 `accept` 或 `reject`，关闭后分别触发 `accepted()` 或 `rejected()`。Escape 默认取消，也可通过 `close-on-escape` 关闭此行为。启用 `close-on-outside` 后，在遮罩上按下并松开会取消。最近打开的对话框接收输入，Tab 在其中循环，关闭后恢复原焦点。

`Drawer` 是贴靠窗口边缘的模态抽屉。`edge` 可取 `left`、`right`、`top` 或 `bottom`，`open` 可双向绑定。设置 `drag-margin: 18px;` 后，可从窗口边缘向内滑动打开，从面板内侧边缘反向滑动关闭。桌面可用中键执行同样的手势，滑动达到阈值后在松手时提交。手势开启触发 `opened()`，手势、Escape 或遮罩关闭触发 `closed()`。遮罩关闭默认启用。抽屉与对话框共用模态栈和焦点恢复，关闭时延迟构建抽屉内容。运行 `bazel run //examples/dialog:dialog` 体验。

`SwipeView` 的直接子元素为静态 `SwipePage`。用 `current-index <=> root.page;` 双向绑定声明顺序中的页索引，通过 `orientation` 选择横向或纵向。可用触摸、中键拖动或聚焦后按方向键切页，Home 和 End 跳到首尾可用页。手势在松手时切页，页内同轴滚动优先，滚到边界后下一次滑动可以切页。`interactive: false;` 暂停手势和键盘切页，应用仍可写入页索引。

隐藏或禁用的页面在导航中跳过。索引不可用时显示首个可用页，用户切页后才写入新索引并触发 `changed(index)`。只有当前页的内容参与场景求值，页面里的组件状态会保留。运行 `bazel run //examples/navigation:navigation` 可体验。

`PageIndicator` 用 `count` 和 `current-index` 表示页数与当前页。它可以独立使用，也可以和 `SwipeView` 双向绑定到同一个页索引。设置 `interactive: true;` 后可点击点位，或聚焦后用方向键和 Home / End 切页，用户切页触发 `changed(index)`。`orientation` 控制横排或竖排，`dot-size`、`spacing`、`color` 和 `current-color` 控制外观。空间不足时，点位围绕当前页显示一段连续索引，最多绘制 64 个点，两端缩小的点提示还有页。点击仍使用点位对应的原始页索引。

`StackView` 的直接子元素为带 ID 的静态 `StackPage`。例如 `home := StackPage { ... }` 的路由是 `"home"`。`initial-page` 设定初始路由，默认使用首个页面。事件处理器可调用 `navigation.push("details")`、`pop()`、`replace("summary")`、`pop-to("home")` 或 `reset("home")`。导航经 `xr.navigate` 校验，成功时触发 `changed(page, depth)`。同一路由在栈中占一个位置，根页可用 `reset` 更换。Rust 可通过 `navigate_stack(id, action)` 和 `stack_snapshot(id)` 操作和读取导航栈。

`navigation.current-page`、`navigation.depth` 和 `navigation.can-go-back` 是可在绑定及纯回调中读取的实时状态。读取经 `xr.stack_read` 保留在 MLIR 中，源码语义检查会拒绝 `initial-page` 经属性或回调形成的初始化环。一次导航前后的读取各自获取当时的栈状态。

`navigation.reset-to(["home", "details", "preview"])` 用完整路径恢复导航栈，`push-pages([...])` 则把一段路径追加到现有栈。路径可以来自 `[string]` 属性。整段路径都有效且路由唯一时才提交一次并触发一次 `changed`，当前只求值最终的栈顶页面。

只生成栈顶页面的场景，已声明页面的组件状态在返回时可继续使用。焦点位于导航栈内时，切页会记住原页面的焦点。Alt+左方向键或系统返回键可返回上一层。返回请求携带场景中的栈版本，提交时会核对实时版本。运行 `bazel run //examples/navigation:navigation -- --stack` 体验。

在 `StackView` 设置 `transition-duration: 220;` 可启用 220 毫秒的横向切页过渡。返回到当前路径上的祖先页面时从左侧进入，其他切页从右侧进入。离场页沿用已布局的绘制内容，栈顶状态和键盘焦点立即切换。过渡期间暂停该区域的鼠标与触摸操作，连续导航会从正在显示的页面片段开始下一次过渡。默认立即切页，时长为零或负数时也是如此。过渡期间视口几何变化会直接显示当前页。

`Slider` 的 `value` 可双向绑定到 `float` 属性。鼠标拖动连续更新数值，方向键按 `step` 调整，Home 和 End 移到端点。`changed(value)` 只在交互写入新值时调用，`released(value)` 在鼠标或调节键松开后调用。`orientation: vertical;` 将最小值放在下方。`Switch` 支持 `checked` 双向绑定与 `toggled()`。`ProgressIndicator` 接收 `0..1` 或百分比进度，`indeterminate: true;` 显示运动指示条。

`RangeSlider` 使用 `lower-value` 和 `upper-value` 选择一个区间，两端可分别双向绑定到不同的 `float` 属性。Tab 在两个手柄之间移动焦点，方向键按 `step` 调整，Home 和 End 移到当前手柄可达的边界。点击轨道移动最近的手柄。手柄相遇时，向较小值拖动下限，向较大值拖动上限。横向和纵向都使用 `changed(lower, upper)` 和 `released(lower, upper)` 报告有效区间。外部写入交叉值时，显示的下限收拢到上限，下一次有效交互会写回有序区间。运行 `bazel run //examples/controls:controls` 体验。

`Dial` 的 `value` 同样可双向绑定。输入模式使用 `DialInputMode`，可取 `circular`、`horizontal` 或 `vertical`。圆形模式按角度定位，直线模式按水平或垂直位移相对调节。方向键按 `step` 调节，Home 和 End 移到端点。默认使用 280 度限位圆弧，`wrap: true;` 使用整圈并允许跨过端点。可用 `start-angle` 和 `end-angle` 自定义以十二点方向为零、顺时针递增的角度。事件 `changed(value)` 和 `released(value)` 与 `Slider` 相同。

`SpinBox` 的 `value` 是可双向绑定的整数。它支持 `minimum`、`maximum`、`step-size`、`wrap` 和 `read-only`。方向键和步进按钮立即提交，直接输入的数字在 Enter 或移开焦点时提交，Escape 丢弃草稿。交互改值触发 `edited(int)`。

`ComboBox` 从 `[string]` 模型选择一项，可绑定 `current-index` 或 `current-value`，用户选择时触发 `selected(string)`。按值绑定取首个匹配项。弹层在窗口顶层绘制，只绘制当前可见行，支持滚轮、方向键、Home、End、PageUp、PageDown、Enter 与 Escape。点击弹层之外会关闭弹层并消费本次点击。

`RadioGroup` 接收静态 `RadioButton`，也可以用一个 `for` 生成所有选项。通过组的 `current-index` 或 `current-value` 双向绑定选择，按钮的选中状态由组推导。组支持横竖排列、方向键导航和 `selected(string)`。

`TabWidget` 接收静态 `Tab` 子元素，可双向绑定 `current-index`，并通过 `orientation` 切换顶部或左侧标签栏。非活动页面保留组件状态，只有活动页构建内容场景。方向键跳过禁用项，Tab 键把每组选项视为一个焦点停靠点。

`SplitView` 把直接子元素排成横向或纵向面板，支持嵌套。面板沿分栏方向使用 `min-width`、`preferred-width`、`max-width` 或对应的高度属性。拖动分隔线只调整相邻的可见面板，并在当前实例中保存比例。用 Tab 聚焦分隔线后，可用方向键步进，Home 和 End 到达两个边界。`handle-size` 和 `handle-color` 设置分隔线，`resized()` 在比例更新后触发。隐藏面板不占空间，再显示时会恢复其比例。

`MenuBar` 和 `ContextMenuArea` 分别通过顶部菜单和右键打开 `Menu`。菜单支持嵌套子菜单、禁用项、分隔线和可勾选的 `MenuItem`。`activated()` 在选中可用菜单项时触发。方向键导航，Escape 逐层关闭。右键菜单也可用 Shift+F10 打开。

`MenuItem` 的 `shortcut: "Primary+Shift+E";` 会在菜单中显示组合键，并与点击共用 `activated()`。`Primary` 在 macOS 上表示 Command，在其他平台表示 Ctrl。也可以组合 `Ctrl`、`Alt`、`Shift` 和 `Super`，主键支持英文字母、数字和 F1 到 F12。字母和数字需要 Ctrl、Alt、Super 或 Primary 中的一个修饰键。无效字符串不会注册快捷键，可将绑定设为空字符串清除快捷键。

菜单栏快捷键在当前窗口或最上层模态区域内生效。打开右键菜单时，只处理该菜单内的快捷键。隐藏和禁用项不参与匹配，同一作用域内的重复组合键不会触发命令。文本编辑保留选择、剪贴板和撤销重做快捷键。运行 `bazel run //examples/menu:menu` 可体验。

在组件根部声明命名的 `Action` 后，`Button` 和 `MenuItem` 可以通过 `action: command-id;` 共享命令。命令集中声明 `text`、`shortcut`、`enabled`、`checkable`、`checked` 和 `activated()`。可勾选命令先切换 `checked` 再执行回调，也可用 `checked <=> root.some-flag;` 关联组件属性。控件可以覆盖显示文本或额外限制自身的 `enabled`。同一命令的多个控件共用一个快捷键，按钮单独展示时也可以触发命令。每个组件实例独立持有命令状态。

在控件或视觉元素内声明 `Tooltip { text: "操作说明"; }`，可在悬停或通过 Tab 聚焦后显示提示。每个元素可附加一个 `Tooltip`，内容可用纯文本或单个自定义布局。`delay` 默认 600 毫秒，`timeout` 默认持续到离开、失焦或发生输入。提示浮在窗口内，靠近边缘时会避让，点击会穿过浮层。菜单和模态作用域会限制提示显示。

`TableView` 使用静态 `TableColumn` 和一个 `for … : TableRow`。每行按列顺序声明一个展示单元格，支持文本、图像及嵌套布局。列宽支持 `width`、`min-width`、`preferred-width`、`max-width` 和 `horizontal-stretch`。超宽时可用触控板或 Shift+滚轮同步滚动表头和单元格。表头固定在垂直滚动区域上方，只创建可见的行。`current-row` 支持双向绑定，鼠标选择或键盘导航触发 `current-row-changed(int)`。可排序表头交替发出 `sort-ascending(int)` 和 `sort-descending(int)`，应用可用 `VecModel::sort_by` 稳定排序并保留行标识。

设置 `resizable-columns: true;` 后，可拖动表头分隔线调整列宽。可在单列设置 `resizable: false;` 锁定分隔线。手动宽度保存在当前表格实例，受声明的最小、最大宽度限制，排序和替换行模型时仍然有效。

`multi-select: true;` 启用行多选。Shift+点击或 Shift+方向键选取范围，Ctrl/Cmd+点击或 Space 增减一行，Ctrl/Cmd+Shift 增加范围。Ctrl/Cmd+A 全选，Escape 清空，Ctrl/Cmd+方向键只移动 `current-row`。排序和插入保留已选行的标识，删除会移除对应的选择，替换模型会清空选择。`selection-changed(int)` 报告用户操作后的选中数量。Rust 宿主可通过场景节点的 `table_selection` 查询当前选择，使用 `keys()` 获取行标识或 `indices(&model)` 按模型顺序取得索引。

`selection-target` 接受 `rows`、`cells` 和 `columns`，默认按行选择。单元格模式下，Shift 扩展矩形选区，方向键移动焦点并滚动到当前单元格。列模式可点击表头或表身选择整列，Alt+点击表头排序。`multi-select` 在三种目标下都可启用。`selection-changed(int)` 按当前目标报告行、单元格或列数。Rust 可读取 `current_column()`、`columns()` 或 `cell_keys(column)`。单元格选择跟随记录标识，整列选择在替换行模型后保留。

用 `bazel run //examples/table:table -- --cells` 或 `bazel run //examples/table:table -- --columns` 体验两种选择方式。

`TableCell { text <=> item; }` 可编辑 `for item in root.records` 中可写 `[string]` 模型的一行。双击单元格，或在聚焦后按 Enter / F2 开始编辑。Enter 提交，Tab 提交并移动焦点，Escape 或切换到其他控件会丢弃草稿。应用在编辑期间重排模型时，仍可见的记录会按行标识保留草稿。滚出视口、删除记录或外部改值会结束编辑。文本输入、IME、剪贴板和撤销在草稿上工作，提交时才修改模型。

结构化行可用 `export struct StockRow { name: string, units: int }` 声明，`property <[StockRow]>` 接收 `StockRow { name: "交换机", units: 12 }` 等记录字面量。在 `TableRow` 的直接子元素中，`TableCell { text <=> row.name; }` 编辑字符串，`SpinBox { value <=> row.units; }` 编辑整数，`Slider { value <=> row.load; }` 编辑浮点数，`CheckBox { checked <=> row.available; }` 或 `Switch` 编辑布尔值。提交核对行标识和被编辑字段的原值，再把改动合并进最新记录。Rust 使用生成的 `StockRow` 和 `VecModel<StockRow>`。运行 `bazel run //examples/table:records` 可体验独立字段编辑和按类型排序。

`background` 和 `border-color` 使用 `brush` 类型，纯色可直接绑定。线性渐变使用 `@linear-gradient(90deg, #536dfe 0%, root.accent 100%)`，径向渐变使用 `@radial-gradient(circle, #fff, #536dfe)`，中心和半径由元素尺寸决定。色标颜色可以引用属性，省略的位置会均匀插值，显式位置须在 0% 到 100% 之间且按升序排列。`Rectangle` 和 `Button` 可以用 `border-top-left-radius` 等属性分别覆盖四角的 `border-radius`。

`Image` 的 `source: @image-url("assets/icon.svg");` 引用相对于 `.xr` 的本地 PNG 或 SVG。图片以 `!xr.image` 通过 MLIR，编译器检查资源后把编码字节嵌入 Rust。支持 `image-fit: fill | contain | cover | preserve`。布局内默认 `contain`，其他位置默认 `fill`。SVG 支持独立的矢量图形，由 resvg 按显示尺寸和 DPI 栅格化。SVG 文本和外链资源暂未支持。

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

## 异步 Rust

`xrack_platform_winit::spawn_local` 在运行 XRack 的 UI 线程上轮询 `Future + 'static`，支持捕获 `Rc` 等 `!Send` 状态。可以从 `run_with_messages` 的 setup、UI 回调或其他本地任务启动。任务返回 `LocalTask<T>`，可 `.await` 结果或调用 `abort()`，丢弃句柄会让任务继续运行。窗口事件循环退出时会取消未完成的本地任务。生成组件的 `as_weak()` 可用于在 `.await` 后重新取得组件。

`run_with_messages(component, setup)` 把 `UiSender<M>` 交给 setup，setup 返回 `FnMut(&mut Component, M)` 消息处理器。工作线程或应用自己的异步运行时可调用 `try_send`。队列满或关闭时会把消息随错误返回。事件循环每批处理最多 64 条消息，再统一刷新场景和布局。默认队列容量是 256，组件状态和 GPU 绘制留在 UI 线程。

`bazel run //examples/async_demo:async_demo` 使用一个由应用持有的多线程 Tokio runtime 运行计时和后台搜索。UI 本地 Future 等待 Tokio 计时任务的 `JoinHandle`，完成后在 UI 线程更新组件。搜索通过 `UiSender` 提交列表和逐行更新，用请求 ID 丢弃过期结果。应用可以按自己的 I/O 需求启用 Tokio 的驱动。

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

## MLIR 阶段

```text
.xr → ANTLR4 C++ parse tree → AST JSON → Rust AST
    → 语义分析 → TableGen builders → xr dialect（UI IR）
    → MLIR verify + XR schema checks
    → xr-fold-constants → canonicalize → CSE → verify
    → 按 operation 和 SSA use-def 生成 proc_macro2::TokenStream
    → xrackc 输出 Rust 源码
```

`parser/bridge.cc` 调用 ANTLR 的 `document()`，遍历解析树并编码 `document` 和 `diagnostics`。Rust 的 `xrack-parser` 通过 C ABI 读取 JSON 缓冲区，用 Serde 反序列化成 `xrack-syntax` 定义的 `ast::Document`，再调用 C++ 的释放函数。`--emit ast` 可以查看单文件的 AST，编译入口还会解析导入并合并文档。

`xr` 方言是编译主链路的 UI IR。语义分析验证 AST 成员并交给原生 MLIR builder。`xr.instance` 以符号引用指定组件，`xr.read` 和 `xr.assign` 携带解析后的属性所属元素或实例。MLIR 会核对引用目标、属性类型和访问权限。表达式使用 SSA operand/result 和 `!xr.int` 等类型，源码位置保留为 MLIR location。Rust 后端通过 MLIR C API 查找 operand 的定义，在 getter、绑定或赋值的使用位置生成表达式，以保留惰性求值、短路和回调中的读写顺序。`--emit hir` 提供前端调试视图。

`mlir/Xrack.td` 使用 TableGen ODS 定义 `xr` 方言。Bazel 通过 `mlir-tblgen` 生成操作和类型的 C++ builder 与校验器。Rust 通过 C ABI 传入 SSA 句柄、类型、属性和位置，C++ 调用生成的 `Op::create` 和 `Type::get`。MLIR printer 负责生成 `--emit mlir-raw` 和 `--emit mlir` 的文本。

`xr-fold-constants` 通过 `mlir/XrackPasses.td` 声明，折叠可表示的整数算术、比较和布尔表达式。可能溢出或除零的算术保持原操作。常量短路按语言规则保留需要执行的分支，动态条件即使两臂相同也会求值。`canonicalize` 简化常量条件选择，`CSE` 合并同类型的相同字面量。`--emit mlir-raw` 输出 native pass 之前的 lowering 结果，`--emit mlir` 输出 Rust 后端消费的优化结果。编译库返回 token stream，`xrackc` 在写入 `.rs` 文件或标准输出时才将其转成文本。

Bazel 使用 `rules_rust_bindgen` 和同一份 LLVM 23 的 clang/libclang 生成 `c_bindings.rs`。`xrack_mlir_bindings` 将它作为 crate 根文件，供 `xrack-mlir` 和 Rust Analyzer 的项目图使用。Rust 持有原生模块，构建和优化都在这个模块上完成。C++ 接口负责注册方言、调用生成的 builder、配置 pass 和跨操作校验。

## 测试

`bazel test //tests:mlir_pass_test` 对同一原生模块断言优化前后的 SSA 和操作结构。用例覆盖常量折叠、CSE、动态分支及跨赋值的读取顺序。

`bazel test //tests:ui_interaction_test` 用控件 ID 驱动点击、键盘导航和文本输入，在确定性的布局下验证计数器、模态对话框与选择控件。该目标可在无 GPU 的 CI 中运行。`bazel test //tests:gpu_test` 额外使用原生图形适配器验证离屏像素和绘制路径。

## 首版边界

当前的功能缺口包括通用双向属性链接、嵌套记录和自定义单元格编辑器、回调别名和子元素上的自定义回调、换行布局、读取布局结果、完整的状态与过渡语法、标准控件库和原生无障碍树。双向文字的精确光标定位尚待实现。文件扩展名约定为 `.xr`，语法取 Slint 风格子集。

`xrack-layout` 计算线性布局、绝对坐标和祖先裁剪。隐藏元素不占布局空间。鼠标点击需要在同一目标内按下和释放，禁用或被裁剪的控件不会触发点击。winit 将鼠标、Tab / Shift+Tab 和 Enter / Space 事件交给该状态机。绑定变化、窗口缩放和 DPI 变化会触发重绘。

原生 GPU 路径目前在 macOS / Metal 验证。离屏 GPU 测试需要适用的图形适配器和支持中文的系统字体，需显式运行。单行文本输入已接入 IME 预编辑和提交，多窗口与原生无障碍树尚待实现。
