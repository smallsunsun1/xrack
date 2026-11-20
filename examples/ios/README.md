# Pocket Tasks

XRack 的 iOS 巡检任务看板。巡检进度、待处理和已完成列表、任务录入与切换都写在 `touch.xr`。竖屏上下排列，横屏用同一份界面和模型切换成双栏。构建时，macOS 上的 `xrackc` 生成 Rust，Bazel 用 `rules_rust` 交叉编译静态库，再用 `rules_apple` 打包 iOS 应用。winit 的 UIKit 后端处理窗口和输入，FemtoVG 通过 wgpu/Metal 绘制。

iOS 无障碍树会逐项公开可见任务的标题、位置和当前状态。辅助功能可以激活待办项来完成任务，也可以在已完成列表激活它来重新打开。向某个可见任务发送滚动命令时，任务列表会滚动并公开新出现的任务。

长按任务行可以打开上下文菜单，点 **Mark complete** 完成任务，或在已完成列表中点 **Reopen task** 重新打开。松手时任务状态保持不变，选择菜单项后才提交。短按仍可直接切换状态，在任务行上滑动则会滚动列表。

向左滑动任务行会露出 **Complete**，已完成任务会露出 **Reopen**。滑动本身保留任务状态，点按钮才提交。也可在操作按钮上向右滑动收起操作区，或使用行尾的 **Show actions** 无障碍入口。

在 **XRack Controls** 中用 `--swipe-actions` 打开 **Document actions**。聚焦 **Review document** 后，可从 VoiceOver 操作列表展开任意一侧，随后浏览该行旁边的 **Pin** 或 **Archive**。第二行只提供右侧动作。

用 `--menu-paging` 打开 **Menu paging**。在 **Review commands** 按钮的 VoiceOver 操作中打开菜单，可从当前菜单项翻页找到 **Apply review**。返回首页打开 **Archive filters** 后，可在子菜单中独立翻页。回归目标为 `//examples/ios:simulator_menu_accessibility_test`。

同一示例的 **Paging interval** 下拉框中间一页是禁用项。开启 VoiceOver 后，可从当前项翻过这一页，到最后一页激活 **Annual**。翻页会把当前位置发给 UIKit，激活前保留原来的 **Daily**。同一回归目标也覆盖下拉框翻页、边界和重新打开后的选项身份。

连接外接键盘后，可以用 Tab / Shift+Tab 移动焦点，回车或空格激活按钮。在任务行或行内按钮上，左方向键展开操作区，右方向键或 Escape 收起，展开和收起本身不会完成任务。文本框支持方向键移动光标、Shift 扩展选区和 Command+A 全选；iOS 的 `Primary` 快捷键使用 Command。输入法正在组字时，文字和候选选择继续交给 UIKit。

完成或重新打开任务时，进度条会用 800 毫秒过渡到新值。iOS「设置 → 辅助功能 → 动态效果」开启 **减弱动态效果** 后，进度条直接显示新值，关闭后后续操作恢复动画。应用在回到前台时也会重新读取设置。

可在 iOS「设置 → 辅助功能 → 显示与文字大小 → 更大字体」调整字号，再返回应用查看。文字绘制和输入选区会使用系统缩放后的字号。模拟器也可执行 `xcrun simctl ui <设备标识> content_size extra-extra-extra-large`，用 `large` 恢复默认类别。

同一设置页的「粗体文本」会切换控件使用的字重。可在 **Appearance settings** 界面对比英文和简体中文示例，并确认返回应用时已输入的草稿依然保留。

点右上角的 **Notes** 进入巡检备注，可以用系统键盘输入多行内容，手动输入分类或从下拉列表选择。打开 **Read-only** 后，备注仍可查看。切回 **Tasks** 再返回，或者旋转设备，备注内容会保留。

在备注里长按一个词可选中文字，也可保持手指接触并拖动来扩展选区。松手后可以重新拖动选区两端的手柄，也可以从编辑菜单复制、剪切、粘贴或全选。只读时仍可调整选区并复制。

用 `--selection-scroll` 启动 **XRack Controls**，长按首行文本后拖到编辑区下缘并停住。选区会随着文字滚动而延伸，停到末行可看到 **End of notes selected**。拖动已有选区手柄时也可向边缘滚动。`//examples/ios:simulator_pointer_drag_test` 覆盖长按拖选和松手后的停止行为。

UIKit 直接移动光标或改变选区时，XRack 会同步更新选区绑定和绘制，只改选区时不会触发 `edited`。用 `--native-selection` 启动 XRack Controls，分别点 **Focus line** 和 **Focus notes**，可以在单行和多行示例里观察光标、选中文字和编辑次数。宿主将 UIKit 的 UTF-16 位置转换为示例显示的 UTF-8 字节偏移。 在选中文字后开始输入候选词，预编辑显示在原选区。候选词可以反复更新，确认后才替换原文并增加一次编辑计数。 组字时点 **Replace line** 或 **Replace notes**，应用会写入新值并结束本次组字，输入框保持焦点。

UIKit 向当前输入框请求撤销或重做时，使用与 XRack 编辑菜单相同的文本历史。单行和多行输入都有独立历史。焦点切换使旧的系统撤销请求失效，输入法组字期间会暂停这两个命令。 UIKit 的选词、全选、剪切、复制和粘贴也使用同一编辑状态。组字期间暂停这些命令，密码字段禁止剪切和复制。粘贴可用性只查询剪贴板的类型，执行粘贴时才读取内容。

从其他应用复制文字后，可以在备注或任务输入框选择 **Paste**。出现 iOS 授权弹窗时，点允许粘贴会继续替换原选区，拒绝则保留原文。

用 `--clipboard-authorization` 启动 XRack Controls，可以单独测试跨应用粘贴。在另一个应用复制文字，返回示例，点 **Select emoji** 和 **Paste into selection**。同意后会替换原选区，点 **Undo edit** 可恢复原文。拒绝后，原文和选区都保留。

开启 VoiceOver 后，在 **Paste destination** 进入编辑，再从「操作」转子选择全选、拷贝、剪切、粘贴、撤销或重做。可用操作随选区和编辑历史变化。`//examples/ios:simulator_native_actions_test` 通过 UIKit 验证这些操作，包括含 emoji 的选区和密码输入限制。

用 `--edit-menu-escape` 启动 **XRack Controls**，选中并长按草稿文字打开编辑菜单。按 Escape 或从屏幕起始边缘滑动会收起菜单，文本、选区和对话框仍保留。HID 回归使用 `controls_simulator.ipa` 和 `--test_arg=--edit-menu-escape`，UIKit 无障碍取消由 `//examples/ios:simulator_edit_menu_escape_test` 验证。

在同一个示例里点 **Select draft** 和 **Move draft**，随即长按选区打开菜单。草稿字段会在稍后向右移，原生菜单跟着选区移动。专用 UIKit 回归也会在菜单打开后触发布局变化，再核对关闭顺序。

两个原生测试分别在新模拟器内安装独立的 Clipboard Fixture，并从系统授权弹窗选择允许或拒绝。

```sh
bazel test --config=ios-sim --local_test_jobs=1 --test_timeout=360 \
  //examples/ios:simulator_clipboard_allow_test \
  //examples/ios:simulator_clipboard_deny_test
```

在备注页点 **Import**，可以从 Files 多选 UTF-8 的 `.txt` 和 `.md`。选择后，系统提示框会显示文件数量。点 **Yes** 合并所选文件并替换当前备注，点 **No** 或 **Cancel** 保留原备注。示例总计最多读取 64 KiB，读取失败也会保留原备注。示例开放了自己的 Documents 目录，方便通过 Files 的 **On My iPhone** 提供本地测试文件。

点 **Folder**，从 Files 进入一个目录后点 **Open**。示例按文件名顺序合并该目录第一层的 UTF-8 `.txt` 和 `.md`，总计最多 64 KiB，全部读取成功后才替换备注。取消、空目录、读取失败或读入期间修改了备注时，编辑器保留当前内容。

写好备注后，点 **Share** 可把当前文本交给 iOS 分享面板，点 **Save to Files** 可将 UTF-8 文本保存为 `inspection-notes.txt`。示例把临时源文件保留到 Files 面板关闭，随后自动清理。保存或取消均保留编辑器内容。

点 **Readings** 录入样本数、压力和温度偏移。三项分别使用整数键盘、小数键盘和可输入负号的键盘。压力字段用逗号显示小数，键盘的英文点号也可正常录入。输入草稿后直接点步进箭头或按外接键盘的上下键，会提交草稿步进后的值。点 **Done** 或切到另一项可直接提交输入，已提交值显示在表单下方。

**Readings** 下方的 **Code**、**Reading** 和 **Find** 演示单行表单。编号至少需要两位，读数须在 1 到 10 之间。数字键盘的 **Done**、系统搜索键和外接键盘 Return 共用提交规则。成功后收起相应的键盘，校验失败时可以继续编辑。结果显示在下方。任务页的 **New task** 也可以直接用软键盘回车添加。

在 **Readings** 点 **Contact details**，可试用邮箱、电话和网址键盘。邮箱键盘的 **Next** 提交当前值并移到电话。电话字段至少填三个字符后，工具栏的 **Next** 才会移到网址。网址键盘提供 `/`，用 **Done** 提交并收起键盘。带国家区号和分隔符的电话号码可以原样保留。

电话字段也可用 **Previous field** 提交后回到邮箱。只输入一个数字时，工具栏会保留电话焦点和草稿。邮箱工具栏上的 **Next field** 可以再回到电话，网址字段的前进按钮在表单末尾不可用。

录入联系人时可以旋转到横屏。弹层在可用高度变小时收紧内边距，表单在 `ScrollView` 内滚动，**Next** 会将目标字段带到键盘上方。收起键盘后，可继续检查已填写的字段或关闭表单。

**Readings** 可以滚动。编辑 **Reading** 时旋转到横屏，输入框会自动滚到 Done 工具栏上方，保留原输入和焦点。键盘占用空间时，下面的日期、时间和提交结果仍可滚动访问。

在 **Readings** 打开软键盘后，拖动表单会收起键盘和 **Done** 工具栏，草稿与字段焦点保留。再次点 **Reading** 可以接着输入。示例在表单的 `ScrollView` 上启用了 `dismiss-keyboard-on-drag`。

在 iPad 上也可点系统的「收起键盘」按钮。电话和多行备注都会保留当前草稿与编辑焦点，旋转屏幕时继续保持收起。再次点进字段可继续编辑。用 iPad 模拟器运行下文的 HID 测试时，添加 `--test_arg=--keyboard-dismissal` 可验证这两条路径。

在 iPad 上编辑 **Find**，把浮动键盘拖到输入框原位，表单会临时移开内容。拖走键盘后，输入框恢复原位置。可用 iPad mini (A17 Pro) 模拟器运行下文的 HID 测试，加上 `--test_arg=--floating-keyboard` 验证这个场景。

**Readings** 里也可以点当前日期或时间来安排巡检。日历允许选闰日，周末置灰。时间弹层提供小时、分钟、秒和 AM/PM 四个滚轮，确认范围是 08:00 至 18:00。系统开启「减弱动态效果」时，拖动滚轮松手即就近落位。滚轮仍在减速时，可以用 Tab 切到 **Save time**，按 Return 或空格确认当前可见行。点 **Cancel** 或按 Esc 保留原值。横屏时，日期弹层有足够宽度便会把操作按钮移到日历旁边。

## 构建并运行

从仓库根目录在 Apple Silicon Mac 上执行。请使用与当前 macOS 兼容的 Xcode。运行模拟器时，需安装 iOS 18.2 或更高版本的 Simulator 运行时。Bazel 通过默认的 `host_xcodes` 发现 Xcode。有多个版本时，可以用 `xcode-select` 选择默认版本。

```sh
bazel run --config=ios-sim //examples/ios:simulator \
  --@rules_apple//apple/build_settings:ios_simulator_device="iPhone 16 Pro" \
  --@rules_apple//apple/build_settings:ios_simulator_version=18.2
```

交互运行会优先复用匹配的模拟器。需要固定设备时，用 `xcrun simctl list devices available` 找到 UDID，再给 `bazel run` 加上 `--run_env=BAZEL_APPLE_DEVICE_UDID=<UDID>`。如果只想打开应用并让终端返回，加上 `--run_env=BAZEL_SIMCTL_LAUNCH_FLAGS=`。默认会在终端继续显示应用日志。

Xcode 27 的 Device Hub 如果停留在其他设备，应用启动后可用终端打印的模拟器 UDID 打开对应窗口。

```sh
open "devices://device/open?id=<UDID>"
```

可以按本机的 `xcrun simctl list devices available` 结果修改设备和版本。用 `bazel build --config=ios-sim //examples/ios:simulator` 可单独构建应用包。用 `bazel cquery --config=ios-sim --output=files //examples/ios:simulator` 查看 `simulator.ipa` 的实际路径。解包后的 `Payload/PocketTasks.app` 可由 `xcrun simctl install` 安装，再用 `xcrun simctl launch <设备标识> dev.xrack.ios-demo` 启动。

### iPhone 和 iPad 真机

在 Apple Silicon Mac 上，`--config=ios-device` 使用 `rules_rust` 的 `aarch64-apple-ios` 目标和 `rules_apple` 选择的 Xcode iPhoneOS SDK。应用目标使用同一份源码和最低系统版本。用 `--config=ios-device-check` 可在配置开发证书前构建三个真机可执行文件，验证编译与链接。

```sh
bazel build --config=ios-device-check //examples/ios:device_binaries
bazel cquery --config=ios-device-check --output=files //examples/ios:device_binaries
```

安装到设备时，先在本机配置 Apple Development 签名身份，并将覆盖目标 Bundle ID 和设备的 `.mobileprovision` 放入 `examples/ios/`。这个目录中的描述文件已被 Git 忽略。Pocket Tasks 的 Bundle ID 是 `dev.xrack.ios-demo`，XRack Controls 和 XRack Split 分别是 `dev.xrack.ios-controls` 和 `dev.xrack.ios-split`。用 `xcrun devicectl list devices` 查设备标识，并在设备上启用开发者模式。 [rules_apple 真机教程](https://github.com/bazelbuild/rules_apple/blob/main/doc/tutorials/ios-app.md) · [Apple 开发者模式说明](https://developer.apple.com/documentation/xcode/enabling-developer-mode-on-a-device)

```sh
bazel run --config=ios-device \
  --//examples/ios:provisioning_profile=//examples/ios:pocket-tasks.mobileprovision \
  --@rules_apple//apple/build_settings:ios_device="<设备标识>" \
  //examples/ios:simulator
```

## 测试

```sh
bazel test //examples/ios:touch_parity_test
bazel test --config=ios-sim //examples/ios:simulator_ui_test \
  --@rules_apple//apple/build_settings:ios_simulator_device="iPhone 16 Pro" \
  --@rules_apple//apple/build_settings:ios_simulator_version=18.2
```

第一项用同一份 `.xr` 测试 AOT 和 MLIR 解释器，覆盖任务操作、多行备注、可编辑分类、行内按钮键盘导航，以及日期和时间草稿。第二项通过 XCUITest 驱动软键盘、外接键盘、长按菜单和滑动操作区，也检查闰日和禁用周末的无障碍元素、横屏编辑读数，以及拖动时间滚轮后保存。

原生 XCTest 由 `rules_apple` 为每次执行创建临时模拟器，测试结束后清理。测试应用从本次构建安装，界面和输入状态也从新设备开始。临时设备使用英文软件键盘来验证多行备注输入。模拟器型号和运行时仍通过上面的 Bazel 设置选择。 只用 `--test_filter` 运行键盘用例时，可以加 `--test_env=XRACK_XCTEST_PHOTOS=0` 跳过相册样本导入。

实际 HID 按键另有一个测试目标。先构建上面的模拟器 IPA，再用宿主配置运行。测试需要安装支持 `ui key --command` 的 fb-idb 和 `idb_companion`，并显式指定测试设备；它不会操作桌面上当前聚焦的应用。将以下路径替换为实际产物和工具路径：

```sh
bazel test //examples/ios:simulator_hardware_keyboard_test \
  --test_arg=--ipa=/absolute/path/to/simulator.ipa \
  --test_arg=--udid=SIMULATOR-UDID \
  --test_arg=--idb-python=/absolute/path/to/python3 \
  --test_arg=--idb-companion=/absolute/path/to/idb_companion
```

若 fb-idb 安装在单独目录，可加 `--test_arg=--idb-site-packages=/absolute/path/to/site-packages`。测试安装并校验 IPA 中的可执行文件，覆盖光标移动、Command+A、Tab 后回车添加任务、Escape 收起操作区，以及表单校验、键盘收起和多行回车换行。读数页的搜索测试通过 UIKit 无障碍定位提交和清空按钮，并验证清空、撤销和继续编辑。测试还会在横屏完成一项任务，按 Home 再回到应用，核对进程、列表和任务数量。Xcode 27.2 搭配 iOS 18.2 时，XCTest 的硬件键注入和 UIKit 日志回调曾影响这两类测试，因此由独立驱动执行。测试结束后恢复示例初始状态，截图和无障碍树保存在测试输出中。

在 iPhone 16 Pro 模拟器加 `--test_arg=--landscape-contact-form` 可单独验证联系人表单。测试旋转到横屏，分别用硬件 Return 和实际触摸运行表单。触摸路径点击系统键盘的 **Next**、工具栏的 **Next** 和系统键盘的 **Done**，同时校验电话号码过短时保留焦点。两条路径都检查三个字段完整可见、草稿保留和提交后键盘收起。

### 外接键盘功能键

用 `--function-keys` 启动 **XRack Controls**。点 **Inspection commands** 取得焦点，按 F2 观察刷新和释放计数。点进 **Command draft** 后，F2 和 Command+F12 仍可执行命令。用 Shift+F10 或键盘上的 Application 键打开 **Inspection commands** 的上下文菜单，再用方向键和回车调用 **Open runbook**。

运行上述 HID 回归时，将 IPA 路径换成 `controls_simulator.ipa`，并加 `--test_arg=--function-keys`。

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

### 系统外观示例

用 `--button-shapes` 启动透明按钮示例。在 iOS「辅助功能 → 显示与文字大小」切换 **Button Shapes**，返回应用后可对照按钮轮廓。命中区域和禁用状态沿用原控件逻辑。HID 回归使用 `--test_arg=--button-shapes`，并恢复模拟器原设置。

用 `--selection-marks` 启动选择示例。在同一设置页切换 **Differentiate Without Color**，回到应用后查看当前标签的下划线、下拉框的选中勾标和键盘高亮轮廓。HID 回归使用 `controls_simulator.ipa` 和 `--test_arg=--selection-marks`，验证设置切换、选项命中区域和键盘选择，并恢复模拟器原设置。

用 `--appearance` 启动 **XRack Controls** 后，可以切换 **System**、**Light** 和 **Dark**。System 模式跟随 UIKit 外观。切换系统深色时，默认控件和已打开的对话框应用当前调色板，输入框保留草稿。

```sh
xcrun simctl launch --terminate-running-process <设备标识> dev.xrack.ios-controls --appearance
xcrun simctl ui <设备标识> appearance dark
xcrun simctl ui <设备标识> increase_contrast enabled
```

用最后一条命令可比较启用「增强对比度」后的输入边框和焦点指示。显式设置的色样可作为对照。把参数换成 `disabled` 可恢复普通对比度。

在「设置 → 辅助功能 → 显示与文字大小」开启「开启／关闭标签」，然后回到示例。**Inspection reminders** 可点击切换状态，**Required reminders** 是已开启的禁用开关。默认轨道会以竖线表示开启、圆环表示关闭。

用 `--native-actions` 启动示例并长按 **Review options**。菜单展开时执行 `xcrun simctl ui <设备标识> content_size accessibility-extra-extra-extra-large`，可以看到字体、行高和可见项数一起变化。长菜单可滚动，点击放大后的 **Record review** 仍会提交。把字号设回 `large` 可恢复常规尺寸。

在 iPhone 16 Pro 模拟器运行 HID 回归时，将 `--ipa` 换成 `controls_simulator.ipa` 并加 `--test_arg=--menu-dynamic-type`，可以自动验证打开的菜单随字号变化，以及横屏滚动到底后选择最后一项。测试结束后恢复原系统字号。

在 **Review options** 里选 **Inspect popup choices**，可体验大字号下的下拉框和搜索建议。HID 回归使用同一个 IPA 和 `--test_arg=--choice-dynamic-type`，覆盖弹层打开时调大字号、滑到末尾选项，以及软键盘打开时浏览、选中和提交搜索建议。

在 **Review options** 选择 **Manage tabs**，打开后把系统字号调到最大，可查看标签栏和关闭按钮随字号变化。HID 回归使用 `controls_simulator.ipa` 和 `--test_arg=--tab-dynamic-type`，覆盖保留当前页、键盘切换到溢出标签和触摸关闭。

用 `--native-actions` 启动后，把系统字号调到最大，可观察固定高度的 **Review options** 按钮。HID 回归使用同一个 IPA 和 `--test_arg=--button-dynamic-type`，采集按钮画面并长按执行 **Record review**。

同一示例的 **Inspection instruments** 表格可用 `--test_arg=--table-dynamic-type` 执行大字号回归。它会采集表头和固定高度单元格的画面，再验证排序、读取完整值和提交编辑。

用 `--compact-rows` 启动 **Inspection rows**，可查看固定高度的标准列表、分段标题和层级树。HID 回归使用同一个 IPA 和 `--test_arg=--row-dynamic-type`，覆盖最大字号下的选择、展开、编辑和折叠。

在 **Review options → Adjust instrument values** 中，**Sample transfer** 随 **Sensor gain** 依次显示部分完成、进行中和完成。UIKit 按系统区域格式读出进度百分比。进度未知时，元素保留名称并清除旧的百分比。

用 `--compact-toggles` 启动 **Inspection toggles**，可查看固定高度的复选框、开关和从右到左的阿拉伯语单选组。大字号时标签适配控件高度，超出宽度的部分用省略号表示。UIKit 仍提供完整名称。HID 回归使用 `controls_simulator.ipa` 和 `--test_arg=--toggle-dynamic-type`，验证触摸选中、外接键盘切换和禁用状态。

用 `--modal-titles` 启动标题示例。窄对话框会在空间允许时把标题排成两行。较矮的抽屉会限制标题的字号增长，给操作按钮留出空间，并在需要时用省略号收尾。UIKit 提供完整标题和独立的操作按钮。HID 回归使用 `controls_simulator.ipa` 和 `--test_arg=--modal-title-dynamic-type`。

用 `--group-titles` 查看大字号的展开面板和 RTL 可勾选分组。空间允许时标题最多显示两行，两行都属于同一个可点击标题区域。UIKit 保留完整标题，子控件仍随展开或勾选状态更新。HID 回归使用 `--test_arg=--group-title-dynamic-type`。

在 iPad mini 的 **Inspect popup choices** 里输入查询。将浮动键盘拖到建议列表上，弹层会移到键盘外，仍可触摸选中建议和提交。把键盘移开后，未关闭的弹层会恢复原位。HID 回归使用 `controls_simulator.ipa` 和 `--test_arg=--floating-keyboard-choices`。

在 **Pocket Tasks → Open readings** 打开日期或时间选择器，然后调大系统字号。HID 回归使用 Pocket Tasks 的 `simulator.ipa` 并加 `--test_arg=--picker-dynamic-type`，验证已打开的日历放大后仍可保存闰日，以及最大系统字号下横屏拖动分钟滚轮并确认。

未指定 `layout-direction` 的窗口会跟随 UIKit 的布局方向。方向变化后，水平布局、滑块和无障碍位置使用同一份新布局。用 `--system-direction` 启动 **XRack Controls**，可对照随系统镜像的操作行与显式设为从左到右的序号行。

### 下拉刷新

用 `--pull-to-refresh` 启动 **XRack Controls**，打开与桌面共用的 **Inspection feed** 示例。在列表顶部向下拉，然后松手。示例用定时器模拟异步加载，提示消失后会增加已完成刷新次数。也可用 **Refresh** 按钮，或在 VoiceOver 中聚焦一行并从操作列表选择 **Refresh**。原生回归目标 `simulator_refresh_accessibility_test` 验证加载期间移除操作，完成后可继续从其他行刷新。

启用 VoiceOver 后，开始和完成刷新时的状态文本会以普通优先级播报，初次打开示例时的计数保持安静。示例通过 `Label` 的 `accessible-live` 设置优先级。

```sh
xcrun simctl launch --terminate-running-process <设备标识> dev.xrack.ios-controls --pull-to-refresh
```

### 列表触摸重排

用 `--reorder-list` 启动 **Pocket Tasks** 打开 **Inspection order**。轻点一行可标记并选中，快速划动浏览列表。按住一行后拖到另一行，松手时用 `Model.move` 调整模型顺序。选中项和标记跟随原记录。行末的上下按钮也可以移动该行，并向 VoiceOver 提供完整的操作名称。

VoiceOver 分页滚动时，阅读焦点跟随仍可见的原行。原行滚出后，向后翻页从新视口的首行继续读，向前翻页从末行继续读。`//examples/ios:simulator_list_scroll_test` 通过 UIKit 滚动回调检查虚拟行更新和选中状态。

拖住 **Pump pressure** 到列表底部并停住，列表会滚出 **Cooling fan** 等后续记录。悬停边缘时不会提交模型变化。在末行松手，之前标记的 **Pump pressure** 会移动到末尾。示例通过 `drag-auto-scroll` 启用边缘滚动。

```sh
xcrun simctl launch --terminate-running-process <设备标识> dev.xrack.ios-demo --reorder-list
```

### 系统相册选图

用 `--choose-photos` 启动 **Pocket Tasks**，点击 **Choose photos**。一次最多选四张。左右滑动预览或使用页码控件，照片按选择顺序显示。点 **Save original** 可保存当前原图。多选时可以用 **Save all** 将整批原图复制到 Files。导入完成后才替换上一次的照片，取消时保留当前预览和页码。模拟器测试会向临时相册加入 PNG 和 HEIC 样本，预览由 ImageIO 生成。

保存时在 Files 中进入目标目录，点 **Open** 确认。重复保存时，同名副本会增加数字后缀。点 **Share originals** 可将当前一批原图交给系统分享面板。关闭面板后可以继续浏览当前页。

```sh
xcrun simctl launch --terminate-running-process <设备标识> dev.xrack.ios-demo --choose-photos
```

### 重新读取备注目录

在 **Notes** 点击 **Folder**，选择包含 `.txt` 或 `.md` 的目录。成功读取后，Pocket Tasks 会在应用私有存储中记住该目录。重启应用后，点击 **Reload last folder** 可读取目录里的最新内容。若书签过期或授权被撤销，可以再次点 **Folder** 选择。

### 系统面板排队

用 `--native-panels` 启动 Pocket Tasks，点 **Open panel queue**。`.xr` 的 `MessageDialog` 打开后，Rust 同时请求 Files 目录选择器、系统相册和分享面板。依次关闭面板，页面会显示每个请求的结果。

点 **Open timed panels** 可以观察 Rust 停止等待时的行为。Files 导出和分享面板先后打开，每个面板到期后自动关闭。最后的提示框确认队列还能处理新的 `.xr` 对话框。

点 **Open page notice** 或 **Open page Files** 进入巡检页面，再点 **Show inspection dialog**。页面移除后，系统面板会自动关闭，并显示窗口级提示框。

### 系统色板

用 `--native-colors` 启动 Pocket Tasks，点 **Edit marker color**。可用系统网格、光谱或滑条调整巡检标记颜色。编辑中会更新临时色块和 **Preview** 色值，关闭色板后提交最终颜色。取消勾选 **Edit opacity** 可以调整 RGB 并保留原透明度。

### iPad 鼠标与触控板

用 `--ipad-pointer` 启动 Pocket Tasks。将指针移入 **Inspect sensor** 可以看到悬停反馈。右击、Control-click 或手指长按按钮会打开菜单。选择 **Open sensor log** 后可看到 **Sensor log opens** 计数。普通点击只更新 **Inspections** 计数。将指针移到 **Inspection log** 上，用滚轮或双指滚动可浏览巡检记录。列表也支持手指划动。

按住 Command 在 **Inspection log** 上滚动，可以切换记录字号并保留滚动位置。松开 Command 后，滚动继续浏览记录。`//examples/ios:simulator_pointer_scroll_test` 将带修饰键和普通滚动样本送入 UIKit 桥接。`//examples/ios:simulator_pointer_test` 覆盖模拟器鼠标滚动、右击、Control-click 和触摸。

打开 XRack Controls 的 **Open gestures**，可以在 **Gesture canvas** 上用触控板捏合或旋转。两种动作可以组合。松手后保留图形变换，失去焦点时取消本次操作。该面板也可直接用两指触屏，单指仍可点 **Tap pad**。

用 `--pointer-styles` 启动 XRack Controls。把光标移到 **Inspection name** 可看到原生文字光标，移到两个分栏手柄可看到横向或纵向箭头，移到 **Align sensor** 区域可看到十字准星。移到普通按钮时会恢复系统光标。 按住鼠标主键拖动可以调整分栏或选取文字。 双击文本可选词，三击可选中所在段落，单行输入框会选中整行。点 **Click or tap here** 可以看到当前输入来自鼠标或触屏。

### 上下文菜单和表格行操作

用 `--native-actions` 打开 **Inspection instruments**。聚焦 **Instrument** 或 **Location** 表头时，可从 VoiceOver 操作列表向左或向右移动列。**Instrument** 表头的激活动作仍用于排序。

勾选 **Resize table headers** 后，**Instrument** 表头可用无障碍操作调整宽度，行标题可调整高度。排序后，调整过的行高跟随原记录。

**Inspection queue** 展示上下文菜单和可移动表格行。长按 **Review options** 或用 VoiceOver 的 **Show context menu** 打开菜单。启用 VoiceOver 时，聚焦行标题可以在「操作」转子中向上或向下移动该行。 点 **Instrument** 切换升序和降序，点 **Clear sort** 恢复原顺序并重新启用手动移行。启用 VoiceOver 时，列标题会读出当前顺序并提示下一步。 **Location** 列支持单元格编辑。点单元格或在无障碍操作中选择 **Edit cell**，用 UIKit 键盘修改草稿。可用 **Save edit** 提交，也可用 **Cancel edit** 或系统无障碍取消动作放弃。 下方的 **Inspection hierarchy** 可以点三角展开，也可以用 VoiceOver 的 **Expand** 和 **Collapse** 操作。展开后可以编辑子节点的备注。

在 **Review options** 菜单里选择 **Manage tabs**，可以试用标签关闭。关闭 **Results** 时保留 **Draft**，关闭当前的 **Draft** 后回到固定的 **Pinned** 页。同一菜单的 **Browse narrow tabs** 会打开窄标签栏。滑动标签或从关闭按钮发起无障碍滚动，可以浏览后续标签。聚焦 **Draft** 后在 VoiceOver 操作列表中选 **Move tab right**，可验证标签移到溢出区域时栏会自动滚动，当前页仍是 **Overview**。

同一菜单下的 **Inspect cards** 演示 `SwipeView`。在 **Card summary** 或 **Card notes** 上发起横向无障碍滚动会切到下一张可用卡片。垂直滚动可浏览第一张卡片的内容。用 **Mirror cards** 可以切换横向翻页方向。

**Read service log** 演示嵌套滚动。焦点在班次文本时，横向无障碍滚动浏览班次，垂直滚动浏览整份记录。先横向切到 **Night shift**，再纵向滚动到 **Parts replaced**，点一下系统状态栏即可回到 **Shift notes**。班次区域仍停在 **Night shift**。

**Report page controls** 演示 `PageIndicator` 和报告页同步。UIKit 会读取当前页和总页数，启用调节时可进入前一页或后一页。该示例从右到左绘制页点，辅助技术按逻辑页序调整。点 **Lock page changes** 后仍可读取页码。

**Inspection queue** 的主标题，以及 **Read service log** 中可见的 **Shift notes** 和 **Parts replaced**，使用 `accessible-heading-level` 声明层级。启用 VoiceOver 后，可在「标题」转子里按章节导航。滚动日志会更新当前可见的标题。

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

在 **XRack Controls** 的 **Review options** 中选择 **Open tools drawer**，可查看底部抽屉。关闭后从 Home 指示条上方拖动，可看到面板预览。打开时从拉手向下拖动，面板内的控件会一起移动。短拖动松手后恢复原位。HID 回归使用 `controls_simulator.ipa` 和 `--test_arg=--drawer-preview`，会在手指松开前采样画面和无障碍位置。

应用接收 UIKit 的 `accessibilityPerformEscape`，用于 VoiceOver 取消弹层。宿主内测试通过 UIKit 激活对话框和下拉框，再从无障碍容器调用取消动作，检查取消回调、对话框关闭和选项值。

```sh
bazel test --config=ios-sim //examples/ios:simulator_accessibility_escape_test \
  --@rules_apple//apple/build_settings:ios_simulator_device="iPhone 16 Pro" \
  --@rules_apple//apple/build_settings:ios_simulator_version=18.2
```

### 登录与验证码字段

用 `--native-actions` 启动 **XRack Controls**。长按 **Review options** 后选择 **Sign-in fields**，可以用系统键盘体验账号邮箱、当前密码、新密码和验证码。账号字段保留邮箱键盘，验证码字段使用数字键盘。Next 依次切到下一项，普通备注字段恢复默认输入用途。

`simulator_native_actions_test` 检查活动 UIKit 输入框的内容类型、密码遮罩和键盘，并验证输入写回字段与焦点切换。系统密码和验证码建议由设备与应用配置决定。

### 文件拖放

在 iPad 安装 **XRack Controls**，用 `--file-drop` 打开与 macOS 共用的文件拖放示例。iPadOS 18 上可在横屏时点 XRack 窗口顶部的「⋯」，选择「分屏浏览」，再选择「文件」。将 PNG 或 JPEG 拖入图片区，或将 TXT 或 Markdown 拖入文档区。两个区域会随分屏宽度缩放。匹配时区域会高亮，松手后可看到接收批数和按原顺序列出的文件路径。

```sh
xcrun simctl launch --terminate-running-process <设备标识> dev.xrack.ios-controls --file-drop
bazel test --config=ios-sim //examples/ios:simulator_file_drop_test \
  --@rules_apple//apple/build_settings:ios_simulator_device="iPad mini (A17 Pro)" \
  --@rules_apple//apple/build_settings:ios_simulator_version=18.2
```

原生测试通过 UIKit 的接收委托提交文件 URL 和异步文件表示，检查类型匹配、顺序、内容、失败提示和成功重试。文件复制到应用沙箱的临时目录，路径可用到窗口关闭为止。

在 iPadOS 18.2 模拟器上，从「文件」拖入的 Markdown 有时只提供可显示的元数据。若系统文件提供方返回读取错误，示例通过 `files-failed(reason)` 显示原因，并保留上一次已接收的批次。成功接收下一批后清除提示。也可在 Pocket Tasks 的 **Notes → Import** 通过系统文件选择器导入 Markdown。

### 文本拖放

在 iPad 分屏打开 **XRack Controls** 和另一个支持拖出文字的 App。用 `--text-drop` 启动示例，把选中的文字拖到 **Drop title** 或 **Drop notes**。文字从落点对应的光标位置插入，整次拖放可以撤销。多项文字按拖拽顺序用换行连接。只读和禁用字段会拒绝拖放，等待期间继续编辑也会使旧拖放失效。

```sh
xcrun simctl launch --terminate-running-process <设备标识> dev.xrack.ios-controls --text-drop
bazel test --config=ios-sim //examples/ios:simulator_text_drop_test \
  --@rules_apple//apple/build_settings:ios_simulator_device="iPad mini (A17 Pro)" \
  --@rules_apple//apple/build_settings:ios_simulator_version=18.2
```

用 `--text-drag` 启动 **XRack Controls**，点 **Select sample**，长按高亮的内容并拖到分屏中的另一个 App。系统会复制选中的纯文本。长按后原地松开会打开编辑菜单。单行、多行和只读输入框都可以作为来源，密码字段和输入法组字期间会拒绝拖出。`//examples/ios:simulator_text_drag_test` 会实际读取 UIKit 数据提供器返回的内容。

### 长按提示

用 `--touch-tooltip` 启动 **XRack Controls**。轻点 **Inspect sensor** 会增加巡检次数。长按会显示读数提示，松手后巡检次数保持原值。提示出现前向上划动会滚动记录。再次触摸或提示超时后，提示关闭。

用 `--native-actions` 启动后，**Review options** 的纯文本提示也会提供给 VoiceOver。选中 **Record review** 后，再聚焦 **Review options** 可读取更新后的说明。**Clear sort** 使用控件自己声明的 `accessible-description`。

### 声明式目录导入

用 `--folder-dialog` 启动 **Pocket Tasks**。点 **Import one folder**，在 Files 中打开 **Pump notes** 并确认，示例会读取导入目录中的 `reports/summary.txt`。点 **Import folders** 可以选择一个目录后继续添加。在确认面板点 **Import selected folders** 后，示例会读取整批目录的内容。**Cancel** 会取消本批导入。

`FileDialog` 的 `select-folder` 和 `select-folders` 都把所选目录复制到应用的临时目录，再通过 `accepted` 返回本地路径。需要就地读写或保存系统授权时，可使用 Rust 的 `DesktopWindow::select_directory()` 和 `IosDirectory`。

### 动态加载与页面返回

在 **XRack Controls** 使用 `--native-actions`，从 **Review options** 选择 **Test accessible navigation**。激活 **Record pressure** 后，点 **Load flow tool** 切换为流量工具。UIKit 会公开新工具，卸载后原有操作失效。

用 **Open inspection detail** 进入详情页。VoiceOver 的系统返回动作会尝试返回摘要页。**Lock return** 可阻止离开。使用 **Open review overlay** 打开的弹层会优先处理返回，锁定时可用 **Dismiss review overlay** 关闭。`//examples/ios:simulator_native_actions_test` 调用 UIKit 的无障碍接口检查这些行为。

在详情页也可从屏幕起始边缘向内滑动并松手。弹层和页面拦截规则与 VoiceOver 返回相同。iPhone 16 Pro 模拟器的 HID 回归使用 `controls_simulator.ipa` 和 `--test_arg=--edge-back`，覆盖短滑、锁定、先关闭弹层以及回到根页。

用 `--native-actions` 启动后，从 **Review options** 选择 **Adjust instrument values**，可用 VoiceOver 调节传感器增益、记录范围、小数偏移和分钟。**Fixed sample count** 会读出固定值。原生 XCTest 通过 UIKit 的增减接口检查边界、双手柄独立性、小数读值和分钟循环。

### 键盘辅助输入

用 `--keyboard-layout` 启动 **XRack Controls**，可直接打开键盘设置表单。弹出软键盘后旋转到横屏，用 **Next field** 切换到备注。正文会滚动显示当前字段，**Save drafts** 保持可用。空间较矮且宽度允许时，操作区移到正文侧边。`//examples/ios:simulator_pointer_drag_test` 包含这项横屏回归。

在 iPhone SE 模拟器的横屏模式下，键盘仅显示输入工具栏时，可以拖动同一表单。**Next field** 保持可用，可以继续用外接键盘输入并保存。

用 `--native-actions` 启动 **XRack Controls**，从 **Review options** 打开 **Configure keyboard assistance**。标题输入框默认按单词首字母大写，可用 **Use identifier keyboard** 切换到全大写并关闭自动更正和拼写检查。正在组字时，键盘会在提交后切换。

在 **Review note** 中可以用 Return 换行。输入至少两个字符后，可用键盘工具栏上的 **Previous field** 回到标题，或用 **Next field** 前往访问码。访问码字段隐藏导航工具栏，保留 **Done** 和密码键盘策略。

启用 VoiceOver 后，聚焦 **Review note** 并双击即可编辑。收起键盘后再次双击，原草稿保留。原生回归调用与 VoiceOver 双击相同的 UIKit 激活入口。

从 **Review options** 打开 **Write long note**，可以体验长备注。停靠键盘打开后，输入或移动光标会按需滚动外层表单，让当前行留在键盘上方。手动滚动表单时可以查看其他段落，下一次编辑会带回当前行。

在 iPad mini 上可把键盘切到浮动模式，拖到长备注的光标上。表单会为当前行腾出空间，键盘移开后恢复原位。用英文 QWERTY 键盘运行 HID 回归时，将 `--ipa` 换成 `controls_simulator.ipa`，加上 `--test_arg=--floating-keyboard-long-note`。

### UIKit 宿主支持

- iOS 模拟器目前可以运行 Rust AOT 应用。宿主复用 UIKit 上的 winit 窗口、触摸和生命周期，通过 Metal 绘制。应用可通过 `xrack::platform::run_with_viewport` 读取可用逻辑尺寸，并根据安全区、旋转和停靠键盘调整自己的布局。
- 系统安全区和应用设置的内边距分别计算。iOS 还会用 Core Text 绘制系统苹方的简体中文回退字形，让备注、任务列表和菜单沿用同一套排版与选区逻辑。示例无需打包系统字体。
- 标签和编辑器会用 Core Text 绘制系统 Apple Color Emoji。用 `--emoji` 启动 **XRack Controls**，可试用输入框和不同的彩色 emoji。 `simulator_emoji_test` 从实际 Metal 画面检查单个 emoji、肤色组合和国旗。
- `TextInput`、`LineEdit`、`SearchField`、可编辑的 `ComboBox`、`TextEdit` 和 `TextArea` 使用 UIKit 文本系统。XRack 显示预编辑，接收提交内容与选区，并把 UTF-16 位置转换为运行时所用的 UTF-8 位置。停靠键盘会缩小布局内容区，浮动键盘保留全宽内容区。
- 在 **Notes** 中长按文字会打开 UIKit 编辑菜单。可以剪切所选文字并撤销。勾选 **Read-only** 后仍可选择和复制。编辑菜单与软键盘可以同时显示。
- 外接键盘通过 UIKit 的按下、松开和取消事件接入共享输入逻辑。编辑器保留原生文字输入和 IME，导航、焦点与 Command 快捷键使用 XRack 的控件语义；切到后台时清除修饰键状态。
- `FileDialog` 支持导入单个和多个文件或目录，`MessageDialog` 使用 UIKit 的系统提示，两者沿用同一窗口的弹窗队列。Rust 宿主可以通过 `DesktopWindow` 选择并读取目录、打开分享面板或向 Files 导出现有文件。真机签名和 VoiceOver 的端到端验收仍待补齐。解释器测试在宿主端运行。
- [Android 示例](../android/README.md) 复用同一份 `.xr` 和任务模型，通过 NativeActivity 和 Vulkan 在 arm64 模拟器上运行。系统输入和文件面板的接入仍在后续范围。
