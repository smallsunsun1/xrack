# Pocket Tasks · Android

此示例通过 `XRackActivity` 启动 winit，使用 FemtoVG 和 wgpu/Vulkan 绘制。Android 与 iOS 使用同一个 `touch_ui` 目标，界面和任务模型来自 [touch.xr](../ios/touch.xr)。

## 构建与启动

SDK Platform 34、Build Tools 35.0.0、Platform Tools 37.0.0 和 NDK r28c（`28.2.13676358`）由 Bazel 的 `hermetic_android_toolchains` 模块下载并校验，版本声明在 `MODULE.bazel`。构建和设备测试不读取本机 Android SDK／NDK 安装目录，也不需要设置 `ANDROID_HOME`、`ANDROID_SDK_ROOT` 或 `ANDROID_NDK_HOME`。下载归档由 Bazel repository cache 复用，编译产物沿用仓库的 disk cache。SDK／NDK 许可接受版本配置见 `.bazelrc` 的 `ACCEPTED_ANDROID_*_LICENSE_VERSION`。

大型 NDK 归档优先通过 Google 区域 CDN 下载，并保留原地址作为后备，规则位于 `bazel/android_downloads.cfg`。两种地址使用同一个固定版本的归档校验值。

`rules_rust` 会下载 `aarch64-linux-android` 的 Rust 标准库，`rules_android_ndk` 提供链接器和 Android sysroot，因此无需执行 `rustup target add`。真机需要 arm64 和 Vulkan 支持。

```sh
bazel build --config=android --repo_env=CARGO_BAZEL_ISOLATED=false //examples/android:app
apk="$(cd bazel-bin/examples/android && pwd -P)/app.apk"
bazel run --repo_env=CARGO_BAZEL_ISOLATED=false //bazel:adb -- install -r "$apk"
bazel run --repo_env=CARGO_BAZEL_ISOLATED=false //bazel:adb -- shell am start -n dev.xrack.android_demo/dev.xrack.platform.XRackActivity
```

`bazel run --repo_env=CARGO_BAZEL_ISOLATED=false //bazel:adb -- devices` 使用下载的 ADB 列出设备。连接多个设备时，在 `--` 后加 `-s <设备序列号>`。ADB 命令使用宿主平台，不加 `--config=android`。设备回归连接一台已启动的 Android 34 arm64 模拟器；构建 APK 不依赖模拟器或 AVD。

## 模拟器测试

在 macOS 上连接 Android 34 arm64 模拟器，从仓库根目录运行以下命令。各设备测试目标通过 Bazel runfiles 提供下载的 ADB，用 macOS Vision 从渲染画面定位和验证控件。将设备编号替换为设备列表中的实际编号。

```sh
apk="$(cd bazel-bin/examples/android && pwd -P)/app.apk"
bazel test --repo_env=CARGO_BAZEL_ISOLATED=false \
  --test_env=DEVELOPER_DIR \
  --test_arg=--serial=emulator-5570 --test_arg="--apk=$apk" \
  //examples/android:simulator_ui_test
```

测试覆盖列表滑动、用长按菜单完成任务、重新打开任务、横竖屏与前后台切换、唤起键盘并新增任务、数字退格与回车提交，以及多行备注的只读保护和跨页面保留。剪贴板测试用长按菜单从只读备注复制，在任务输入框粘贴、剪切并再次粘贴。失败时可在 Bazel 的测试输出目录检查 `last-screen.png`。宿主端可用 `bazel test //examples/ios:touch_parity_test` 验证共享界面的 AOT 与解释器行为。

用相同的 APK 和参数运行 `//examples/android:row_swipe_ui_test` 可单独验证任务行的 `SwipeDelegate`。横向短拖先预览，松开后回弹。完整滑动露出操作区，反向拖动或系统返回会收起它。从已露出的 **Complete** 按钮上纵向拖动会滚动列表，滚回后原任务仍保留操作区。点击 **Complete** 和 **Reopen** 后检查任务归属、相邻行和计数。设备截图保存在 `swipe-complete.png` 和 `swipe-reopen.png`。

用同样的参数运行 `//examples/android:native_dialog_ui_test` 可单独测试 Android 系统选择器和确认框，包括取消打开、拒绝覆盖、选中两份文件并确认后更新备注。

Android 的 `FileDialog` 支持 `open-file` 与 `open-files`。选择器返回的文档会复制到应用私有缓存，回调得到可用普通文件 API 读取的路径。`MessageDialog` 使用 Android 系统确认框。应用可依赖 `//crates/xrack-platform-winit/android:activity`，并在 Manifest 中使用 `dev.xrack.platform.XRackActivity`。

`DesktopWindow.select_directory()` 打开 Android 系统目录选择器，返回 `AndroidDirectory`。用 `entries()` 列举授权目录，`AndroidDocument.name()` 和 `is_directory()` 读取条目信息，`directory()` 进入子目录，`read_to_string(max_bytes)` 在字节上限内读取 UTF-8 内容。目录内容由文档提供器持有，应用应在工作线程执行 I/O。目录授权随当前运行实例使用。

`DesktopWindow.export_file(path)` 打开系统保存面板，将已有绝对路径文件复制到用户选择的文档。源文件要保留到异步调用结束。保存完成返回 `true`，取消返回 `false`，失败返回错误。示例的 Folder 从所选目录导入 `.txt` 和 `.md`，Save to Files 导出当前备注。目录和导出回归可用同样的参数运行 `//examples/android:storage_ui_test`。

Pocket Tasks 对单批备注应用 64 KiB 的文本上限，包含文件之间的分隔符。设备回归会读取包含 Unicode 的导出文件，检查无效 UTF-8 和整批超限时保留原备注。报错后可继续使用操作按钮，经文本筛选器重新导入刚导出的文件。超限界面保存在 `storage-rejected-note.png`。

`DesktopWindow.share_text(text)` 打开系统分享面板。系统回调确认用户已选择接收应用时返回 `true`，关闭面板时返回 `false`，打开失败时返回错误。接收应用后续的发送或保存由它自身处理。示例的 Share 按钮在选中目标后显示 Share target selected，取消后显示 Share canceled，并保留备注。

分享回归使用本地测试接收应用，检查文本、MIME 类型、取消和返回原窗口后再次分享。先用 Android 配置构建 `//examples/android:app` 和 `//examples/android:share_receiver`，保存两份 APK 的实际路径，再运行以下命令。

```sh
apk="$(cd bazel-bin/examples/android && pwd -P)/app.apk"
receiver="$(cd bazel-bin/examples/android && pwd -P)/share_receiver.apk"
bazel test --repo_env=CARGO_BAZEL_ISOLATED=false \
  --test_env=DEVELOPER_DIR \
  --test_arg=--serial=emulator-5570 --test_arg="--apk=$apk" \
  --test_arg="--share-receiver-apk=$receiver" \
  //examples/android:share_ui_test
```

接收应用仅用于测试，测试结束后会卸载。`XRackActivity` 通过 `InputConnection` 接入输入法预编辑、Unicode 提交、选区和按码点删除。预编辑不写入绑定属性，切换控件时会取消尚未提交的组合内容，旧输入连接的回执会被丢弃。Android 的 UTF-16 选区位置会转换成 XRack 使用的 UTF-8 字节位置。

录入焦点离开编辑器后，Android 的文本输入桥会释放原生视图焦点并收起输入法。焦点回到折叠标题等普通控件时，硬件 Enter 和 Space 由控件处理。

输入法回归使用同一份测试 APK 中的 XRack Test Keyboard，可将上述分享命令的测试目标替换为 `//examples/android:ime_ui_test`。测试通过输入法发送组合和提交操作，检查取消组合、旧连接、emoji 删除和表单提交。测试结束时恢复原来的输入法并卸载测试 APK。

设备测试逐字符生成 ADB 键盘事件，避免模拟器绘制较慢时，整段注入的后部事件因时间戳过期而被系统丢弃。DSL 的 `FileDialog.save-file`、`select-folder` 和 `select-folders` 仍待接入。

## 独立控件回归

[controls.xr](controls.xr) 是面向竖屏的控件测试界面，独立安装为 XRack Controls。它使用普通的双向绑定和回调，覆盖 Slider 的实时吸附、RangeSlider 的释放吸附、Switch、CheckBox、SpinBox、DoubleSpinBox、ComboBox、ProgressIndicator 和 Dialog。静音会禁用音量滑块，Lock inputs 会禁用区间、数值和下拉输入。

```sh
bazel build --config=android --repo_env=CARGO_BAZEL_ISOLATED=false \
  //examples/android:controls_app
controls_apk="$(cd bazel-bin/examples/android && pwd -P)/controls_app.apk"
bazel test --repo_env=CARGO_BAZEL_ISOLATED=false \
  --test_env=DEVELOPER_DIR \
  --test_arg=--serial=emulator-5570 --test_arg="--apk=$controls_apk" \
  //examples/android:controls_ui_test
```

设备测试使用 Android 34 arm64 模拟器的触摸设备，检查拖动后的值、释放次数、禁用保护、数值增减、下拉选项和弹层确认／取消。数值步进箭头不会启动软键盘，点击数值文本后仍可使用键盘编辑。点击弹层外的 Background 按钮时，先关闭弹层，背景计数保持不变。完成后可在测试产物中查看 `controls.png`，失败时查看 `last-screen.png`。运行多个设备测试时使用 `--local_test_jobs=1`，避免它们同时操作同一台模拟器。

`bazel test //examples/android:controls_parity_test` 用同一份 `.xr` 验证 Rust AOT 和 MLIR 解释器的属性与交互结果。它在 macOS 宿主运行；Android APK 使用 AOT。当前设备测试按此示例的竖屏布局定位，尚不代表所有屏幕尺寸和真机输入法均已验证。

Android 系统返回键先处理当前菜单、下拉框或弹层，再返回 `StackView` 的上一页。按下、长按产生的重复事件和松开合起来只关闭一层。弹层的 `close-on-escape: false;` 也会阻止系统返回关闭它。控件回归会检查返回关闭下拉框后保留选中值，以及长按返回关闭对话框时取消回调只触发一次。

**Open wheels** 打开选择面板，用竖直的 `Tumbler` 选择索引，用曲线上的 `PathView` 选择记录的业务值。设备回归检查非循环滚轮在边界停住、循环滚轮越过首尾、卡片镜像后反向滑动，以及禁用期间保持选中值和回调计数。测试还会在手指未松开时切到后台，恢复后检查旧选择保留、暂存的拖动取消，随后验证下一次拖动能正常提交。宿主对照使用同一面板，最终设备画面保存在 `selection-wheels.png`。

**Open gestures** 打开双指缩放和旋转面板。设备测试通过模拟器触摸设备发送两个独立触点。第二指加入后，第一指对 **Tap pad** 的点击候选取消。放开其中一指会结束手势并保留图形变换，仍按住的另一指松开时不会触发点击。双指按住时切到后台会触发一次取消回调，示例恢复手势开始前的变换。返回后可重新轻触按钮。测试输出 `gesture-active.png` 和 `gesture-resumed.png`，宿主对照使用同一份声明与回调。

**Open dial** 打开旋钮面板。圆周模式测试一圈内的绝对定位和跨端点循环，水平与垂直模式测试相对拖动。设备回归确认触点拖出旋钮仍可调节、禁用时忽略触摸，以及按住旋钮切到后台时保留已实时写入的值且不触发松手回调。恢复后可以继续拖动，松手时回调读取最新值。用相同的 APK 和参数运行 `//examples/android:dial_ui_test` 可单独执行，输出 `dial.png`。同一份交互也由 `//examples/android:controls_parity_test` 对照。

**Open options** 打开非互斥 `ButtonGroup` 的导出选项面板。全选框的自定义指示器显示未选、部分选中和全部选中。批量切换会更新禁用的 Manifest 选项和未显示的 Review 选项。拖动 Compress 开关时，滑块先移动，汇总和回调计数在松手后更新。

用控件 APK 运行 `//examples/android:export_group_ui_test`。测试会触摸全选图案、检查三种指示器颜色和拖动预览、验证 Review 的三态循环，并确认锁定后点击全选和成员不会改值或触发回调。设备截图为 `export-group-switch-preview.png` 和 `export-group-cleared.png`。`//examples/android:controls_parity_test` 用同一份 `.xr` 对照 AOT 和解释器。

**Open profile** 打开包含可勾选 `GroupBox` 和嵌套 `Expander` 的连接面板。触摸标题后可编辑目标地址。**Fold target** 折叠高级选项，释放内容空间并把焦点移回标题，按 Enter 可展开并继续编辑草稿。**Disable delivery** 禁用外层分组，保留内容布局和草稿，把焦点交还分组标题。按 Space 可重新启用。输入法收起时也能用 F8 折叠。

用控件 APK 运行 `//examples/android:connection_profile_ui_test`。测试测量展开前后的截图位置，检查两次焦点迁移都收起系统键盘，并在手指按住 **Apply target** 时折叠，确认松手不会保存。设备截图为 `connection-folded-draft.png`、`connection-disabled-group.png` 和 `connection-saved.png`。`//examples/android:controls_parity_test` 用同一份 `.xr` 对照 AOT 和解释器。

**Open holds** 打开按钮按压面板。短触 **Inspect control** 会按顺序发出按下、释放和点击。按住六秒触发一次 `press-and-hold`，到期前拖出或切到后台会取消手势。示例用六秒阈值给模拟器的系统切换留出时间。**Repeat step** 持续连发，第三次后禁用，松手不会添加点击。

用控件 APK 运行 `//examples/android:button_holds_ui_test`。测试用模拟器触点保持按住，检查回调计数、`pressed` 的可见状态和禁用后的输入拦截。设备截图为 `button-held-once.png`、`button-repeat-capped.png` 和 `button-holds-complete.png`。宿主用同一份 `.xr` 检查 AOT 和解释器的定时边界。

在 **Open holds** 中选 **Open verify** 可打开滚动区域里的 `DelayButton`。连续按住约八秒后确认，确认后轻点即可复位。点击 **Schedule lock** 后立即按住确认按钮，可观察计时完成前由程序禁用按钮并清除进度。

用控件 APK 运行 `//examples/android:delay_confirmation_ui_test`。测试从截图采样空、部分和全部填充的像素，并检查提前释放、滚动接管和程序禁用后的零提交。完成确认后松手会保留选中态，复位不会多触发一次回调。截图为 `delay-filling.png`、`delay-locked.png` 和 `delay-confirmed.png`。

在 **Open holds** 中选 **Open clicks** 可查看双击和程序化点击。**Pair target** 通过共享 `Action` 记录普通激活，双击回调记录它观察到的命令数。**Program click** 同步点击，**Animate twice** 连续调用两次 `animate-click()`，**Cancel queued** 在动画释放前禁用目标。

用控件 APK 运行 `//examples/android:click_routes_ui_test`。测试通过模拟器原始触摸事件，在 450 毫秒内注入各组点按，检查同点双击、位移过大、不同按钮打断、回调顺序以及取消后重新启用。截图为 `click-paired.png` 和 `click-animation-canceled.png`。宿主用同一份 `.xr` 对照 AOT 和解释器。

## 库存表格回归

`table_app` 安装后名为 **XRack Inventory**。`StandardTableView` 用业务键标识库存记录，冻结首列。在单元格内横拉或竖拉可滚动，长按后拖动可跨过冻结边界框选。用 Ctrl+C 复制前两行、前两列，再选中 Gasket 并用 Ctrl+V 粘贴。界面检查目标记录的四个字段。**Edit pump units** 打开整数单元格编辑器。软键盘提交有效草稿后收起，无效草稿留在编辑器里。

```sh
bazel build --config=android --repo_env=CARGO_BAZEL_ISOLATED=false \
  //examples/android:table_app //examples/android:share_receiver
apk="$(cd bazel-bin/examples/android && pwd -P)/table_app.apk"
keyboard_apk="$(dirname "$apk")/share_receiver.apk"
bazel test --repo_env=CARGO_BAZEL_ISOLATED=false --nocache_test_results \
  --test_env=DEVELOPER_DIR \
  --test_arg=--serial=emulator-5570 --test_arg="--apk=$apk" \
  --test_arg="--keyboard-apk=$keyboard_apk" //examples/android:table_ui_test
```

设备测试使用测试输入法触发原生提交，结束后恢复原输入法。产物包括 `table-selection.png` 和 `table-edited.png`。在宿主上运行 `//examples/android:table_parity_test` 可对照同一份表格的 AOT 和解释器结果。

## 树形目录回归

在 **XRack Inventory** 中点 **Open directory** 打开检查目录。展开 Workshop 和 Reports 后选择 Pump check，折叠 Workshop 会将光标移回可见祖先，同时保留选中的业务键。**Find old notes** 显示命中文档及其祖先，**Show all** 恢复之前的展开状态。**Insert report** 在选中记录前插入一个同级节点，**Mirror tree** 将展开按钮移到右侧。

使用库存 APK 的路径运行 `//examples/android:tree_ui_test`，传入 `--serial` 和 `--apk`，目录用例可直接运行。测试通过真实触点操作自定义展开按钮，并核对选中键和回调计数。最终画面保存在 `tree-rtl.png`。宿主对照由同一个 `//examples/android:table_parity_test` 执行。

## 分栏布局回归

在 **XRack Inventory** 中点 **Open split** 打开嵌套分栏。外层分隔线左右调整 Browse 的宽度，内层分隔线上下调整 Edit 和 Result 的高度。外层可见分隔线宽 8 个逻辑像素，两条分隔线的触摸热区均为 36 个逻辑像素。面板显示实际布局宽高与正在调整的轴。**Save split** 和 **Restore split** 保存、恢复两层分栏，**Mirror panes** 切换阅读方向，**Lock split** 禁用拖动。

用库存 APK 的路径运行 `//examples/android:split_ui_test`，传入 `--serial` 和 `--apk`。测试从可见分隔线旁起拖，验证热区、横竖轴隔离、最小与最大尺寸、RTL 拖动及保存恢复，输出 `split-restored.png`。宿主使用 `//examples/android:table_parity_test` 对照相同的触摸命中和布局结果。

## 应用内拖放回归

在 **XRack Inventory** 中点 **Open transfer**。轻触 **Card A** 只触发点击。拖到 **Other bay** 会因键不匹配而取消。**Copy bay** 中有一个不接受这张卡的子目标，外层仍可接收，松手后源卡保留。拖到 **Archive bay** 并松手会移动卡片。面板通过完成回调显示收到的字符串和操作计数。

沿用库存 APK 和设备参数运行 `//examples/android:transfer_ui_test`。测试还会在卡片停留在归档区且未松手时切到后台，验证取消只触发一次并保留源卡。恢复后完成移动，输出 `transfer-archived.png`。AOT 与解释器对照由 `//examples/android:table_parity_test` 执行。

## 画布滚动回归

在 **XRack Inventory** 中点 **Open canvas**。轻触 **Inspect tile** 增加点击计数，从按钮斜向拖动则同时移动 `Flickable` 的两条轴。**Across** 和 **Down** 是附着到同一视口的自定义滑块，会随内容同步移动。勾选 **Lock pan** 后仍可轻触按钮和拖动滑块。将两条滑块拉到末端可以看到 **Far corner**，点 **Reset canvas** 回到原点。

沿用库存 APK 和设备参数运行 `//examples/android:canvas_ui_test`。测试通过触摸验证按钮与拖动的仲裁、两轴的偏移写回、附着滑块、边界裁剪和程序复位，输出 `canvas-corner.png`。同一画布也由 `//examples/android:table_parity_test` 在 AOT 和解释器中验证。

## 零件网格回归

在 **XRack Inventory** 中点 **Open catalog**。轻触 **Valve** 卡片选中键 `20`，再点 **Pin Valve** 收藏。可以从复选框上起拖，将卡片滚出视口。**Move Valve** 把记录从索引 `1` 移到 `10` 并按键定位。**Hide Valve** 暂时从网格中筛掉该记录，**Show Valve** 恢复显示。面板显示当前业务键、显示索引和用户选择次数。

沿用库存 APK 和设备参数运行 `//examples/android:catalog_ui_test`，验证虚拟单元格离屏、移动和过滤后，选择与卡片内的收藏状态都随记录保留。测试还会收藏邻近的 **Panel**，核对两个实例的状态和网格选择互不干扰。结果保存在 `catalog-restored.png`。宿主对照由 `//examples/android:table_parity_test` 执行。

## 检查记录分段回归

在 **XRack Inventory** 中点 **Open log**，向上拖动 **Intake** 组的记录。当前组的标题会固定在列表顶部，继续拖动时由 **Bench** 的标题接替。**Insert intake** 在列表开头插入一条记录，**Hide bracket** 和 **Show bracket** 切换前部记录的可见性。保留滚动锚点后，当前正在阅读的 **Seal intake** 不会随前部记录移动。

沿用库存 APK 和设备参数运行 `//examples/android:inspection_log_ui_test`。测试核对自定义标题的吸顶和切换、标题条数、阅读位置，以及程序修改模型时 `scrolled()` 计数保持稳定。产物包括 `log-anchored.png` 和 `log-bench-pinned.png`。宿主对照由 `//examples/android:table_parity_test` 执行。

## 富文本触摸回归

在 **XRack Inventory** 中点 **Open bulletin**。轻触 **Procedure** 或 **Limits** 会显示链接目标和激活次数。**Compact text** 只保留第一行，**Show full text** 恢复全文。长按链接文字可选中词、打开 **Copy** 菜单，并把选中的纯文本粘贴到下方输入框。

沿用库存 APK 和设备参数运行 `//examples/android:bulletin_ui_test`。测试会点按截断前的链接位置，核对隐藏的链接没有被激活，再通过 Android 剪贴板依次复制和粘贴两个词。结果保存在 `bulletin-pasted.png`。宿主对照由 `//examples/android:table_parity_test` 执行。

## 日期与时间回归

用 Pocket Tasks 的 APK 路径运行 `//examples/android:picker_ui_test`，参数与基础模拟器测试相同。测试操作共享的 [touch.xr](../ios/touch.xr)，覆盖长按任务菜单的返回关闭、日期范围两端的月份导航、闰日、禁用周末、草稿取消与确认，以及 12 小时制的 AM／PM 范围校验。还会在时间草稿未确认时旋转屏幕，再通过系统返回取消，检查已保存的日期和时间保留。

```sh
apk="$(cd bazel-bin/examples/android && pwd -P)/app.apk"
bazel test --repo_env=CARGO_BAZEL_ISOLATED=false --nocache_test_results \
  --test_env=DEVELOPER_DIR \
  --test_arg=--serial=emulator-5570 --test_arg="--apk=$apk" \
  //examples/android:picker_ui_test
```

测试结束会输出 `date-picker.png`、`time-picker.png`、`time-picker-landscape.png` 和最终页面 `pickers.png`。日期和时间控件在 AOT 与解释器中的草稿隔离、取消、禁用值与窄屏／横屏布局由 `//examples/ios:touch_parity_test` 对照验证。

## 页面导航与触摸回归

[navigation.xr](navigation.xr) 使用 RadioGroup、TabWidget、SwipeView、PageIndicator、StackView 和 Drawer。页内的 TapCounter 自己持有计数，切换标签、滑动卡片和从预览返回详情后，可以检查局部组件状态是否保留。Quiet 单选项和 Disabled 标签被禁用，Lock swipes 会同时禁用手势切页与分页圆点。

```sh
bazel build --config=android --repo_env=CARGO_BAZEL_ISOLATED=false \
  //examples/android:navigation_app
navigation_apk="$(cd bazel-bin/examples/android && pwd -P)/navigation_app.apk"
bazel test --repo_env=CARGO_BAZEL_ISOLATED=false --nocache_test_results \
  --test_env=DEVELOPER_DIR \
  --test_arg=--serial=emulator-5570 --test_arg="--apk=$navigation_apk" \
  //examples/android:navigation_ui_test
```

APK 安装后名为 XRack Navigation。测试覆盖单选与禁用项、标签切换、滑动与圆点选择、边缘拖动打开抽屉、抽屉内的嵌套对话框、系统返回以及前后台切换。`navigation.png`、`navigation-drawer.png` 和 `navigation-help.png` 保留渲染结果。当前界面按竖屏安排；宿主端的 `//examples/android:navigation_parity_test` 用同一份 `.xr` 对照 AOT 与解释器。

系统返回首先跟随焦点所在的 `StackView`。当焦点位于页外，例如刚关闭由全局按钮打开的抽屉时，会回退窗口内唯一可返回的活动栈。嵌套栈优先处理内层；隐藏、禁用和离屏的栈不会成为默认候选。多个独立活动栈同时可返回、且焦点未归属其中任何一个时，保持页面不变。菜单和弹层仍先于页面栈处理。

## 动态加载回归

在 **XRack Navigation** 中从 **Open menu** 进入 **Open tools**。**Load pressure** 和 **Load flow** 延迟挂载各自的工具实例。录入读数后点 **Switch on press**，下一次按住 **Record Pressure** 会切换到 Flow。松手时新工具的记录计数保持为零。按 Enter 可激活新实例获得焦点的按钮。**Missing source** 演示来源错误，**Unload tool** 清除挂载，再加载会初始化新的局部计数。

沿用导航 APK 和设备参数运行 `//examples/android:loader_ui_test`。测试检查真实触摸在换页前后的归属、`loaded()` 计数、焦点和卸载后的命中结果。最终画面保存为 `loader-remounted.png`。`//examples/android:navigation_parity_test` 对照 AOT 与解释器，并在宿主上逐帧检查延迟构造与取消。

## 动态卡片回归

在 **XRack Navigation** 中从 **Open menu** 进入 **Open cards**。**Add cards** 同步挂载 North、延迟挂载 South。点击 North 后用 **North down** 调整顺序，按 Enter 仍会更新 North 的计数。把 North 移回顶部后点 **Arm removal**。短按继续计数，长按则由定时器移除 North。移除时继续按住，随后松手，顶上来的 South 保留自己的计数。

用 **Add fresh card** 在原位置创建新 North。**Try old handle** 用已失效的句柄尝试写入属性、调用回调，界面显示默认结果 `-1`，两张当前卡片的计数保持原值。沿用导航 APK 和设备参数运行 `//examples/android:dynamic_cards_ui_test`，最终画面保存为 `dynamic-cards-remounted.png`。`//examples/android:navigation_parity_test` 使用同一场景对照 AOT 与解释器。

## 图像和 GIF 回归

在 **XRack Navigation** 中从 **Open menu** 进入 **Open images**。左侧图片从同一张 PNG 图集选择上排或下排，**Use cover** 和 **Use contain** 切换铺满和等比留边。右侧播放内嵌 GIF，**Pause pulse** 暂停，**Resume pulse** 继续，**Stop pulse** 返回首帧。每次播放一轮，完成后保留末帧。

沿用导航 APK 和设备参数运行 `//examples/android:images_ui_test`。测试直接对照模拟器截图的像素，检查留边、居中裁剪、图集切换和 GIF 的首末帧，也核对暂停和再次播放的完成计数。产物包括 `images-contained.png` 和 `images-finished.png`。`//examples/android:theme_parity_test` 对照两种后端的资源尺寸、裁剪参数和播放时序。

## 可编辑下拉框与输入法回归

在 **XRack Navigation** 中从 **Open menu** 进入 **Open choices**。编辑框采用记录模型，界面分别显示已经提交的业务值、草稿和回调计数。输入 `47` 后点 **Choices**，弹层高亮启用的 **47 Service**，**47 Hold** 是可见的禁用项。

使用导航 APK 和上文的测试输入法 APK 运行 `//examples/android:combo_ime_ui_test`，同时传入 `--apk` 和 `--keyboard-apk`。设备回归检查 `ni` 预编辑、提交 `47`、点击禁用和启用选项，以及通过输入法提交 `你好🚰` 并匹配业务值。测试结束后恢复系统输入法。截图为 `choice-ime-popup.png` 和 `choice-ime-accepted.png`。`//examples/android:navigation_parity_test` 对照两种后端的草稿、补全和提交行为。

## 搜索候选回归

在 **XRack Navigation** 中从 **Open menu** 进入 **Open search**。先输入 `me`，再追加 `t`。示例用定时器先送达 `met` 的候选，五秒后送达较早的 `me` 批次。按住 **Meter kit** 等待旧批次到达，可以观察菜单收起和触摸取消。追加 `er` 后，`meter` 的新候选会出现。

沿用导航 APK 和设备参数运行 `//examples/android:search_results_ui_test`。测试检查迟到的候选、松手后的提交计数、选中 **Meter sensor**、Ctrl+Z 撤销选择以及 Enter 提交查询。产物为 `search-result-selected.png` 和 `search-query-submitted.png`。`//examples/android:navigation_parity_test` 使用同一份界面对照 AOT 和解释器。

## 锚定弹层回归

在 **XRack Navigation** 中从 **Open menu** 进入 **Open popup**。用 **Show actions** 打开弹层，点弹层内的 **Move anchor** 将锚点移到右下方。弹层会跟随锚点，空间不足时换到上方。移动后用 **Apply action** 核对新的触摸区域。

在普通模式下，点 **Count outside** 会关闭弹层并增加背景计数。打开 **Modal popup** 后，同一个触点会关闭弹层，背景计数保持原值。Android 返回键逐层关闭弹层和工作台。沿用导航 APK 和设备参数运行 `//examples/android:popup_window_ui_test`，产物为 `popup-flipped.png` 和 `popup-back.png`。宿主的 `//examples/android:navigation_parity_test` 也会对照布局和焦点恢复。

## 触摸菜单栏回归

在 **XRack Navigation** 中从 **Open menu** 进入 **Open commands**，点 **Commands** 打开菜单栏弹层。主菜单和 **View mode** 子菜单的触摸行高都是 44 个逻辑像素。**Compact**、工具栏按钮和 Ctrl+2 通过同一命令组更新选中值与工具栏状态。**Lock modes** 禁用组内的命令入口。

沿用导航 APK 和设备参数运行 `//examples/android:command_menu_ui_test`。测试通过截图测量行高，核对禁用项、命令与命令组的回调次数及顺序，再用返回键依次收起子菜单、主菜单和工作台。截图为 `touch-menu-submenu.png` 和 `touch-menu-complete.png`。宿主的 `//examples/android:navigation_parity_test` 对照两种后端的动作和行高。

## 计时与进度回归

导航示例的 **Open scan** 打开采样面板。**Start scan** 启动重复计时器，采到两份时暂停，**Continue scan** 继续到四份。横向 `ProgressIndicator` 与圆形 `Spinner` 显示同一进度。**Queue report** 启动单次计时器，等待期间 `BusyIndicator` 会转动。返回键关闭面板后会撤销调度，重新打开时保留采样进度，并重新等待尚未交付的报告。

使用导航 APK 运行 `//examples/android:scan_ui_test`。设备测试直接采样截图像素，检查空轨、半程填充、满进度以及忙碌弧在无新输入时旋转和完成后消失。`//examples/android:navigation_parity_test` 使用同一份 `.xr`，用可控时钟检查 AOT 和解释器的精确到期边界、关闭后的调度和单次回调顺序。设备截图为 `scan-halfway.png`、`scan-report-pending.png` 和 `scan-report-complete.png`。

## 焦点作用域与按键路由回归

导航示例的 **Open focus** 打开两个嵌套 `FocusScope` 组成的录入面板。**Enter fields** 将焦点给上一次使用的输入框，首次进入或备忘框已禁用时会聚焦编号框。Tab 和 Shift+Tab 在编号和备忘之间切换。按 X 时外层先捕获，内层随后拦截，输入框保留原值。按 F2 时事件先经捕获，再由内层冒泡到外层。焦点移出作用域后，F2 不再触发这组处理器。

使用导航 APK 运行 `//examples/android:focus_routes_ui_test`。测试通过 Android 输入分发器发送按键，检查系统键盘显示与收起、恢复前台后继续向原输入框输入，以及禁用旧目标后的焦点回退。`//examples/android:navigation_parity_test` 用同一份 `.xr` 对照 AOT 和解释器。设备截图为 `focus-key-route.png` 和 `focus-disabled-fallback.png`。

## 颜色与字体回归

XRack Navigation 的 Settings 页提供 Pick color 和 Pick font。颜色选择器支持十六进制校验、通道数值和 HSV 色板，确认后更新页面上的色块；字体选择器使用 Android 字体目录，确认后应用字体族、字号、字重和斜体。

使用导航 APK 的路径运行 `//examples/android:theme_ui_test`，参数与导航设备测试相同。测试通过长按编辑菜单替换内容，检查无效十六进制不能确认、色板拖动更新预览、取消保留原值，以及确认后的原生窗口色块像素。字体流程选择真实目录中的 Roboto，再编辑字号、字重和斜体。测试还检查输入法显示时操作按钮仍在键盘上方，以及横屏时操作区可见。

颜色和字体弹层的操作按钮保持可见，内容超出空间时可滚动。`XRackActivity` 将实际输入法区域通知原生窗口；Android 30 及以后使用 IME WindowInsets，26–29 使用可见窗口矩形估算。窗口可用区域与键盘的关系见 [Android 输入法文档](https://developer.android.com/develop/ui/views/touch-and-input/keyboard-input/visibility)。当前设备回归仍限于 Android 34 arm64 模拟器。输入法候选区域如果只是标记已有文本，在长按选择时可以完成标记；真正未提交的预编辑仍保留。SpinBox 与 DoubleSpinBox 的文本区也支持长按选字和编辑菜单，箭头区域继续执行步进。

`//examples/android:theme_parity_test` 对照 AOT 和解释器的草稿、无效输入、旧会话拒绝和整值提交，并检查窄屏、横屏下编辑字段可滚动访问。设备测试输出 `theme-color.png`、`theme-font.png`、`theme-landscape.png` 和 `theme-result.png`。

## 键盘快捷键回归

Android 平台层跟踪左右 Ctrl、Shift、Alt 和 Meta，在窗口失焦或挂起时清除状态。编辑键会恢复为命名按键，Ctrl+A–Z 的控制字符会恢复为快捷键身份；快捷键事件的文本载荷不会进入编辑属性。AltGr 产生的可打印字符仍沿用输入法映射。

Android 可能在下一次触摸按下后才交付前一个快捷键的松键事件。松键不会中断已经开始的触摸手势，因此快捷键之后点击按钮仍会正常触发。平台宿主单测覆盖这一交错顺序，设备测试还会在只读文本全选后切换页面。

```sh
bazel build --config=android --repo_env=CARGO_BAZEL_ISOLATED=false \
  //examples/android:app //examples/android:navigation_app
apk="$(cd bazel-bin/examples/android && pwd -P)/app.apk"
navigation_apk="$(cd bazel-bin/examples/android && pwd -P)/navigation_app.apk"
bazel test --repo_env=CARGO_BAZEL_ISOLATED=false --nocache_test_results \
  --test_env=DEVELOPER_DIR \
  --test_arg=--serial=emulator-5570 --test_arg="--apk=$apk" \
  --test_arg="--navigation-apk=$navigation_apk" \
  //examples/android:keyboard_ui_test
```

测试通过 Android 输入分发器注入按键组合，检查 Ctrl+A/C/X/V、Ctrl+Z、Ctrl+Shift+Z、Ctrl+Y 和 Shift+左箭头选字，并检查只读编辑器可以复制而不能剪切、粘贴或撤销。导航示例的独立 `Shortcut { sequence: "Ctrl+K"; … }` 更新 Hotkeys 计数。测试还会在按住 Ctrl 时切到桌面，释放后恢复窗口，检查普通 K 不会继续触发快捷键。`keyboard-shortcuts.png` 和 `keyboard-action.png` 保存最终状态。AltGr 字符保留由宿主单测覆盖，目前尚未用实体键盘验证不同键盘布局。
