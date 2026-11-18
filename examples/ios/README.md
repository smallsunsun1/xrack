# Pocket Tasks

XRack 的 iOS 模拟器任务看板。巡检进度、待处理和已完成列表、任务录入与切换都写在 `touch.xr`。竖屏上下排列，横屏用同一份界面和模型切换成双栏。构建时，macOS 上的 `xrackc` 生成 Rust，Bazel 用 `rules_rust` 交叉编译静态库，再用 `rules_apple` 打包 iOS 应用。模拟器里由 winit 的 UIKit 后端处理窗口和输入，FemtoVG 通过 wgpu/Metal 绘制。

iOS 无障碍树会逐项公开可见任务的标题、位置和当前状态。辅助功能可以激活待办项来完成任务，也可以在已完成列表激活它来重新打开。向某个可见任务发送滚动命令时，任务列表会滚动并公开新出现的任务。

长按任务行可以打开上下文菜单，点 **Mark complete** 完成任务，或在已完成列表中点 **Reopen task** 重新打开。松手时任务状态保持不变，选择菜单项后才提交。短按仍可直接切换状态，在任务行上滑动则会滚动列表。

向左滑动任务行会露出 **Complete**，已完成任务会露出 **Reopen**。滑动本身保留任务状态，点按钮才提交。也可在操作按钮上向右滑动收起操作区，或使用行尾的 **Show actions** 无障碍入口。

连接外接键盘后，可以用 Tab / Shift+Tab 移动焦点，回车或空格激活按钮。在任务行或行内按钮上，左方向键展开操作区，右方向键或 Escape 收起，展开和收起本身不会完成任务。文本框支持方向键移动光标、Shift 扩展选区和 Command+A 全选；iOS 的 `Primary` 快捷键使用 Command。输入法正在组字时，文字和候选选择继续交给 UIKit。

完成或重新打开任务时，进度条会用 800 毫秒过渡到新值。iOS「设置 → 辅助功能 → 动态效果」开启 **减弱动态效果** 后，进度条直接显示新值，关闭后后续操作恢复动画。应用在回到前台时也会重新读取设置。

可在 iOS「设置 → 辅助功能 → 显示与文字大小 → 更大字体」调整字号，再返回应用查看。文字绘制和输入选区会使用系统缩放后的字号。模拟器也可执行 `xcrun simctl ui <设备标识> content_size extra-extra-extra-large`，用 `large` 恢复默认类别。

点右上角的 **Notes** 进入巡检备注，可以用系统键盘输入多行内容，手动输入分类或从下拉列表选择。打开 **Read-only** 后，备注仍可查看。切回 **Tasks** 再返回，或者旋转设备，备注内容会保留。

在备注里长按一个词可选中文字，也可保持手指接触并拖动来扩展选区。松手后可以重新拖动选区两端的手柄，也可以从编辑菜单复制、剪切、粘贴或全选。只读时仍可调整选区并复制。

从其他应用复制文字后，可以在备注或任务输入框选择 **Paste**。出现 iOS 授权弹窗时，点允许粘贴会继续替换原选区，拒绝则保留原文。

在备注页点 **Import**，可以从 Files 多选 UTF-8 的 `.txt` 和 `.md`。选择后，系统提示框会显示文件数量。点 **Yes** 合并所选文件并替换当前备注，点 **No** 或 **Cancel** 保留原备注。示例总计最多读取 64 KiB，读取失败也会保留原备注。示例开放了自己的 Documents 目录，方便通过 Files 的 **On My iPhone** 提供本地测试文件。

点 **Folder**，从 Files 进入一个目录后点 **Open**。示例按文件名顺序合并该目录第一层的 UTF-8 `.txt` 和 `.md`，总计最多 64 KiB，全部读取成功后才替换备注。取消、空目录、读取失败或读入期间修改了备注时，编辑器保留当前内容。

写好备注后，点 **Share** 可把当前文本交给 iOS 分享面板，点 **Save to Files** 可将 UTF-8 文本保存为 `inspection-notes.txt`。示例把临时源文件保留到 Files 面板关闭，随后自动清理。保存或取消均保留编辑器内容。

点 **Readings** 录入样本数、压力和温度偏移。三项分别使用整数键盘、小数键盘和可输入负号的键盘。压力字段用逗号显示小数，键盘的英文点号也可正常录入。输入草稿后直接点步进箭头或按外接键盘的上下键，会提交草稿步进后的值。点 **Done** 或切到另一项可直接提交输入，已提交值显示在表单下方。

**Readings** 下方的 **Code**、**Reading** 和 **Find** 演示单行表单。编号至少需要两位，读数须在 1 到 10 之间。数字键盘的 **Done**、系统搜索键和外接键盘 Return 共用提交规则。成功后收起相应的键盘，校验失败时可以继续编辑。结果显示在下方。任务页的 **New task** 也可以直接用软键盘回车添加。

在 **Readings** 点 **Contact details**，可试用邮箱、电话和网址键盘。邮箱键盘的 **Next** 提交当前值并移到电话。电话字段至少填三个字符后，工具栏的 **Next** 才会移到网址。网址键盘提供 `/`，用 **Done** 提交并收起键盘。带国家区号和分隔符的电话号码可以原样保留。

录入联系人时可以旋转到横屏。弹层在可用高度变小时收紧内边距，表单在 `ScrollView` 内滚动，**Next** 会将目标字段带到键盘上方。收起键盘后，可继续检查已填写的字段或关闭表单。

**Readings** 可以滚动。编辑 **Reading** 时旋转到横屏，输入框会自动滚到 Done 工具栏上方，保留原输入和焦点。键盘占用空间时，下面的日期、时间和提交结果仍可滚动访问。

在 iPad 上编辑 **Find**，把浮动键盘拖到输入框原位，表单会临时移开内容。拖走键盘后，输入框恢复原位置。可用 iPad mini (A17 Pro) 模拟器运行下文的 HID 测试，加上 `--test_arg=--floating-keyboard` 验证这个场景。

**Readings** 里也可以点当前日期或时间来安排巡检。日历允许选闰日，周末置灰。时间弹层提供小时、分钟、秒和 AM/PM 四个滚轮，确认范围是 08:00 至 18:00。系统开启「减弱动态效果」时，拖动滚轮松手即就近落位。滚轮仍在减速时，可以用 Tab 切到 **Save time**，按 Return 或空格确认当前可见行。点 **Cancel** 或按 Esc 保留原值。横屏时，日期弹层有足够宽度便会把操作按钮移到日历旁边。

## 构建并运行

从仓库根目录在 Apple Silicon Mac 上执行。请使用与当前 macOS 兼容的 Xcode，并安装 iOS 18.2 或更高版本的 Simulator 运行时。Bazel 通过默认的 `host_xcodes` 发现 Xcode。有多个版本时，可以用 `xcode-select` 选择默认版本。

```sh
bazel run --config=ios-sim //examples/ios:simulator \
  --@rules_apple//apple/build_settings:ios_simulator_device="iPhone 16 Pro" \
  --@rules_apple//apple/build_settings:ios_simulator_version=18.2
```

可以按本机的 `xcrun simctl list devices available` 结果修改设备和版本。用 `bazel build --config=ios-sim //examples/ios:simulator` 可单独构建应用包。用 `bazel cquery --config=ios-sim --output=files //examples/ios:simulator` 查看 `simulator.ipa` 的实际路径。解包后的 `Payload/PocketTasks.app` 可由 `xcrun simctl install` 安装，再用 `xcrun simctl launch <设备标识> dev.xrack.ios-demo` 启动。

## 测试

```sh
bazel test //examples/ios:touch_parity_test
bazel test --config=ios-sim //examples/ios:simulator_ui_test \
  --@rules_apple//apple/build_settings:ios_simulator_device="iPhone 16 Pro" \
  --@rules_apple//apple/build_settings:ios_simulator_version=18.2
```

第一项用同一份 `.xr` 测试 AOT 和 MLIR 解释器，覆盖任务操作、多行备注、可编辑分类、行内按钮键盘导航，以及日期和时间草稿。第二项通过 XCUITest 驱动软键盘、外接键盘、长按菜单和滑动操作区，也检查闰日和禁用周末的无障碍元素、横屏编辑读数，以及拖动时间滚轮后保存。

原生 XCTest 由 `rules_apple` 为每次执行创建临时模拟器，测试结束后清理。测试应用从本次构建安装，界面和输入状态也从新设备开始。临时设备使用英文软件键盘来验证多行备注输入。模拟器型号和运行时仍通过上面的 Bazel 设置选择。

实际 HID 按键另有一个测试目标。先构建上面的模拟器 IPA，再用宿主配置运行。测试需要安装支持 `ui key --command` 的 fb-idb 和 `idb_companion`，并显式指定测试设备；它不会操作桌面上当前聚焦的应用。将以下路径替换为实际产物和工具路径：

```sh
bazel test //examples/ios:simulator_hardware_keyboard_test \
  --test_arg=--ipa=/absolute/path/to/simulator.ipa \
  --test_arg=--udid=SIMULATOR-UDID \
  --test_arg=--idb-python=/absolute/path/to/python3 \
  --test_arg=--idb-companion=/absolute/path/to/idb_companion
```

若 fb-idb 安装在单独目录，可加 `--test_arg=--idb-site-packages=/absolute/path/to/site-packages`。测试安装并校验 IPA 中的可执行文件，覆盖光标移动、Command+A、Tab 后回车添加任务、Escape 收起操作区，以及表单校验、键盘收起和多行回车换行。读数页的搜索测试通过 UIKit 无障碍定位提交和清空按钮，并验证清空、撤销和继续编辑。测试还会在横屏完成一项任务，按 Home 再回到应用，核对进程、列表和任务数量。Xcode 27.2 搭配 iOS 18.2 时，XCTest 的硬件键注入和 UIKit 日志回调曾影响这两类测试，因此由独立驱动执行。测试结束后恢复示例初始状态，截图和无障碍树保存在测试输出中。

## 手机端现状

### iPad 分栏示例

**XRack Split** 复用 [桌面固定侧栏示例](../split/split.xr) 的布局。分隔线宽 8 点，触摸目标宽 44 点。支持拖动、外接键盘步进、切换布局方向和保存恢复尺寸。可移动分隔线也会在 UIKit 无障碍树中提供调节操作和当前尺寸。

```sh
bazel run --config=ios-sim //examples/ios:split_simulator \
  --@rules_apple//apple/build_settings:ios_simulator_device="iPad mini (A17 Pro)" \
  --@rules_apple//apple/build_settings:ios_simulator_version=18.2
```

可用同一个 HID 测试目标验证此应用。将 `--ipa` 换成 `split_simulator.ipa`，选择 iPad mini 模拟器，再加上 `--test_arg=--split-demo`。

### 移动控件示例

**XRack Controls** 复用 [Android 控件示例](../android/controls.xr)。区间滑块的两个手柄初始都在 50，向左拖动调整下限，向右拖动调整上限。两个手柄各自向 UIKit 提供可调节的无障碍元素。

```sh
bazel run --config=ios-sim //examples/ios:controls_simulator \
  --@rules_apple//apple/build_settings:ios_simulator_device="iPhone 16 Pro" \
  --@rules_apple//apple/build_settings:ios_simulator_version=18.2
```

在 **Open wheels** 面板中，曲线卡片选择器 **Route cards** 向 UIKit 提供一个可调节元素，当前值是选中的卡片名称。可以拖动切换卡片，也可以勾选禁用和镜像选项，体验它们对控件的影响。

使用 HID 测试验证时，将 `--ipa` 换成 `controls_simulator.ipa`。使用 `--test_arg=--range-demo` 验证双手柄滑块，或使用 `--test_arg=--path-view-demo` 验证曲线卡片选择器。

### 滚动条示例

构建并安装上面的 **XRack Controls** 后，可以用启动参数打开同一份 [滚动条示例](../list/list.xr) 的两种界面。

```sh
xcrun simctl launch --terminate-running-process <设备标识> dev.xrack.ios-controls --scroll-bars
xcrun simctl launch --terminate-running-process <设备标识> dev.xrack.ios-controls --standalone-scroll
```

第一种界面把横向和纵向滚动条接到同一个可滚动画布，第二种界面用独立的比例值移动内容。拖动滑块，或聚焦后按方向键，可以看到界面和 UIKit 无障碍值同步更新。HID 测试使用同一份 IPA 和 `--test_arg=--scroll-bar-demo` 验证两种界面。

### 上下文菜单和表格行操作

**Inspection queue** 展示上下文菜单和可移动表格行。长按 **Review options** 或用 VoiceOver 的 **Show context menu** 打开菜单。启用 VoiceOver 时，聚焦行标题可以在「操作」转子中向上或向下移动该行。 点 **Instrument** 切换升序和降序，点 **Clear sort** 恢复原顺序并重新启用手动移行。启用 VoiceOver 时，列标题会读出当前顺序并提示下一步。 **Location** 列支持单元格编辑。点单元格或在无障碍操作中选择 **Edit cell**，用 UIKit 键盘修改草稿。可用 **Save edit** 提交，也可用 **Cancel edit** 或系统无障碍取消动作放弃。 下方的 **Inspection hierarchy** 可以点三角展开，也可以用 VoiceOver 的 **Expand** 和 **Collapse** 操作。展开后可以编辑子节点的备注。

在 **Review options** 菜单里选择 **Manage tabs**，可以试用标签关闭。关闭 **Results** 时保留 **Draft**，关闭当前的 **Draft** 后回到固定的 **Pinned** 页。 同一菜单的 **Browse narrow tabs** 会打开窄标签栏。滑动标签或从关闭按钮发起无障碍滚动，可以浏览后续标签，同时保留当前选中项。

同一菜单下的 **Inspect cards** 演示 `SwipeView`。在 **Card summary** 或 **Card notes** 上发起横向无障碍滚动会切到下一张可用卡片。垂直滚动可浏览第一张卡片的内容。用 **Mirror cards** 可以切换横向翻页方向。

**Read service log** 演示嵌套滚动。焦点在班次文本时，横向无障碍滚动浏览班次，垂直滚动浏览整份记录。返回顶部会保留班次区域的横向位置。

**Disclosure controls** 演示可折叠标题和可勾选分组。展开 **Advanced options** 后可以执行 **Network details**，切换 **Optional settings** 可以启用或禁用 **Test settings**。

**Open tools drawer** 打开底部的 **Inspection tools**。也可以从 Home 指示条上方往上滑打开，从抽屉内侧的上边缘往下滑关闭。

```sh
xcrun simctl launch --terminate-running-process <设备标识> dev.xrack.ios-controls --native-actions
```

宿主内 XCTest 直接执行 UIKit 自定义动作，验证菜单回调、行移动、记录身份、旧动作失效，以及列排序的读数和下一步提示。 用这一版应用包运行 HID 测试并加上 `--test_arg=--native-actions-demo`，还可以检查系统级快照中的列标题、单元格与各行移动方向。

```sh
bazel test --config=ios-sim //examples/ios:simulator_native_actions_test \
  --@rules_apple//apple/build_settings:ios_simulator_device="iPhone 16 Pro" \
  --@rules_apple//apple/build_settings:ios_simulator_version=18.2
```

### 无障碍关闭回归

应用接收 UIKit 的 `accessibilityPerformEscape`，用于 VoiceOver 取消弹层。宿主内测试通过 UIKit 激活对话框和下拉框，再从无障碍容器调用取消动作，检查取消回调、对话框关闭和选项值。

```sh
bazel test --config=ios-sim //examples/ios:simulator_accessibility_escape_test \
  --@rules_apple//apple/build_settings:ios_simulator_device="iPhone 16 Pro" \
  --@rules_apple//apple/build_settings:ios_simulator_version=18.2
```

### UIKit 宿主支持

- iOS 模拟器目前可以运行 Rust AOT 应用。宿主复用 UIKit 上的 winit 窗口、触摸和生命周期，通过 Metal 绘制。应用可通过 `xrack::platform::run_with_viewport` 读取可用逻辑尺寸，并根据安全区、旋转和停靠键盘调整自己的布局。
- 系统安全区和应用设置的内边距分别计算。iOS 还会用 Core Text 绘制系统苹方的简体中文回退字形，让备注、任务列表和菜单沿用同一套排版与选区逻辑。示例无需打包系统字体。
- `TextInput`、`LineEdit`、`SearchField`、可编辑的 `ComboBox`、`TextEdit` 和 `TextArea` 使用 UIKit 文本系统。XRack 显示预编辑，接收提交内容与选区，并把 UTF-16 位置转换为运行时所用的 UTF-8 位置。停靠键盘会缩小布局内容区，浮动键盘保留全宽内容区。
- 外接键盘通过 UIKit 的按下、松开和取消事件接入共享输入逻辑。编辑器保留原生文字输入和 IME，导航、焦点与 Command 快捷键使用 XRack 的控件语义；切到后台时清除修饰键状态。
- `FileDialog` 支持导入单个和多个文件，`MessageDialog` 使用 UIKit 的系统提示，两者沿用同一窗口的弹窗队列。Rust 宿主可以通过 `DesktopWindow` 选择并读取目录、打开分享面板或向 Files 导出现有文件。真机签名和 VoiceOver 的端到端验收仍待补齐。解释器测试在宿主端运行。
- [Android 示例](../android/README.md) 复用同一份 `.xr` 和任务模型，通过 NativeActivity 和 Vulkan 在 arm64 模拟器上运行。系统输入和文件面板的接入仍在后续范围。
