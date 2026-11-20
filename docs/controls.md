# 控件与布局参考

[返回项目首页](../README.md)

列表、树和表格见 [模型与虚拟视图](models.md)。控件的完整可运行用例见 [examples](../examples/)。

## 主题、布局和动画

窗口可设置 `color-scheme: system;` 跟随宿主系统主题，或用 `light`、`dark` 固定应用内容的配色。`accent-color` 可绑定到应用状态，用于按钮、滑条、焦点和选中态。主题同时应用到窗口内的菜单和弹层。控件显式指定的背景和前景会覆盖调色板默认值。省略主题属性时沿用原有配色。运行 `bazel run //examples/controls:controls -- --theme` 可实时切换三种模式。

Android 的 Activity 把系统夜间模式变化转发给 Rust，由 Rust 更新窗口调色板。示例在 Manifest 的 `android:configChanges` 中声明 `uiMode`，让原窗口接收配色变化并保留页面状态。 [Android 系统配色回归](../examples/android/README.md#颜色与字体回归)

iOS 的 `system` 模式读取 UIKit 当前窗口的外观，应用运行中切换浅色或深色时会刷新调色板，回到前台时也会重新读取。[iOS 系统外观示例](../examples/ios/README.md#系统外观示例) 演示了这一过程。

iOS 开启「增强对比度」后，默认文字、输入边框、焦点指示、选中态和滚动滑块使用对比更强的配色。`system`、`light` 和 `dark` 都会响应这个设置，控件显式指定的颜色仍由其属性决定。

`HorizontalLayout` 和 `VerticalLayout` 支持四边 `padding-*`、`spacing`、`min-*`、`preferred-*`、`max-*`、双轴伸缩与对齐。组件实例可以在使用处声明这些尺寸约束和子项属性，实例上的绑定通过 `xr.layout_bind` 进入场景树。主轴对齐支持 `stretch`、`start`、`center`、`end`、`space-between`、`space-around` 和 `space-evenly`。交叉轴可用 `cross-axis-self-alignment` 覆盖单个子项。`layout-order` 改变视觉位置，键盘焦点按声明顺序移动。

`GridLayout` 支持 `Row`、`row`、`col`、`rowspan`、`colspan` 和分轴间距。尺寸绑定可使用相对父容器内容区域的百分比，例如 `width: 50%;`。`animate width { duration: 220ms; }` 可对相同类型的视觉属性值做线性过渡。

在 iOS 开启「减弱动态效果」后，`animate`、状态过渡和 `StackView`、`SwipeView` 的切页动画会直接显示目标状态。拖动页面时仍会跟手，松手后立即显示当前页或目标页。应用会在系统设置变更和回到前台时重新读取偏好。

iOS 会按系统「文字大小」缩放默认字号和控件显式指定的 `font-size`，文字测量、编辑选区和实际绘制使用同一字号。更改系统字号或返回应用时，宿主重新计算布局，并清除旧字号对应的列表行缓存。显式指定的控件宽高仍由应用布局决定，长文字可用自动高度、换行或滚动容器容纳。

iOS 开启「粗体文本」后，控件会选择可用的较重字重，简体中文可回退到苹方中粗体。布局、文字绘制与光标定位使用同一份字形度量。切换设置后，字号继续跟随「文字大小」。

同一过渡可以用 `animate x, width` 应用于多个属性。`duration` 和 `delay` 用毫秒整数声明，默认均为 `0ms`。`easing` 支持 `linear`、`ease`、`ease-in`、`ease-out`、`ease-in-out` 和 `cubic-bezier(x1, y1, x2, y2)`。自定义曲线的两个 x 坐标均须在 0 到 1 之间。`enabled` 可绑定应用状态，关闭后属性直接显示目标值。相同类型且色标数量一致的渐变也可参与过渡。

每个属性保留自己的开始和结束时间。目标改变时从当前显示值起步，其他属性继续原有过渡。窗口在延迟期间可等待下一次唤醒。运行 `bazel run //examples/layout:layout -- --animations`，可对比线性与缓动，在过渡中反转目标或只更新旁边的计数。

`states [ alarm when root.level >= 2: { indicator.width: 280px; } ]` 可在一个状态里覆盖多个视觉属性。每个状态组按声明顺序选择第一个满足 `when` 的状态。没有匹配的覆盖时读取原绑定，即使原绑定依赖的值在状态激活期间变化也会恢复到最新结果。状态覆盖可作用于声明元素及同一组件、同一重复上下文内的后代。

状态可以改变组件实例的 `in`、`in-out` 输入及调用方设置的布局属性，例如 `badge.caption: "告警"; badge.width: 280px;`。输入覆盖参与组件属性的惰性求值，内部控件和外部 getter 会读到相同的值。状态退出时恢复调用方的最新绑定，或使用子组件的默认值。显式写入会接管这个输入，重置后恢复状态绑定。状态可以声明在实例体内，重复实例会按当前行独立选择状态。编译器检查循环依赖和与双向链接的冲突。运行 `bazel run //examples/layout:layout -- --component-states` 查看状态联动和布局转场。

状态内部的 `in`、`out` 和 `in-out` 可以声明 `animate indicator.width { duration: 300ms; }`。转场属性需要一个显式的原绑定。`animate *` 为该状态修改的可插值属性提供默认时序，显式声明优先。同一属性同时命中进入和离开时序时采用进入时序。运行 `bazel run //examples/layout:layout -- --states`，在正常、预警和告警之间切换，观察中途反转与恢复。

[Android 状态转场示例](../examples/android/README.md#状态转场回归) 比较组件移动过程中的实际绘制边界和触摸命中，验证退出状态时恢复最新的基础宽度。

`GroupBox` 为控件提供标题和边框，直接子项沿用垂直布局的间距、对齐与尺寸约束。`content-padding` 设置默认内边距，四边 `padding-*` 可单独覆盖。设置 `checkable: true;` 后，标题可用鼠标、键盘或辅助技术勾选，`checked` 可双向绑定，用户切换时触发 `toggled(bool)`。

取消勾选会保留内容和布局，禁用组内输入、快捷键和弹层。普通输入焦点会回到仍可用的分组标题。运行 `bazel run //examples/controls:controls -- --groups` 体验。

`Expander` 默认折叠。设定 `title` 和可双向绑定的 `expanded`，就可以通过点击标题、Enter、Space 或无障碍展开和折叠。用户切换会触发 `expanded-changed(bool)`。折叠释放内容占用的布局空间，保留控件状态，并把内容里的焦点移回标题。展开后的子项沿用垂直布局的内边距、间距与尺寸约束。 iOS 的展开标题会读出当前状态，并提供 **Expand** 或 **Collapse** 无障碍动作。展开后的子控件可以分别聚焦。可勾选的 `GroupBox` 会同时向 UIKit 报告标题的勾选值和子项的禁用状态。运行 `bazel run //examples/controls:controls -- --expander`，输入草稿后按 F8 折叠，再展开可继续编辑。

[Android 连接面板](../examples/android/README.md#独立控件回归) 演示编辑时折叠、标题焦点恢复和草稿保留，也可比较禁用分组时保留的布局。

`FlowLayout` 按子项的首选尺寸自动换行，设置 `orientation: vertical;` 可改为纵向排列并换列。每一行或列独立应用 `alignment` 和 `cross-axis-alignment`，子项仍可使用尺寸约束、`layout-order` 和 `cross-axis-self-alignment`。用 `horizontal-spacing` 和 `vertical-spacing` 分别控制两个方向的间距，默认沿用 `spacing`。隐藏和零尺寸子项不参与排布。运行 `bazel run //examples/layout:layout -- --flow` 后调整窗口宽度，可观察按钮换行和下方状态文本的位置变化。

`FlexboxLayout` 支持 `flex-direction` 的 `row`、`row-reverse`、`column` 和 `column-reverse`，以及 `flex-wrap` 的 `wrap`、`no-wrap` 和 `wrap-reverse`。`alignment: stretch;` 按主轴伸缩权重分配每行的剩余空间，并遵守最大尺寸。空间不足时按首选尺寸比例收缩，受最小尺寸限制。`cross-axis-line-alignment` 分配行与行之间的空间，`cross-axis-alignment` 和子项的 `cross-axis-self-alignment` 对齐行内元素。`spacing-horizontal` 和 `spacing-vertical` 可分别设定两个方向的间距。Tab 和无障碍按布局方向及 `layout-order` 遍历。运行 `bazel run //examples/layout:layout -- --flex` 体验。

窗口或容器可用 `layout-direction: right-to-left;` 设置布局方向，子树继承最近的声明。局部组件可用 `left-to-right` 覆盖。线性布局、流式布局、Flexbox、GridLayout 和 GridView 会按方向放置子项，`start` 和 `end` 随之调整。显式坐标和四边 padding 使用屏幕的物理方向。虚拟网格的左右方向键按屏幕位置导航。运行 `bazel run //examples/layout:layout -- --rtl` 可以切换方向并比较嵌套覆盖。

[Android 方向与换行回归](../examples/android/README.md#方向与换行布局回归) 验证网格镜像、Flow 和 Flex 换行后的触摸命中，以及 Flex 的键盘焦点顺序。

文字和编辑器可用 `horizontal-alignment: start;` 或 `end` 对齐到逻辑起止边。最近的 `layout-direction` 声明决定这两条边，未声明时使用文本自身的方向。`left` 和 `right` 表示物理边。编辑器用实际对齐后的文字位置计算点击、选区、光标、输入法锚点和无障碍文本范围。运行 `bazel run //examples/form:form -- --rtl-text` 体验。

`Text` 和 `Label` 可以用 `accessible-heading-level: 2;` 声明章节标题。整数 `1` 到 `6` 表示标题层级，默认 `0`，范围外的值按普通文字处理。标题名称优先使用非空的 `accessible-name`，其次使用 `text`。无障碍树会保留层级，iOS 会将标题交给系统的标题导航。视觉样式和键盘焦点顺序由原有属性决定。

`Text` 和 `Label` 的 `accessible-live` 接受 `AccessibleLive`。默认 `off`。设置 `polite` 可在读屏有空时播报刷新结果等状态文字，`assertive` 用于需要打断当前播报的紧急状态。开启后，非空文字的变化会交给系统无障碍适配器，控件保留原有焦点顺序。与标题层级同时启用时，播报内容使用标题的无障碍名称。

```xr
Label {
    text: root.upload-status;
    accessible-live: polite;
}
```

`Label` 默认不自动换行，适合标题和状态文字。`Text` 保留现有的默认换行。两者都支持 `wrap`、`horizontal-alignment`、`vertical-alignment`、`overflow` 和 `max-lines`。`start` 和 `end` 按段落方向解释，`left` 和 `right` 指物理边缘。`overflow: elide;` 按字素簇缩短超宽文字，隐藏后续行时在最后一个完整可见行加省略号。辅助技术仍可读取完整文本。运行 `bazel run //examples/layout:layout -- --labels` 体验。

## 替换控件部件

在 `Button` 下声明一个 `ButtonContent`，可用任意视觉子树替代默认文字与图标。在 `Slider` 下声明一个 `SliderHandle`，可自定义手柄的颜色、边框和尺寸。按键、焦点、指针和无障碍操作由父控件处理，部件内部不会形成额外的焦点目标。自定义按钮仍可通过 `text` 或 `accessible-name` 提供辅助技术使用的名称。

`CheckBox`、`RadioButton` 和 `Switch` 可以各声明一个 `ToggleIndicator`。部件由父控件摆放，可用 `width` 和 `height` 设置尺寸，标签会自动为它留出空间。内部可以放同一个自定义视觉组件，并将样式绑定到选中值或三态值。点击和键盘操作仍由父控件接收，辅助技术读取原控件的角色和状态。运行 `bazel run //examples/slots -- --toggle-indicators` 体验三个控件共用视觉组件的示例。

`Switch` 和 `SwitchDelegate` 可以各声明一个 `SwitchThumb`。滑块宽高可以写成长度或相对于轨道的百分比。它由控件摆放，实时跟随拖动预览，并自动处理 RTL。它可以搭配默认轨道或同级的 `ToggleIndicator`。虚拟列表中的部件可读取当前行的值来设置样式。运行 `bazel run //examples/controls -- --rtl-feedback` 试拖「同步」开关。

`SpinBox` 和 `DoubleSpinBox` 可分别声明 `SpinUpIndicator`、`SpinDownIndicator`。两个部件独立可选，省略的一侧使用默认箭头。控件为上下按钮分配同列的等高区域，子项声明的尺寸参与控件首选尺寸，空间不足时最多占一半宽度。文字编辑、方向键、鼠标和辅助技术仍由父控件处理。步进按钮列放在布局的末尾侧，RTL 时移到左侧，文字编辑区随之换边。运行 `bazel run //examples/slots -- --spin-indicators` 切换方向并体验整数和小数步进。

`RangeSlider` 可独立声明 `RangeLowerHandle` 和 `RangeUpperHandle`，省略的一侧使用默认手柄。控件根据两个部件的尺寸确定共同的轨道范围。绘制、命中和无障碍边界使用同一位置，`live: false;` 时部件也跟随拖动预览。两个端点保留独立的键盘焦点和无障碍数值。运行 `bazel run //examples/slots -- --range-handles`，体验横向下限和纵向上限定制。

iOS 增大系统字号时，按钮会按文字测量所需空间。显式限制高度时，内置标签可收紧默认上下内边距，再按可用空间调整字号。能容纳的整行会完整绘制，放不下的内容用省略号提示。无障碍树保留完整按钮名。替换的 `ButtonContent` 使用自己的布局。

`TabButton` 也可使用 `ButtonContent`，包括通过 `for` 创建的标签。标签会用替换内容测量它在横排或竖排中的长度，显式的 `width` 或 `height` 优先。保留 `text` 或 `accessible-name` 给辅助技术使用。选中态、键盘导航和滚动仍由 `TabBar` 处理。运行 `bazel run //examples/slots -- --tab-content` 查看同一个视觉组件在按钮和标签间的复用。

`bazel run //examples/slots:slots` 展示有内容插槽的设置面板、带计数的按钮内容和自定义音量手柄。用 Tab 切换焦点，Space 激活按钮，方向键调整音量。

## 文字和输入

`StyledText` 接收 `styled-text` 值并复用标签的布局属性。``@markdown("**粗体**、*斜体*和 `xrackc`")`` 在编译时生成文字和字素对齐的样式区间。支持段落、标题、粗体、斜体、删除线、代码和链接。图片和 HTML 会产生诊断。Rust 可用 `StyledText::from_markdown` 解析动态内容，也可用 `StyledText::from_parts` 提供有样式区间和链接的文本。
Markdown 链接用 `[文案](目标)` 声明，默认显示下划线并可配置 `link-color`。鼠标和 Tab / Enter 均可触发 `link-activated(target)`，应用收到字符串后决定如何处理。布局截断和祖先裁剪会同步限制链接的命中范围和焦点。运行 `bazel run //examples/layout:layout -- --styled-text` 体验换行和截断。

设置 `selectable: true;` 后，`StyledText` 可以拖选、双击选词、全选和复制纯文本。选区按已排版的字形定位，样式和链接保持有效。短按链接触发 `link-activated`，从链接上开始拖动则选择文字。可通过 `selected-text` 读取选中内容，通过 `select-all()`、`select-word()`、`set-selection-offsets(anchor, focus)`、`clear-selection()` 和 `copy()` 操作已聚焦的选区。全选包含原文中被截断的部分。运行 `bazel run //examples/layout:layout -- --selectable-styled-text` 体验。

Android 库存示例的 **Open bulletin** 演示轻触链接、长按选词、从编辑菜单复制纯文本，以及截断后的链接命中。设备回归目标为 `//examples/android:bulletin_ui_test`。[Android 示例](../examples/android/README.md#富文本触摸回归)

窗口可绑定 `default-font-family`、`default-font-size`、`default-font-weight` 和 `default-font-italic`。文字、按钮和输入控件可分别用 `font-family`、`font-size`、`font-weight` 和 `font-italic` 覆盖窗口设置。字体可用系统字体族名或 `monospace`。缺少所选字形时继续用系统回退字体。字重使用 100 到 900，400 为常规字重，0 沿用默认值。可变字体使用支持的字重和斜体轴。布局、GPU 绘制、编辑器光标和无障碍文字范围共用字体选择。运行 `bazel run //examples/layout:layout -- --fonts`，切换等宽字体和字号，观察单行输入和多行换行。

在 `.xr` 文件顶部写 `import "fonts/example.ttf";` 可内嵌本地 TTF、OTF 或 TTC 字体。路径相对于声明所在的 `.xr` 文件，Bazel 目标通过 `xr_ui_library` 的 `assets` 列出字体文件。通过字体内部的字体族名设置 `font-family` 或 `default-font-family`。内嵌字体优先于系统同名字体，TTC 可按字重和斜体选择文件中的字体变体。

编译器校验字体并把字节嵌入生成的 Rust。同一文件在组件导入链中出现多次时只嵌入一次。窗口使用各自的字体上下文，Rust 的离屏渲染也可以选择 `TextSystem::bundled_only()`。运行 `bazel run //examples/layout:layout -- --bundled-fonts` 可查看内嵌的两款字重和一个使用内嵌字体的输入框。

`TextInput` 和 `LineEdit` 提供单行键盘编辑、选区、IME 预编辑和提交。`text <=> root.query;` 与 `checked <=> root.flag;` 分别把文本输入和复选框与可写属性相连。条件子元素用 `if condition : Element { ... }` 表达，隐藏时保留组件的内部状态。重复元素用 `for item[index] in root.items : Text { ... }` 表达，接受整数计数、`[string]` 和命名记录列表，以匿名、无状态的显示模板生成纵向列表。

`TextInput`、`LineEdit`、`TextEdit`、`TextArea`、`SearchField` 和可编辑的 `ComboBox` 默认使用 6 个逻辑像素的内边距。`padding` 设置四边的统一值，`padding-left`、`padding-right`、`padding-top`、`padding-bottom` 可以分别覆盖，方向以屏幕物理边为准。布局会给内边距预留空间。多行换行、选区、光标与输入法定位都在内缩后的区域计算。下拉箭头和搜索按钮保留各自的点击区域。

`placeholder-text-color` 是可绑定的 `color`，用于编辑区为空时的占位文字。正常输入使用 `color`，禁用时使用主题颜色。运行 `bazel run //examples/form:form -- --editor-insets`，可以切换四边内边距，在单行和多行中试用鼠标与键盘输入。

在单行输入中设置 `input-type: password;` 后，每个字素簇默认显示为圆点 `•`，IME 预编辑也会遮罩。`TextInput`、`LineEdit` 和可编辑的 `ComboBox` 可用 `password-character: "*";` 换成其他标记。使用字符串的第一个字素簇。空值、空白、控制字符、超过 64 字节或连续显示时会合并的标记使用默认圆点。应用通过 `text` 绑定读写真值。`TextInput` 和 `LineEdit` 可用 `password-visible: true;` 显式显示原文。剪贴板复制和剪切仍被禁用，无障碍树仍只发布遮罩后的值和选区。运行 `bazel run //examples/form:form -- --password` 体验显示和切换遮罩。

`password-mask-delay` 以毫秒为单位。设为 `750` 可短暂显示刚键入的单个字素。默认 `0`，所有字符立即遮罩。计时从这次键入开始。粘贴、程序写入、IME 预编辑或提交都保持遮罩。移动光标、失焦、窗口失活或关闭延迟后会恢复完整遮罩。剪贴板和无障碍沿用密码字段的规则，无障碍仍只收到遮罩字符。

Android 示例的 **Open holds → Open privacy** 可体验自定义遮罩和显式显示。同一场景的设备回归使用键盘快捷键验证两种显示状态下的复制与剪切保护。 [Android 控件回归](../examples/android/README.md#独立控件回归)

`TextInput` 和 `LineEdit` 的 `input-type: number;` 接受 ASCII 数字，`decimal` 允许前置正负号和一个小数分隔符。`decimal-separator` 默认为 `"."`，也可设为 `","`。空文本、单独的符号或分隔符可作为编辑中间态，按 Enter 提交需至少有一位数字。`input-type: search;` 将单行输入向辅助技术标记为搜索框。

`input-type: email;`、`url` 和 `tel` 可用于 `TextInput`、`LineEdit` 和可编辑的 `ComboBox`。在 iOS 和 Android 上，它们分别选择邮箱、网址和电话键盘。iOS 也把用途提供给 UIKit 的文本内容提示。三种类型沿用普通文本的编辑和校验规则，可用 `input-pattern` 或 `minimum-length` 声明表单约束。示例见 [iOS 巡检表单](../examples/ios/README.md)。

`TextInput`、`LineEdit` 和可编辑的 `ComboBox` 可用 `input-purpose` 为 iOS AutoFill 声明用途。它接受 `InputPurpose` 的 `auto`、`username`、`current-password`、`new-password` 和 `one-time-code`。默认 `auto` 使用 `input-type` 原有的系统提示。用途可以和键盘类型组合，例如 `input-type: email; input-purpose: username;`。用 `input-type: password;` 开启密码遮罩，用 `input-type: number;` 限定验证码为数字。系统建议取决于设备设置、可用凭据和应用配置。示例见 [iOS 登录字段](../examples/ios/README.md#登录与验证码字段)。

iOS 的 `TextInput`、`LineEdit`、`TextEdit`、`TextArea`、`SearchField` 和可编辑的 `ComboBox` 可以绑定 `keyboard-capitalization`、`auto-correct` 和 `spell-check`。`KeyboardCapitalization` 提供 `auto`、`none`、`words`、`sentences` 和 `characters`。普通文本默认按句首大写，自动更正和拼写检查沿用系统设置。技术字段默认关闭辅助输入。两个布尔属性可分别覆盖默认行为，密码和验证码字段始终关闭这三项。绑定变化会在当前组字结束后生效。[iOS 键盘辅助输入示例](../examples/ios/README.md#键盘辅助输入)

iOS 的单行编辑器会根据 `input-type` 选择系统键盘。数字和电话字段的 **Done**、搜索字段的系统搜索键和外接键盘的 Return 使用同一套提交规则。通过校验时提交并收起数字、电话或搜索键盘，未通过时保留焦点供继续修改。其他单行字段的回车触发 `accepted`，多行字段的回车插入换行。

`TextInput`、`LineEdit` 和可编辑的 `ComboBox` 支持 `return-key: next;` 和 `return-key: done;`。按键先校验当前值并触发原有提交回调。`next` 沿当前窗口或弹层的 Tab 顺序移动到下一个可编辑字段，跳过隐藏、禁用和只读字段。没有下一项时结束编辑。`done` 直接结束编辑。校验失败会保留焦点。默认的 `auto` 沿用输入类型原有的回车行为。iOS 会设置系统键盘上的 Next 或 Done，数字和电话键盘通过工具栏提供相应按钮。Android 使用系统输入法的对应操作。

iOS 对使用 `return-key: next;` 或 `done;` 的字段默认提供键盘工具栏导航。文本编辑器可以用 `keyboard-navigation: true;` 手动开启，例如让 `TextArea` 显示字段导航，同时保留 Return 换行。`keyboard-navigation: false;` 隐藏导航按钮，回车动作仍由 `return-key` 决定。

**Previous field** 和 **Next field** 先校验并提交当前字段，再沿同一焦点范围移动。到达首尾时，相应按钮不可用。校验失败时保留草稿和焦点，组字期间暂停导航。数字和电话键盘上的 **Next** 也可以直接移到下一项。

iOS 停靠键盘打开时，`TextEdit` 和 `TextArea` 会随输入和光标移动，按需滚动启用了 `auto-reveal-focus` 的外层 `ScrollView` 或 `Flickable`，以显示当前行。iPad 浮动键盘盖住可见光标时，表单内容会暂时移开。键盘移走后恢复原位置，滚动绑定保留原值。[长备注示例](../examples/ios/README.md#键盘辅助输入)

iPad 的系统「收起键盘」按钮会保留当前字段的草稿和逻辑焦点。窗口尺寸变化后键盘继续保持收起，点进原字段可恢复编辑。

[Android 联系人表单回归](../examples/android/README.md#模拟器测试) 检查邮件、电话和网址的系统键盘类型，以及 Next／Done 的校验、焦点切换和收起行为。

iOS 的系统字体回退使用 Core Text 绘制简体中文苹方字形。常规字重的回退字形与文本测量、光标定位和选区共用同一份排版数据，应用可以继续用 `font-family` 指定自己的字体。

单行和多行编辑器都支持按词操作。macOS 使用 Option，Windows 和 Linux 使用 Ctrl。配合左右方向键可移动一个 Unicode 词段，再按住 Shift 可扩展选区。配合 Backspace 或 Delete 可删除光标前方或后方的词段，也可删除已有选区。方向键遵循双向文字的视觉顺序，删除使用文本中的逻辑前后顺序。每次删除可以撤销。运行 `bazel run //examples/form:form -- --bidi` 试用混合方向文字。

`TextInput`、`LineEdit`、`TextEdit`、`TextArea`、`SearchField` 和可编辑的 `ComboBox` 支持 `mouse-selection-mode`。默认 `MouseSelectionMode.characters` 按 Unicode 字素拖选，`MouseSelectionMode.words` 让拖动和 Shift+点击按完整 Unicode 词段扩选。两种模式下双击选中词段，三击选中由换行分隔的整段。按住最后一次点击后拖动，可继续按词或按段扩选。单行输入框的三击会选中整行。密码字段按词或按段选择时以整段文字为单位。

`MouseSelectionMode.caret` 让鼠标只负责聚焦和放置光标。键盘、无障碍和程序仍可设置选区。运行 `bazel run //examples/form:form -- --editor-selection`，点击模式按钮切换三种行为。

触摸屏上，长按文本可选中光标处的词，保持手指接触并拖动可扩展选区，松手后打开编辑菜单。单行和多行编辑器的选区端点会显示手柄，松手后仍可重新拖动手柄来调整边界。菜单使用适合触摸的行高，复用编辑器的复制、剪切和粘贴规则。只读控件仍可选择和复制文字。可在 [Pocket Tasks](../examples/ios/README.md) 的备注页体验。

iOS 长按选词或拖完选区手柄后使用 UIKit 编辑菜单。菜单动作沿用 XRack 的选区、只读、密码保护和撤销历史，键盘可保持打开。iOS 跨应用粘贴可由系统弹窗请求授权。允许后，XRack 会在恢复活跃时核对原字段、文字和选区，再完成这次粘贴。拒绝授权会保留原文，应用真正进入后台时会取消尚未完成的粘贴。

在 `TextArea` 或 `TextEdit` 中按住鼠标拖出编辑区域时，内容会沿该方向自动滚动并继续扩展选区。离边缘越远，滚动越快。不换行的长行也可向左右拖选。松开鼠标、移回编辑区域或失去焦点时停止滚动。运行 `bazel run //examples/form:form -- --long-text-selection` 可体验字素和词段两种拖选方式。

`TextInput`、`LineEdit`、`SearchField` 和可编辑的 `ComboBox` 也支持横向拖选。单行控件在按住期间逐步移动可见范围。松开后保留显示位置，下次按键编辑或移动光标时恢复光标跟随。在同一个示例中，可以用下方的单行控件试用。

这些编辑器也支持 `overwrite-mode: true;`。选区为空时，键盘输入和 IME 提交按 Unicode 字素覆盖光标后的内容，最多到当前行尾。已经选中的文字仍按正常规则替换。掩码字段按槽位覆盖，固定分隔符保留。剪贴板粘贴沿用插入行为。已提交的覆盖支持撤销和重做。

回调可用 `editor.select-word();` 在已聚焦的编辑器里选择光标处的词段，密码字段选择整段并继续隐藏 `selected-text`。在同一个示例里勾选「覆盖输入」，或按「选择光标处的词」体验。

设置 `persistent-selection: true;` 后，工具栏或另一个输入框取得焦点时，原编辑器仍显示选区并提供 `selected-text` 和选区偏移。用键盘或 `focus()` 返回时可继续编辑该选区。编辑器失去焦点时隐藏光标，保存已提交文本的选区。外部修改文本、关闭属性或移除编辑器会清除保存的选区。默认值是 `false`。运行 `bazel run //examples/form:form -- --persistent-selection`，依次点「选择开头」「记录选区」「回到编辑器」。

编辑器和可选取的 `StyledText` 可分别绑定 `selection-color` 和 `selected-text-color`，类型都是 `color`。前者设置选区背景，后者覆盖选中字形的前景色。可使用带透明度的颜色。省略属性时仍使用主题背景和原有文字前景色。运行 `bazel run //examples/form:form -- --selection-colors`，选中文字后点「切换选区配色」对照单行、多行和富文本效果。

可编辑的文本控件可在回调中调用 `insert-text(offset, text)`、`replace-text(start, end, text)`、`remove-text(start, end)`、`append-text(text)` 和 `clear-text()`。偏移是从 0 开始的 UTF-8 字节数，越界值会收拢到文本范围内，字素内部的偏移会落到该字素开头。替换和删除接受两个方向的边界，包含起点并排除终点。

命令参数在回调运行时求值，随后按调用顺序应用到挂载的编辑器。每条命令都使用执行时的文本，复用当前字段的输入校验和 `edited` 回调。校验通过的改动各记为一笔撤销，并保留当前焦点。运行 `bazel run //examples/form:form -- --text-operations`，可以用工具栏修改正文和日期，再尝试撤销。

单行和多行编辑器可用 `maximum-length` 限制 Unicode 字素簇数，负值表示不限。键盘、粘贴、IME 提交和无障碍替换都校验整次编辑。无效输入会保留原值、选区和撤销记录。应用写入的值保持原样，超长文本可逐步删到限制以内。运行 `bazel run //examples/form:form -- --validation` 体验。

`TextInput`、`LineEdit`、`TextEdit` 和 `TextArea` 可用 `minimum-length` 设置提交时所需的最少字素簇数，默认 0。`submit-when` 可绑定到文本及表单状态，默认 true。只读的 `acceptable-input` 把文本格式、长度和提交条件合成一个布尔值，可直接绑定到提交按钮的 `enabled`。单行编辑器的 Enter 提交使用同一规则，仍可输入未完成的草稿。多行编辑器的 Enter 继续插入换行。运行 `bazel run //examples/form:form -- --submission` 体验。

单行数字输入还可用 `float` 绑定 `minimum-value` 和 `maximum-value`，提交范围包含上下限，省略任一端表示该方向不限。`input-type: decimal;` 下可设 `decimals` 限制小数位数，负值表示不限。越界值和小数位数超限的草稿仍可继续编辑，`acceptable-input` 为 false。运行 `bazel run //examples/form:form -- --numeric-validation`，可以试试输入使用逗号小数点的金额。

`TextInput` 和 `LineEdit` 支持 `input-mask`。例如 `"9999-99-99"` 会把输入或粘贴的 `20261117` 格式化为 `2026-11-17`，分隔符也会写入绑定的 `text`。模式里的 `9` 表示 ASCII 数字，`A` 表示字母字素，`N` 表示字母或数字字素，`H` 表示 ASCII 十六进制字符，`X` 表示任意非控制字素。其他字符是固定文本。在 `.xr` 字符串中用 `\\` 转义掩码字符。

草稿可以只填前几个槽位。全部填完后，`acceptable-input` 还会检查输入类型、长度和 `submit-when`，后者适合表达业务规则。粘贴和 IME 提交会整次校验。程序可以继续直接写入绑定值，只有符合掩码的格式才可提交。空掩码恢复普通编辑。运行 `bazel run //examples/form:form -- --input-mask` 体验日期和设备编号。

Android 示例的 **Open dial → Tags** 可用同一字段测试粘贴格式化、IME 组合与提交、无效输入后的选区恢复，以及软键盘 **Done**。 [Android 控件回归](../examples/android/README.md#独立控件回归)

需要校验可变格式时，可为 `TextInput`、`LineEdit`、`TextEdit`、`TextArea` 或可编辑的 `ComboBox` 绑定 `input-pattern`。它使用 Rust 正则语法进行全串匹配。以 `"[A-Z]{2}-[0-9]{3}"` 为例，`AB-12` 可以继续输入，`AB-123` 满足正则提交条件。在编辑区输入、粘贴或提交 IME 后，无法补全成匹配值的草稿会保留原值。此规则与长度、掩码和 `submit-when` 一起生效。

空模式关闭正则校验。字面量模式的语法或构建错误会定位到源代码，动态模式更新后立即重新检查 `acceptable-input`。运行 `bazel run //examples/form:form -- --input-pattern`，可切换英文或中文的设备编号格式。

只读的 `input-validity` 将编辑草稿和提交规则合成三个 `InputValidity` 枚举值。 `acceptable` 表示可以提交，`intermediate` 表示可保留的草稿仍有条件待满足，`invalid` 表示当前值违反编辑规则。过短、数字超出提交范围或 `submit-when` 尚未满足属于 `intermediate`。输入类型、最大长度、掩码或正则拒绝当前值时得到 `invalid`。可编辑下拉框切换成不可编辑模式时也会返回 `invalid`。同一示例会用这三个状态显示不同提示。

`TextInput`、`LineEdit`、`TextEdit` 和 `TextArea` 提供 `editing-finished(text, acceptable)`。用户改动文字后，按 Tab 或点击离开仍挂载的输入框会结束编辑。回调收到当前文本和 `acceptable-input` 的判定，可按结果保存或提示补全。单行输入通过 Enter 校验时先触发 `accepted`，再触发本次编辑的 `editing-finished`。之后离开同一个输入框时，只有新改动才会再次触发。撤销到编辑前的值会清除待结束状态，直接改写绑定值也由应用自行处理。多行输入的 Enter 继续插入换行。运行 `bazel run //examples/form:form -- --editing-finish` 对照确认、失焦和载入示例编号。


## 导航

用 `StackLayout` 把普通控件或组件放在同一块区域，用 `current-index` 选择任一子项。索引从零开始，`count` 计算子项及模型行，`active-index` 读取实际生效的索引。空布局和超出范围的索引返回 `-1`。布局用各可见子项的最小和首选尺寸维持容器尺寸，当前页填充内边距内的空间并遵守自己的尺寸约束。只有当前页参与绘制、输入和无障碍。可隐藏选中的子项，也可禁用其输入并继续显示它。

重复的页面使用 `for key in model : StackItem { … }`。也可以声明静态 `StackItem`，用只读 `active` 读取它是否被选中。用 `current-value` 和各页的 `value` 按业务键选择，插入或重排后会更新索引并保留匹配行的局部状态。`current-value` 和索引绑定二选一。模型页的内容按需生成，未加载页可在 `StackItem` 上声明尺寸提示。在重复行中使用嵌套的 `StackLayout` 时，需将布局声明在行组件内。运行 `bazel run //examples/navigation:navigation -- --stack-layout` 查看普通组件切页，或传入 `--model-stack-layout` 试用共享模型和重排。

焦点位于某一页内部时，切换到另一页会恢复目标页上次聚焦的控件，首次进入时会选中第一个可聚焦控件。模型页按行身份记住焦点，重排后仍能回到同一行的控件。用布局外的按钮切页时，焦点留在按钮上。示例中按 F6 可在页内焦点保持的情况下切页，模型示例还可按 F7 移动草稿行。

`StackView` 的直接子元素为带 ID 的静态 `StackPage`。例如 `home := StackPage { ... }` 的路由是 `"home"`。`initial-page` 设定初始路由，默认使用首个页面。事件处理器可调用 `navigation.push("details")`、`pop()`、`replace("summary")`、`pop-to("home")` 或 `reset("home")`。导航经 `xr.navigate` 校验，成功时触发 `changed(page, depth)`。同一路由在栈中占一个位置，根页可用 `reset` 更换。Rust 可通过 `navigate_stack(id, action)` 和 `stack_snapshot(id)` 操作和读取导航栈。

`navigation.current-page`、`navigation.depth` 和 `navigation.can-go-back` 是可在绑定及纯回调中读取的实时状态。读取经 `xr.stack_read` 保留在 MLIR 中，源码语义检查会拒绝 `initial-page` 经属性或回调形成的初始化环。一次导航前后的读取各自获取当时的栈状态。

`StackPage.active` 表示页面是否位于栈顶，`stack-index` 返回页面在栈中的零基索引，离栈时返回 `-1`。离开栈顶或移除栈中页面前，`leave-requested(destination, removing)` 可返回 `false` 取消整个导航，并可在处理器里显示确认对话框。栈顶先检查，再按栈顶到栈底的顺序检查其他将被移除的页面。也可声明 `callback can-leave-preview <=> preview.leave-requested;`，由 Rust 的 `on_can_leave_preview` 提供守卫。

导航提交后依次触发旧栈顶的 `deactivated()`、各离栈页面的 `removed()`、新栈顶的 `activated()`，最后通知 `StackView.changed(page, depth)`。这些处理器读取到已提交的栈状态。初始路由通过 `active` 和 `stack-index` 可读，生命周期事件在导航提交时触发。运行 `bazel run //examples/navigation:navigation -- --stack-guard` 体验带未保存草稿的页面。

`navigation.reset-to(["home", "details", "preview"])` 用完整路径恢复导航栈，`push-pages([...])` 则把一段路径追加到现有栈。路径可以来自 `[string]` 属性。整段路径都有效且路由唯一时才提交一次并触发一次 `changed`，当前只求值最终的栈顶页面。

只生成栈顶页面的场景，已声明页面的组件状态在返回时可继续使用。焦点位于导航栈内时，切页会记住原页面的焦点。Alt+左方向键或系统返回键可返回上一层。返回请求携带场景中的栈版本，提交时会核对实时版本。运行 `bazel run //examples/navigation:navigation -- --stack` 体验。

iOS 将 `StackView` 的当前页提供给 UIKit。VoiceOver 的返回动作先处理菜单、选项和弹层，再根据焦点确定应返回的栈。焦点在栈外时，只有一个可返回的可见栈时才执行。弹层禁用 Escape 时，返回停在该弹层。页面返回沿用 `leave-requested` 守卫。

iOS 也支持从屏幕起始边缘向内滑动，松手后执行同一套返回流程。RTL 布局从右边缘开始。短滑、取消或连续操作改变了原返回目标时会保留当前页。配置为从同一边缘打开的 `Drawer` 可以继续使用自己的拖动手势。

在 `StackView` 设置 `transition-duration: 220;` 可启用 220 毫秒的横向切页过渡。返回到当前路径上的祖先页面时从左侧进入，其他切页从右侧进入。离场页沿用已布局的绘制内容，栈顶状态和键盘焦点立即切换。过渡期间暂停该区域的鼠标与触摸操作，连续导航会从正在显示的页面片段开始下一次过渡。默认立即切页，时长为零或负数时也是如此。过渡期间视口几何变化会直接显示当前页。

## 按需加载组件

`Loader` 在运行时挂载一个已声明或已导入的组件，Rust AOT 和解释器使用相同的语义。

```xr
Loader {
    active: root.show-panel;
    source: root.panel;
    asynchronous: true;
    loaded => { self.focus(); }
    details := Details { query: root.query; }
    summary := Summary { }
}
```

`active` 是 `bool`，默认 `true`。`source` 是 `string`，例如 `"details"`，对应候选声明前的名称。省略 `source` 时选择首个候选，空字符串或未知名称会卸载当前组件。指定 `source` 或声明多个候选时，每个候选都需要名称。候选名称用于选择，父组件可通过候选声明上的属性绑定与回调交换数据。

可绑定的只读字符串 `status` 有四种值。`"null"` 表示未启用、来源为空或没有默认候选。`"error"` 表示非空来源找不到候选。`"loading"` 表示候选正在等待或分批构造。`"ready"` 在同步模式表示候选已解析，在延迟模式表示实例已创建。挂载完成后可通过 `loaded()` 请求焦点。

`asynchronous` 默认为 `false`。设为 `true` 时，桌面和预览先呈现 Loader 的空内容，从下一帧开始在 UI 线程分批创建子组件。每个 Loader 在构造任务之间检查时间预算和任务数量。整个候选构造完成后才挂载。正在执行的单个构造任务会运行到结束。

等待期间切换来源或关闭 Loader 会取消旧请求并释放已创建的部分实例。将 `asynchronous` 设为 `false`，会在下一次场景更新中完成当前候选的剩余构造。

候选首次挂载时创建实例。关闭或切换后，原实例会在场景更新时卸载，再挂载会重新初始化其局部属性。需要保留的数据可放在父组件。切换时用普通的属性绑定更新候选的输入。

Android 导航示例的 **Open tools** 演示在手指仍按住时替换候选。松手不会点击占据相同位置的新实例，父组件保留已提交的数据。设备回归目标为 `//examples/android:loader_ui_test`。[Android 示例](../examples/android/README.md#动态加载回归)

`Loader` 在每个轴上分别采用显式尺寸或内容的自然尺寸。挂载后，子组件占用 Loader 的内容区域，`padding` 留在内容区域之外。`loaded()` 在新实例进入场景后触发一次。`self.focus()` 会把焦点交给当前内容中第一个可聚焦项。卸载后，该实例的焦点和指向旧实例的输入事件随之失效。

iOS 将已挂载的 `Loader` 作为 UIKit 无障碍容器。切换来源会更新容器里的元素，辅助技术可激活当前实例中的控件。[iOS 动态导航示例](../examples/ios/README.md#动态加载与无障碍返回)

Rust AOT 可以通过 `ui.loader_item::<Details>("area")` 取得已挂载候选的类型化句柄。解释器的 `Instance::loader_item("area")` 返回解释器实例的句柄。`upgrade()` 在原挂载仍有效时返回实例，卸载后返回 `None`。外部组件的句柄使用 `xrack::interpreter::Instance` 类型。

外部来源使用 `source-url`。它接受本地 `.xr` 或 `.mlir` 路径、`file://` URL 和 HTTP(S) URL。来源文件必须导出一个可嵌入的组件。文件有多个导出时，用 `#ComponentName` 选择组件。

```xr
panel := Loader {
    source-url: root.page-url;
    loaded => { self.focus(); }
}
```

外部来源始终异步读取，`status` 从 `"loading"` 转为 `"ready"` 或 `"error"`。切换来源和关闭 Loader 会丢弃过期结果。本地文件可使用相对导入，HTTP(S) 来源使用独立文件，下载需要 `curl`。Rust AOT 程序用 `xrack/interpreter` 启用外部来源支持。`xrack::run` 和 `xrack::run_desktop` 会安装解释器加载器，自定义宿主可调用 `xrack::interpreter::install_external_loader()`。

## 动态组件容器

`DynamicContainer` 是垂直布局容器。普通回调可在其中创建多份编译时已知的组件。组件输入按原类型检查，创建返回该组件的句柄类型，例如 `DetailsHandle`。默认初始化的句柄为空。Rust AOT 和解释器使用相同的容器生命周期。

```xr
component Details inherits VerticalLayout {
    in-out property <string> title: "";
    Text { text: root.title; }
}

export component App inherits Window {
    private property <DetailsHandle> detail;
    callback show-details();
    show-details => {
        root.detail = panels.mount(Details { title: "详情" });
    }

    VerticalLayout {
        panels := DynamicContainer {}
        open-button := Button {
            text: "打开详情";
            clicked => { root.show-details(); }
        }
    }
}
```

`mount` 立即构造实例并返回句柄，子组件在下一次场景更新时参加布局。`mount-async` 返回等待中的句柄，构造任务分帧执行。`mounted()` 在子实例进入场景后触发。可用 `status(handle)` 区分 `"null"`、`"loading"`、`"ready"` 和 `"error"`，也可用 `is-mounted(handle)` 判断实例是否可访问。

`get(handle, "property", fallback)` 读取公开属性。句柄无法访问时返回指定默认值。`set(handle, "property", value)` 写入 `in` 或 `in-out` 属性。`call(handle, "callback", args...)` 调用无返回值的回调。有返回值的回调在参数前再传一个同类型的默认值。`move(handle, index)` 使用从零开始的索引并保留实例状态。`remove(handle)` 卸载或取消实例，使原句柄失效。

Android 导航示例的 **Open cards** 演示同步与延迟创建、移动时保留卡片状态与焦点，以及长按移除后的触摸和句柄失效。设备回归目标为 `//examples/android:dynamic_cards_ui_test`。[Android 示例](../examples/android/README.md#动态卡片回归)

`mount-url(source)` 通过解释器加载外部文件或 URL，返回 `ExternalComponentHandle`。它支持查询状态、移动和移除，外部 URI 与 `Loader.source-url` 使用相同的加载入口。

## 表单与选择控件

`Slider` 的 `value` 可双向绑定到 `float` 属性。鼠标拖动连续更新数值，方向键按 `step` 调整，Home 和 End 移到端点。`changed(value)` 只在交互写入新值时调用，`released(value)` 在鼠标或调节键松开后调用。`orientation: vertical;` 将最小值放在下方。`Switch` 支持 `checked` 双向绑定与 `toggled()`。`ProgressIndicator` 接收 `0..1` 或百分比进度，`indeterminate: true;` 显示运动指示条。

绑定、自定义手柄和回调可以直接读取 `Slider.value`、`Dial.value`、`SpinBox.value`、`DoubleSpinBox.value`，以及 `RangeSlider.lower-value` 和 `upper-value`。这些属性读取已提交的值。`Slider` 和 `RangeSlider` 使用 `live: false;` 时，拖动中的手柄位置先预览，松手后提交数值。绑定到表格或树模型字段的编辑器也可以用 `self.value` 读取当前行的值。

`Slider.position`、`Dial.position` 与 `RangeSlider.lower-position`、`upper-position` 提供 0 到 1 的归一化位置。滑块使用 `live: false;` 时，位置跟随拖动预览，取消拖动后回到已提交值。手柄可用 `visual-position` 或 `lower-visual-position`、`upper-visual-position` 获取屏幕方向的位置，横向 RTL 从右到左，竖向从上到下。重复的编辑器可在各自的组件内读取位置。

鼠标或触摸可以从 `Switch` 和 `SwitchDelegate` 的指示器开始横向拖动。默认滑块跟随指针，松手时越过中点才提交新值，并触发一次 `toggled()`。纵向触摸仍可滚动外层视口，取消手势会恢复当前值。RTL 下向左拖动打开。运行 `bazel run //examples/controls -- --rtl-feedback` 比较两个方向。

iOS 开启「开启／关闭标签」后，`Switch` 和 `SwitchDelegate` 的默认轨道用竖线表示开、圆环表示关。标记跟随控件方向镜像，禁用时也会显示。自定义 `ToggleIndicator` 可用当前的 `checked` 值绘制自己的状态标记。

`Spinner` 使用圆形轨道显示同样的 `progress` 和 `indeterminate` 状态。`BusyIndicator` 在 `running: true;` 时显示旋转的弧线，停止时隐藏并保留布局占位。三种指示器都支持 `color`、`track-color` 和 `animated: false;`，关闭动画会保留可见状态和无障碍进度。圆形指示器还支持 `stroke-width`。

运行 `bazel run //examples/controls:controls -- --indicators` 比较三种指示器，或运行 `bazel run //examples/async_demo:async_demo` 查看与后台任务绑定的加载状态。离屏验证可用 `Renderer::snapshot_at` 指定指示器的采样时间。

`ProgressIndicator` 可分别声明一个 `ProgressTrack` 和一个 `ProgressFill`。轨道铺满控件，填充随进度伸缩并在 RTL 下从右侧开始。不定进度时，自定义填充会在轨道内移动。用父控件设置进度和尺寸，部件可以包含文字、图形或图片。辅助技术从进度条本身读取当前进度。运行 `bazel run //examples/controls -- --custom-progress` 查看示例。

`ProgressIndicator` 可以用 `minimum`、`maximum` 和 `progress` 直接接收业务单位。默认范围是 0 到 1，也可在该范围使用百分比。超出范围的值会截断到端点，空区间的进度为 0。只读的 `position` 返回 0 到 1 的比例，`visual-position` 在 RTL 下返回镜像位置。无障碍数值按配置的范围报告。iOS 的 UIKit 读值使用系统区域格式的完成百分比，进度未知时保留控件名称。

`CheckBox` 支持布尔 `checked`，也可使用类型化的 `check-state`。三种状态是 `CheckState.unchecked`、`CheckState.partially-checked` 和 `CheckState.checked`。设置 `tristate: true;` 后，用户交互会循环三种状态。默认交互会把部分选中切到已选中。三态模式选择 `check-state` 或 `tristate`，布尔模式使用 `checked`。

自定义指示器和绑定可以直接读取 `CheckBox.checked`、`CheckBox.check-state` 或 `Switch.checked`。三态复选框只有完全选中时 `checked` 才为真，部分选中可通过 `check-state` 区分。`CheckDelegate` 和 `SwitchDelegate` 也能从当前行读取状态，回调可以观察模型字段写入后的值。

`check-state: expression;` 保留单向绑定，交互通过 `check-state-changed(state)` 报告请求的状态，适合汇总子项选择。`check-state <=> root.state;` 直接写入可写属性，也可绑定表格或树行的 `CheckState` 字段。运行 `bazel run //examples/controls:controls -- --check-state` 体验全选和独立三态循环。

`RangeSlider` 使用 `lower-value` 和 `upper-value` 选择一个区间，两端可分别双向绑定到不同的 `float` 属性。Tab 在两个手柄之间移动焦点，方向键按 `step` 调整，Home 和 End 移到当前手柄可达的边界。点击轨道移动最近的手柄。手柄相遇时，向较小值拖动下限，向较大值拖动上限。触摸重合手柄时，沿滑轨移动至少 8 个逻辑像素才选定端点，以免轻微抖动改变拖动方向。横向和纵向都使用 `changed(lower, upper)` 和 `released(lower, upper)` 报告有效区间。外部写入交叉值时，显示的下限收拢到上限，下一次有效交互会写回有序区间。运行 `bazel run //examples/controls:controls` 体验。

横向默认手柄和自定义手柄都继承 `layout-direction`。在 `right-to-left` 下，最小值在右侧，拖向左侧或按左方向键增加数值。纵向仍从底部向顶部递增，区间的下限和上限各自保有焦点和无障碍数值。运行 `bazel run //examples/controls -- --rtl-sliders` 切换方向并体验。

`Slider` 和 `RangeSlider` 支持 `snap-mode`。用 `ScrollSnapMode.always` 在拖动时吸附到以最小值为起点的步进刻度，或用 `on-release` 在松手时吸附。最大值也可作为终点。设置 `live: false;` 后，鼠标和触摸拖动先预览手柄位置，松手后提交，数值变化时触发 `changed`，随后触发 `released`。Escape 可取消待提交的预览，键盘和辅助技术可以直接调整数值。运行 `bazel run //examples/controls:controls -- --slider-snap` 体验。

Android 可构建 `//examples/android:controls_app` 体验触摸拖动、吸附、禁用输入和弹层交互。**Open dial → Range** 演示重合手柄的触摸选定、RTL 拖动和前后台切换后的预览取消。设备回归与同一份界面的 AOT／解释器对照命令见 [Android 控件回归](../examples/android/README.md#独立控件回归)。

`Slider`、`RangeSlider`、`Dial`、`SpinBox` 和 `DoubleSpinBox` 可设置 `wheel-enabled: true`。滚轮向上或向右增加数值，`RangeSlider` 调整靠近指针的手柄。滚轮按系统报告的行增量计步，触控板每 28 个逻辑像素累计为一步。一次输入的多步合并提交，数值变化时触发 `changed` 或 `edited`。上层 `WheelHandler` 仍可先消费输入。非循环控件到达边界后，同方向的后续滚轮会交回外层视口。带修饰键的滚轮沿用原路由。运行 `bazel run //examples/controls:controls -- --wheel` 可试用。

`Pane` 提供主题背景，`ControlFrame` 另有一像素的主题边框。两者默认内边距为 12 个逻辑像素，可分别设置四边。子元素的坐标相对于内边距内的内容区，也可以放入布局容器。未指定外部尺寸时会测量可见内容，`content-width/height` 可覆盖隐式内容尺寸。`available-width/height` 是布局完成后发布的只读内容区尺寸，子元素可用百分比尺寸填满内容区。空白区域拦截点击，`enabled: false` 会禁用其中的交互，`clip: true` 会按圆角裁剪子元素。运行 `bazel run //examples/controls:controls -- --panes` 可试用。

`Dial` 的 `value` 同样可双向绑定。输入模式使用 `DialInputMode`，可取 `circular`、`horizontal` 或 `vertical`。圆形模式按角度定位，直线模式按水平或垂直位移相对调节。方向键按 `step` 调节，Home 和 End 移到端点。默认使用 280 度限位圆弧，`wrap: true;` 使用整圈并允许跨过端点。可用 `start-angle` 和 `end-angle` 自定义以十二点方向为零、顺时针递增的角度。事件 `changed(value)` 和 `released(value)` 与 `Slider` 相同。

[Android 控件回归](../examples/android/README.md#独立控件回归) 的 **Open dial** 面板可用触摸验证三种模式、拖出旋钮后的输入捕获和前后台切换。

`SpinBox` 的 `value` 是可双向绑定的整数。它支持 `minimum`、`maximum`、`step-size`、`wrap` 和 `read-only`。方向键、步进按钮和无障碍步进以当前有效草稿为起点，只提交步进后的结果。没有有效草稿时以已提交值为起点。直接输入的数字在 Enter 或移开焦点时提交，Escape 丢弃草稿。交互改值触发 `edited(int)`。`DoubleSpinBox` 沿用相同规则，按配置的小数精度和分隔符解析草稿。

`DoubleSpinBox` 使用相同的交互，`value` 和步进范围使用 `float`，改值触发 `edited(float)`。`decimals` 指定 0–15 位小数，默认 2 位，步进与提交都按此精度舍入。`decimal-separator` 可设为 `"."` 或 `","`，显示和输入使用同一分隔符。编辑期间外部更新数值会使旧草稿失效。在表格和树视图里可双向绑定可写记录的浮点字段，提交时保留同一行其他字段的更新。运行 `bazel run //examples/controls:controls -- --decimal-spin` 体验。

iOS 会按数值类型显示整数、小数或可输入负号的系统键盘。键盘上方的 **Done** 提交草稿并收起键盘，切换到另一控件也会提交。键盘收起时，点步进箭头可以直接调整数值。系统键盘的英文点号或逗号会转换成 `DoubleSpinBox` 配置的分隔符。校验、范围限制和舍入沿用控件本身的规则。可在 [Pocket Tasks](../examples/ios/README.md) 的 **Readings** 页面体验。

`Tumbler` 用 `model: [string]` 显示标签，或用 `count` 生成从 0 开始的数字。选中项居中，`current-index` 可双向绑定，用户提交时触发 `changed(index)`。`visible-item-count` 取 1–15 之间的奇数，默认 5。默认在模型项多于可见项时循环，也可显式设置 `wrap`。

鼠标或触摸拖动时预览。慢速松手直接吸附，快速甩动会先按惯性滚动，再在目标行居中时提交一次。惯性期间保留已提交的值，循环模型可以跨过首尾，非循环模型在边界内吸附。再次按住、用键盘或滚轮调整、控件被禁用、模型重排或替换都会取消待提交的惯性选择。

`flick-deceleration` 用逻辑像素每平方秒表示减速度，默认 `2400`，设为 `0` 可关闭惯性。只读的 `dragging`、`flicking` 和 `moving` 可用于绑定样式。滚轮按项移动，触控板像素滚动累积到一行后提交。方向键、Home、End 和 Page Up / Page Down 支持键盘选择。运行 `bazel run //examples/controls:controls -- --tumbler` 体验小时和分钟选择。

[Android 控件示例](../examples/android/README.md#独立控件回归) 的 **Open wheels** 面板展示触摸滚轮和曲线卡片选择。设备回归覆盖首尾循环、卡片镜像、禁用状态和应用进入后台时取消未提交的拖动。

iOS 开启「减弱动态效果」时，`Tumbler`、`PathView` 和时间选择器的滚轮仍跟随手指。松手时就近选中当前行，跳过惯性滑动。滑动期间开启设置时，滚轮在下一帧选中当前显示位置对应的行。时间弹层里的选项仍是草稿。滚轮减速时，Tab 可以移动焦点，滚轮继续滑动。点确认按钮或在其上按 Return／空格，会先让可见行落位，再校验草稿并写入绑定。取消则丢弃草稿。

记录模型可以用 `text-role` 指定显示标签，用 `value-role` 指定业务值。两项都是常量字段路径。标签字段必须为 `string`，业务字段可以是 `string`、`int`、`float`、`bool`、`Date` 或 `Time`。省略 `text-role` 时使用 `text` 字段，省略 `value-role` 时使用标签字段。

```xr
zone-wheel := Tumbler {
    model: root.zones;
    text-role: "label";
    value-role: "offset";
    current-value <=> root.offset;
    value-selected(offset) => { root.last-offset = offset; }
}
```

记录模型上的 `count`、`current-index` 和 `current-text` 反映滚轮显示。可以绑定 `current-index` 或 `current-value` 中的一项。按值绑定时，重排后仍选择匹配值的第一行。找不到保存的值时先显示首行，直到用户提交选择才写回业务值。空模型的索引为 `-1`，标签为空。提交先更新绑定，再依次触发 `changed(index)` 和有类型的 `value-selected(value)`。运行 `bazel run //examples/controls:controls -- --record-tumbler` 体验重排和按值选择。

在 `Tumbler` 内声明一个匿名 `TumblerOption`，可为每一行定制外观。模板可读取 `option-text`、零基 `option-index`、`option-enabled`、`option-selected` 和 `option-displacement`。`option-selected` 表示已提交的选择。`option-displacement` 是行中心与滚轮中心之间的距离，以一行的高度为单位，向下为正，拖动时连续变化。

`option-data` 保留原始数据类型。数字滚轮的是 `int`，字符串模型的是 `string`，记录模型可直接访问字段。布局只为与视口相交的行实例化并裁剪模板，滚轮统一处理鼠标、触摸、键盘和无障碍。时区示例用记录字段显示缩写，并用位移控制透明度。

### PathView

`PathView` 沿 SVG 路径按弧长排列选项，适合卡片轮播和曲线选择器。用 `path` 写单一连续路径的 SVG 指令，坐标以控件内的 `0…1` 为基准，四边留出半个卡片的空间。默认路径为横向直线。用 `item-width` 和 `item-height` 设置卡片尺寸，默认分别为 `96px` 和 `72px`。路径可动态切换。无效或零长度路径上没有可选择的卡片。

```xr
PathView {
    model: root.entries;
    text-role: "title";
    value-role: "id";
    current-value <=> root.selected;
    path: "M 0 0.86 Q 0.5 -0.46 1 0.86";
    item-width: 118px;
    item-height: 96px;
    PathViewOption {
        background: option-data.tint;
        opacity: 1 - Math.abs(option-progress - 0.5);
        Text { text: option-text; }
    }
}
```

选择项吸附到路径长度的中点，`option-progress` 是该卡片从路径起点算起的弧长占比。`PathViewOption` 也可读取原始模型数据、标签、索引、选中状态和位移。数字、字符串与记录模型沿用滚轮的选择语义。拖动方向由 `orientation` 指定，默认横向。视图支持惯性、鼠标点击、方向键和原生无障碍选择。RTL 会镜像路径与横向导航。iOS 将 `PathView` 公开为一个可调节元素，读出当前卡片标签，增减沿模型顺序选择上一项或下一项。可以用 `accessible-name` 指定控件名称。运行 `bazel run //examples/navigation:navigation -- --path-view` 体验曲线、直线和 RTL 切换。

`Button`、`ToolButton` 和 `RoundButton` 的只读属性 `pressed` 可绑定到内容或样式。鼠标和触摸按住按钮、或聚焦后按住激活键时返回真。移出、滚动接管或松开会清除状态。正常松手产生的 `clicked()` 可以读取到已经更新的松开状态，连发回调可以读取到仍按住的状态。运行 `bazel run //examples/controls:controls -- --button-pressed` 查看实时提示。

`Button`、`ToolButton` 和 `RoundButton` 可用 `auto-repeat: true;` 在鼠标、触摸或键盘按住时重复触发 `clicked()` 或关联的 `Action`。用 `auto-repeat-delay` 和 `auto-repeat-interval` 设置首次等待时间和间隔，默认 300 和 100 毫秒。间隔为零或负值时，按下和松开沿用普通点击。

普通按钮可以声明 `double-clicked()`。同一输入来源在同一个按钮上完成两次短按，松开位置相隔不超过 6 个逻辑像素、时间差不超过 450 毫秒时，两次点击先各自提交，再触发双击回调。关联的 `Action` 仍按每次点击执行。长按、连发、滚动接管和失焦会中断配对。第二次点击若禁用了按钮或换了底下的记录行，双击回调会取消。运行 `bazel run //examples/controls:controls -- --button-double-click` 比较两个计数。

短按仍在松开时触发一次。首次重复发生后，松开会结束重复。移出按钮、失焦、禁用、改变计时配置或触摸手势被滚动接管时，当前重复会取消。事件循环繁忙时跳过错过的触发。辅助技术的点击仍执行一次操作。运行 `bazel run //examples/controls:controls -- --repeat-buttons` 比较短按、连发和按钮到达上限后停止。

普通按钮也可声明 `press-and-hold()`。鼠标、触摸或聚焦后按住空格或 Enter 达到 `press-and-hold-interval` 时触发一次，默认 800 毫秒，非正值关闭长按。移动超过 6 个逻辑像素、移出、失焦或滚动接管会取消等待。长按触发后，松开结束这次手势。开启 `auto-repeat` 时由连发接管。关联 `Action` 的按钮可在短按时执行命令，在长按时执行独立回调。运行 `bazel run //examples/controls:controls -- --button-hold`，短按「增加」后长按同一按钮可以把数值归零。

普通按钮可声明 `pressed()`、`released()` 和 `canceled()`。三种输入共用这个顺序。按下和释放事件中的 `pressed` 属性已经更新，正常点击在 `released()` 之后处理。在外面松手、失焦或滚动接管时发出 `canceled()`。长按和连发期间，按下与释放事件仍各对应一次真实手势。运行 `bazel run //examples/controls:controls -- --button-lifecycle` 查看顺序。

[Android 按压示例](../examples/android/README.md#独立控件回归) 演示触摸短按、单次长按、连发到上限，以及触点移动或切到后台后的取消事件。

可在有副作用的回调中调用当前组件中可静态寻址的 `Button`、`ToolButton` 或 `RoundButton` 的 `click()`。程序化点击同步发出 `pressed()` 和 `released()`，再走普通激活路径，包括勾选、互斥组和共享 `Action`。按下回调禁用按钮时发出 `canceled()`，释放回调禁用按钮时跳过激活。同一按钮的递归点击会被忽略。运行 `bazel run //examples/controls:controls -- --programmatic-click`，点击触发按钮或按 F4 查看顺序。

`animate-click()` 会立即进入按下态，并由窗口在 100 毫秒后释放和激活。释放前再次调用会重启计时。等待期间禁用按钮会取消激活，卸载后清除按下态。上述示例也可按 F5 体验动画。

[Android 点击路径示例](../examples/android/README.md#独立控件回归) 验证触摸双击与共享命令的顺序，以及程序化点击、动画重启和禁用取消。

`toggle()` 同步修改按钮、共享 `Action` 或互斥组拥有的状态。它可以修改禁用或隐藏按钮的状态，仅更新绑定和外观。互斥组在选中该项和清空选择之间切换。使用 `current-value` 的 `ButtonGroup` 以空字符串表示无选中项，选项使用非空的业务值。运行 `bazel run //examples/controls:controls -- --programmatic-toggle` 后按 F6 体验。

重复按钮可在行内回调中调用 `self.toggle()`。它同步写入当前行的按钮状态或双向绑定的属性。同一个回调随即读取 `self.checked` 就能获得新值。运行 `bazel run //examples/controls:controls -- --checkable-buttons`，在下方的手动切换区体验逐行切换和共享绑定。

`DelayButton` 在鼠标、触摸或键盘持续按住达到 `delay` 毫秒后设置 `checked`，并触发一次 `activated()`。默认延时 3000 毫秒。进度颜色可用 `progress-color` 设置，`checked` 可双向绑定。松手、移出、失焦或禁用会取消仍在等待的确认。已确认的按钮再次点击时复位。

通过辅助技术操作时，第一次激活开始等待，提示就绪后第二次激活提交。提前再次激活或按 Escape 会取消。运行 `bazel run //examples/controls:controls -- --delay` 体验。

[Android 延时确认示例](../examples/android/README.md#独立控件回归) 验证滚动区域里的按压进度、提前松手、禁用和确认后复位。

`Button`、`ToolButton` 和 `RoundButton` 可以用 `checkable: true;` 保留选中状态。`checked` 默认为 false，可用 `checked <=> root.pinned;` 双向绑定。用户激活时先写入选中值，再发出 `toggled()` 和 `clicked()`。程序修改绑定值时只更新选中态和无障碍状态。未关联外部绑定的重复项各自保留状态，移动模型行时选中态跟随原行。绑定了 `Action` 的按钮继续使用命令的选中值。运行 `bazel run //examples/controls:controls -- --checkable-buttons`。

可以在表达式中读取具名按钮的 `button.checked`，例如让 `ButtonContent` 中的文字跟随选中态。读取会跟踪实际持有状态的按钮、`Action` 或组，可在回调切换状态后立即使用。命令式修改使用 `toggle()`，或写入 `checked <=>` 所绑定的属性。重复按钮在自身绑定和回调中用 `self.checked` 读当前行，在自定义内容中可直接读 `checked`。重排时选中状态和内容一起跟随原行。给按值选择的组提供显式的 `value`，可以让文字随选中态变化，同时保留稳定的选择值。

`ComboBox` 从 `[string]` 模型选择一项，可绑定 `current-index` 或 `current-value`，用户选择时触发 `selected(string)`。按值绑定取首个匹配项。弹层在窗口顶层绘制，只绘制当前可见行，支持滚轮、方向键、Home、End、PageUp、PageDown、Enter 与 Escape。点击弹层之外会关闭弹层并消费本次点击。

表达式可直接读取 `choice.count`、`choice.current-index`、`choice.current-text` 和 `choice.current-value`。`count` 包含禁用项。没有匹配项时，索引为 `-1`，选中标签为空。按值绑定保留提交的业务值，即使模型暂时没有匹配项。按索引选择时，读出的业务值来自该行，无选中项时使用字段类型的默认值。重复列表内可在各自的组件中读取下拉框状态。

记录模型可用 `text-role` 指定显示字段，用 `value-role` 指定业务值字段。角色由编译器校验，`current-value` 和 `value-selected` 采用该字段的类型，支持字符串、整数、浮点数、布尔值、`Date` 和 `Time`。`selected(string)` 返回显示标签。按值选择会在模型重排后查找相同业务值，弹层高亮随行标识移动。可编辑的记录下拉框按显示标签匹配，提交未匹配的自由文本时可在 `accepted(string)` 中处理。运行 `bazel run //examples/selection:selection -- --record-combo` 体验。

记录下拉框可以用 `enabled-role: "access.available";` 指定布尔字段。禁用项仍显示，鼠标、键盘、补全和辅助技术会跳过它，提交时重新核对当前模型。程序可保留指向禁用项的当前值。非编辑下拉框支持按前缀定位，短时间连续输入会合成前缀，重复按同一个字符会在匹配项之间循环。弹层关闭时直接选中，打开时只移动高亮，按 Enter 才提交。

给 `ComboBox` 添加一个匿名的 `ComboOption` 子项，可以为字符串模型和记录模型定制下拉选项的外观。模板使用只读的 `option-text`、`option-index`（从 0 开始）、`option-enabled`、`option-selected` 和 `option-highlighted`。每个可见选项都使用自身的数据和状态绘制，鼠标、键盘和辅助技术仍通过下拉框提交原选项。重复列表中的下拉框可把模板放在各自的组件内。运行 `bazel run //examples/selection -- --record-combo` 查看示例。

模板里的 `option-data` 保留模型行的类型。字符串模型可直接显示它，记录模型可读取 `option-data.notes` 或 `option-data.access.available` 等字段。字段由编译器校验。修改一项的说明或图标会更新正在显示该项数据的弹层。

`ComboContent` 是 `ComboBox` 的单个直接子项，用来替换不可编辑时的选中标签。可以用 `choice.current-text` 和 `choice.current-value` 组合文字、编号和图标。内容布局在文字区域内，并和下拉箭头一起适应 RTL。设为不可见会恢复默认标签。切换到编辑模式会暂停求值自定义内容，输入区由自带的编辑器接管。点击、键盘和无障碍仍使用 `ComboBox` 的语义。

在 `ComboBox` 内声明一个 `ComboPopup`，可绑定 `width`、`max-height`、`row-height`、`padding`、`background`、`border-color`、`border-width` 和 `border-radius`。`max-height` 包含内边距，弹层根据窗口剩余空间向上或向下展开。`width` 的百分比相对于控件宽度，`max-height` 的百分比相对于窗口高度。默认行高是 28px，默认最多显示八行，默认宽度取控件宽度与 96px 中较大者。设置 `scroll-indicator: true` 后，内容溢出时会在逻辑末端显示滚动位置，滚轮和键盘仍用于浏览选项。运行 `bazel run //examples/selection -- --popup-style` 体验。

iOS 和 iPadOS 的 `ComboBox` 下拉项及 `SearchField` 建议会随系统文字大小调整字号和行高。`row-height` 是常规字号下的基准，显式弹层宽度和最大高度仍约束布局。弹层避开系统安全区、停靠键盘和浮动键盘。键盘移开后，仍打开的弹层会恢复位置。选项过多时可在弹层里滑动或用 VoiceOver 翻页。点击下拉选项会更新选值，点击搜索建议会填入输入框，随后可用搜索按钮或键盘搜索键提交。

回调中可调用 `choice.open-popup()` 或 `choice.close-popup()`。窗口处理请求时会检查控件的可见性、启用状态和当前模态范围。打开后可绑定只读的 `popup-open` 和 `highlighted-index`，下标从 0 开始，弹层关闭或没有高亮项时为 `-1`。高亮随键盘、鼠标或滚动更新，确认选项时才提交选择。运行 `bazel run //examples/selection -- --popup-control` 查看示例。

`editable: true;` 让 `ComboBox` 的文字区域支持输入、IME、剪贴板和撤销。草稿可用 `edit-text <=> root.draft;` 双向绑定，修改时触发 `edited(string)`。点击箭头或按 F4 可打开列表，列表优先定位到与草稿精确匹配或具有相同前缀的项。关闭列表后按 Enter 提交草稿并触发 `accepted(string)`。精确匹配时更新选项，自定义文本以 `current-index: -1` 表示或写入 `current-value`。运行 `bazel run //examples/selection:selection -- --editable-combo` 体验。

`choice.edit-text` 读取编辑草稿，显式清空后返回空字符串。自动编辑框尚未改动时，草稿跟随当前选中的标签。

如果提交时模型中没有匹配项，可在 `accepted(text)` 中把名称加入模型。回调结束后，控件会从最新模型中查找第一个启用的精确匹配项，并写回索引或业务值。回调主动改写草稿或选择时，控件保留回调的决定。`editing-finished` 可以读取更新后的选择。运行 `bazel run //examples/selection:selection -- --combo-append` 体验新增标签和带业务键的记录。

可编辑下拉框支持与单行文本编辑器相同的提交校验。可用 `minimum-length`、`maximum-length` 和 `submit-when` 设置规则，使用数字输入时还可绑定范围和小数位数。只读的 `acceptable-input` 随草稿和规则变化，进入不可编辑模式时为 false。Enter 提交前会重新校验，未完成或越界的草稿可以继续修改。显式选中模型中启用的选项会更新选择和草稿。运行 `bazel run //examples/selection -- --validated-combo` 体验。

可编辑 `ComboBox` 也可用 `input-mask` 格式化键盘输入、粘贴和 IME 提交的草稿。草稿有改动时，有效的 Enter 确认会先发出 `accepted(text)`，再发出 `editing-finished(text, acceptable)`。从列表选中已有项会先交付选择回调，再以该选项的文字和校验结果结束编辑。按 Tab 或点击离开时会交付当前草稿及其校验结果。切换成不可编辑模式会清除待结束状态。运行 `bazel run //examples/selection:selection -- --combo-lifecycle` 试用设备编号掩码和记录选项。

`auto-complete: true;` 可在编辑区显示模型中首个前缀匹配项的后缀。光标位于草稿末尾、没有选区且 IME 未预编辑时才显示。已有精确匹配时保留当前文字。按可见后缀所在方向的箭头键插入补全并形成一笔可撤销的编辑。按 Escape 收起提示。模型变化会使过期候选失效。按 Enter 仍提交草稿，Tab 仍切换焦点。

Android 导航示例的 **Open choices** 覆盖预编辑、记录选项的业务值、禁用项和输入法确认。设备回归目标为 `//examples/android:combo_ime_ui_test`。[Android 示例](../examples/android/README.md#可编辑下拉框与输入法回归)

`SearchField` 支持 `text` 双向绑定，搜索图标提交查询，清空图标和选中候选都可撤销。候选由应用通过 `suggestions` 提供，类型为 `[string]`。可用 `suggestions-for` 标记生成这批结果的查询，只有标记与当前输入一致时才显示。方向键高亮候选，Enter 选中高亮项或提交当前查询，Escape 收起候选。候选变化使旧的高亮失效。 iOS 将文本区、提交按钮和可用的清空按钮按顺序提供给系统无障碍。清空后可以继续输入，也可以用 Command+Z 恢复查询。

Android 导航示例的 **Open search** 用定时器模拟乱序结果，覆盖候选失效、触摸选择、撤销和显式提交。设备回归目标为 `//examples/android:search_results_ui_test`。[Android 示例](../examples/android/README.md#搜索候选回归)

`ComboBox` 和 `SearchField` 继承 `layout-direction`。RTL 下下拉触发器和清除按钮在左侧，搜索按钮在右侧，候选弹层和项目文字从右侧对齐。行内补全按草稿的文字方向摆放。运行 `bazel run //examples/selection -- --rtl-choice` 切换体验。

在 `ComboBox` 内声明一个 `ComboIndicator { width: 44px; height: 24px; … }` 可替换下拉箭头。部件在逻辑末端居中，尺寸会为文字和输入区留出空间，RTL 时随控件镜像到左侧。部件中的图案没有独立的点击、焦点或无障碍节点。点击图案、键盘打开和无障碍激活仍由下拉框处理。推荐在图案下使用 `Label`、`Image`、`Rectangle` 或 `Path`。

`accepted(string)` 报告显式提交的查询，`selected(string)` 报告选中的候选。两种操作都触发 `search-triggered(string)`。`live: true;` 也会在输入变化时触发它。`bazel run //examples/async_demo:async_demo` 演示用 Tokio 查询并在 UI 线程写回候选，示例单独维护一批候选和虚拟化的结果列表。

`RadioGroup` 可混用固定的 `RadioButton`、`if` 条件项和多段 `for`。组的 `current-index` 使用展开后的顺序，隐藏的条件项保留槽位。动态选项可用 `current-value` 绑定稳定且唯一的业务值，重排和插入时保持原有选择。选项暂时隐藏时保留绑定值，重新显示后恢复选中，模型刷新本身不会触发 `selected(string)`。

`RadioButton.checked` 可用于绑定和自定义指示器，支持直接放在 `RadioGroup` 中的选项和通过 `group:` 加入的选项。在重复选项内使用 `self.checked` 读当前行。按值选择时，可以为选项声明稳定的 `value`，再用选中态修改文字。`RadioDelegate.checked` 跟随列表或网格当前显示的选中项。

`title` 用于显示组标题，也可作为无障碍名称。显式的 `accessible-name` 优先。任一选项拥有键盘焦点时，组的只读 `has-focus` 为真。`focus()` 和 `FocusScope.initial-focus` 优先进入已选且可用的选项，否则进入第一个可用选项。`clear-focus()` 会清除组内焦点。运行 `bazel run //examples/selection:selection -- --mixed-radio-options`，点击「聚焦选项」后可以用方向键选择，再按 F3 清除焦点。

组件根部也可声明非可视的 `ButtonGroup`，各个 `RadioButton` 用 `group: group-id;` 加入同一组，可分别放在不同的行、列或条件容器中。`current-index` 按声明顺序分配槽位，隐藏项保留槽位。`RadioButton.value` 可提供稳定的业务值，`current-value` 和 `selected(string)` 使用该值，省略时沿用 `text`。每组在 Tab 顺序中只占一站，方向键在可用选项间循环。设置组的 `enabled: false;` 停用整组，`accessible-name` 会用于无障碍分组。运行 `bazel run //examples/selection:selection -- --radio-groups` 体验。

`Button`、`ToolButton` 和 `RoundButton` 也可用 `group: group-id;` 加入组。成员使用组件内的固定声明路径，入组自动启用选中态。互斥组使用组的 `current-index` 或 `current-value` 写入选中项。一次新选择先提交选中值，再触发目标的 `toggled()` 和组的 `selected(string)`。点击当前选中按钮仍触发 `clicked()`。方向键移动焦点并改变选择。

`ButtonGroup { exclusive: false; }` 可组合普通按钮、`CheckBox` 和 `Switch`。普通按钮可用 `checked <=> root.flag;` 绑定各自的选中态。全选框用 `check-state <=> options.check-state;` 绑定汇总结果，普通成员各自保留 Tab 停靠点。汇总使用声明中的所有成员，包括隐藏或禁用的选项。`options.set-check-state(CheckState.checked)` 和 `options.set-check-state(CheckState.unchecked)` 分别批量选中和清除，传入部分选中值会保留当前成员状态。批量操作通过各成员的绑定更新属性，成员的用户操作回调由各自的交互触发。运行 `bazel run //examples/controls:controls -- --grouped-buttons` 体验两种分组。

[Android 导出选项示例](../examples/android/README.md#独立控件回归) 把自定义全选指示器、三态复选框和可拖动开关放在同一组，演示批量更新与成员交互各自的回调顺序。

`TabWidget` 可以直接声明多个 `Tab`，也可以用一个 `for … : Tab` 从模型创建标签和页面。可双向绑定 `current-index`，并通过 `orientation` 切换顶部或侧边标签栏。方向键跳过隐藏和禁用项，Tab 键把每组选项视为一个焦点停靠点。

Android 的 `//examples/android:navigation_app` 展示单选组、标签页、滑动页、分页圆点、抽屉和页面栈。[页面导航与触摸回归](../examples/android/README.md#页面导航与触摸回归) 提供设备测试和 AOT／解释器对照命令，页内计数可用于检查切换后的局部状态。

模型驱动的 `Tab` 可以用 `title: doc.title` 和 `value: doc.id`。内建标签栏会读取各行标题及可用状态，页内组件在首次选中时才创建。已访问页的局部状态随模型行保留，包括在同一模型内插入、重排或切页。运行 `bazel run //examples/navigation:navigation -- --model-tabs` 试用模型标签、RTL 侧边栏和页码指示器。

`TabWidget`、`TabBar` 和 `SwipeView` 的只读 `count` 提供总项数，包含隐藏或禁用项。它可直接用于 `PageIndicator.count` 或工具栏标签。模型变化会更新计数。

`TabBar` 独立排列 `TabButton`，可与 `SwipeView` 双向绑定同一个 `current-index`。标签可逐项声明，也可用一个 `for` 从模型生成。横排默认按文字测量宽度，可用 `width` 固定某项。竖排使用 `height`。超出视口时可用滚动条、触摸或沿排列方向滚动浏览。方向键和 Home / End 跳过隐藏或禁用项，绑定变化后会把新的选中项滚入视口。隐藏的标签保留声明索引。运行 `bazel run //examples/navigation:navigation -- --tab-bar` 体验。

iOS 增大系统字号后，`TabBar` 和 `TabWidget` 的标签按新字号重新测量，并把当前选中的标签带回视口。可用空间足够时，独立关闭按钮使用 44 × 44 点的触控区域。显式尺寸仍可约束布局。空间紧张的标题会缩略，VoiceOver 仍可读取完整名称。

在 `TabBar` 或 `TabWidget` 上为模型生成的标签设置 `reorderable: true`，即可拖动标签调整顺序。把焦点放在标签上，也可用 Ctrl + Shift + 方向键重排，macOS 上使用 Command + Shift。拖动时插入线显示落点，靠近溢出边缘会自动滚动。标签必须由一个 `for` 直接读取可写列表属性。释放鼠标或触屏后会移动模型中的原行，复用该行的页面状态，随后触发 `tab-moved(from, to)`，参数为移动前后的索引。按 Escape 可取消，拖动期间模型变化也会取消本次移动。Rust 宿主可用 `VecModel::move_row(from, to)` 移动同一个模型中的行。运行 `bazel run //examples/navigation:navigation -- --model-swipe` 体验共享模型、页面状态和标签重排。

iOS 的 VoiceOver 操作列表也可移动标签。水平栏提供左右移动，垂直栏提供上下移动。动作跳过隐藏或禁用的标签，必要时滚动以显示移动后的标签，并保留当前选中的页。

`TabWidget` 的标签尺寸会保留可读的标题宽度和可点击的行高。标题超出空间后可沿标签栏触摸滑动或滚动。横排可以用横向滚轮或 Shift + 滚轮，竖排使用上下滚动。只有标题栏滚动，页内内容的位置保持稳定。键盘选中超出视口的标签会将它滚入视野。运行 `bazel run //examples/navigation:navigation -- --model-tabs`，开启「窄标签栏」试用拖动、滚动和重排。

`TabBar` 和 `TabWidget` 的 `tabs-closable: true` 会为标题添加关闭按钮。`TabButton` 或 `Tab` 的 `closable` 可以覆盖默认值，例如用 `closable: false` 固定某一页。点击按钮或在标题聚焦时按 Delete 会触发 `tab-close-requested(index, value)`，参数为零基索引和当前 `value`。未设置 `value` 时传空字符串。应用可以在这个回调中保存或确认，再用 `Model.remove` 移除对应的行。异步确认可使用稳定的业务标识重新定位。关闭背景页时可以保留正在编辑的页面。 iOS 在无障碍树中分别公开标签、关闭按钮和当前页内容。 标签栏溢出时，可从可见标签或关闭按钮发起 UIKit 滚动，滚动只移动标签栏。关闭背景标签时保留当前页焦点，关闭当前标签后，焦点随应用选中的后续页面转移。`--model-tabs` 示例包含标题关闭按钮和固定的「概览」页。

在回调里可调用 `bar.move-item(from, to)`、`tabs.move-item(from, to)` 或 `pages.move-item(from, to)`，移动同一个可写列表中的原行。参数是零基索引，`to` 是移动完成后的位置。相同索引或越界时保持原样。命令在回调内立即生效并保留选择和页面状态。`TabBar` 和 `TabWidget` 触发 `tab-moved(from, to)`，`SwipeView` 触发 `page-moved(from, to)`。程序化移动也可以移动隐藏或禁用的行，`reorderable` 只控制手势和键盘重排。

Android 导航示例的 **Open cards → Open tabs** 提供模型标签的触摸重排、后台页关闭和 Delete 关闭。设备用例检查重排后保留页面状态，以及关闭后台页后继续操作当前页。 [Android 模型标签页回归](../examples/android/README.md#模型标签页回归)

`TabBar`、`SwipeView` 和 `TabWidget` 也可以通过 `current-value <=> root.selected` 共享同一个字符串业务标识。分别给 `TabButton`、`SwipePage` 和 `Tab` 声明 `value`。标签和页面可以各有自己的展示顺序。插入或重新排列标签时，会重新计算只读的 `current-index`，业务标识和已打开的页面保持稳定。

在按值模式下，各控件查找第一个可见、启用且值匹配的项。找不到时没有选中项，`current-index` 为 `-1`，保存的 `current-value` 会保留。这样关闭页面时，应用可在同一个回调里选定要转到的文档。用户点击或用键盘、手势选择时，先写回新值，再发出 `changed(index)`。运行 `bazel run //examples/navigation:navigation -- --value-tabs` 试用插入、重排和关闭标签。

当焦点在 `TabWidget` 或 `SwipeView` 的页面内时，切页会恢复新页面上次聚焦的控件。没有记录时，焦点移到页内第一个可聚焦控件。模型页面按行身份记忆焦点，重排后仍能返回同一行的控件。点击 `TabWidget` 的标题后，方向键继续在标题间导航。按 Tab 进入内容区时，会优先回到该页上次聚焦的 Tab 停靠点。可在 `--model-tabs` 示例里聚焦页内的「详情」后按 F6 切页，或在 `--model-swipe` 中滑动、按方向键切页并用 F7 重排前两项。

`SwipeView.transition-duration` 以毫秒指定选中页改变后的滑动时长，默认为 `0`。启用后，拖动时当前页和相邻页跟随指针移动。达到切页距离后松手会从当前位置继续滑动。短距离快速甩动也可切到同方向的相邻页，停顿后以拖动距离判定，收手时急反向可以滑回当前页。取消手势也会滑回当前页。拖动时只准备当前方向的一张相邻页，选择、焦点和事件仍由当前页处理。过渡沿 `orientation` 运动，横排遵循 RTL。过渡期间旧页仅用作绘制快照，焦点和无障碍立即切到新页。按方向键再次切页时，会从已显示的动画帧接续。手势开始后若当前页或目标页被模型替换，或目标页因重排改变位置，本次手势取消。运行 `bazel run //examples/navigation:navigation -- --model-swipe` 可查看 180 毫秒的过渡。

iOS 的 `SwipeView` 支持从页内控件发起无障碍翻页。翻页遵循排列方向和 RTL，跳过隐藏或禁用的页面，触发同一个 `changed(index)` 回调。页内滚动区域能沿请求方向继续滚动时优先滚动内容。翻页完成后，应用把当前可用页的位置和页数发给 UIKit。

`SwipeView` 可以用一个 `for … : SwipePage` 从模型生成全部页面。和 `TabBar` 使用同一个模型、绑定同一个 `current-value` 后，两处的顺序和选择都随模型同步。建议把编辑器、表单等交互和局部状态封装在页内的组件中。

```xr
SwipeView {
    current-value <=> root.selected;
    for doc in root.documents : SwipePage {
        value: doc.id;
        DocumentEditor { title: doc.title; }
    }
}
```

首次访问一页时才创建该页内容。切到其他页后，已访问页的组件状态随模型行标识保留，因此同一个 `VecModel` 内的插入和重排会保持编辑状态。移除该行会结束其页面生命周期。用新模型替换整组数据时，`current-value` 仍能找到同业务标识的页面，局部状态按新行初始化。运行 `bazel run //examples/navigation:navigation -- --model-swipe` 体验。

`TabButton.checked` 可直接驱动自定义标签内容，包括通过 `for` 创建的标签。`Tab.active` 和 `SwipePage.active` 可用于绑定或回调，表示所在控件实际使用的页面。`TabBar` 和 `SwipeView` 回退到首个可用项时，读取结果会随之变化，原来的索引绑定值仍保留。运行 `bazel run //examples/slots -- --tab-content` 查看标签内容示例。

横向 `TabBar` 和 `TabWidget` 继承 `layout-direction`。RTL 下第一项在右侧，左箭头切到下一项，溢出的标签可向左浏览。竖排 `TabWidget` 的标签栏位于起始侧，RTL 时在右侧，上下键仍按从上到下的顺序导航。与标签共用索引的横向 `SwipeView` 同样支持 RTL，按左箭头或向右拖动可切到下一页。运行 `bazel run //examples/navigation -- --rtl-tabs` 切换体验。

横向 `PageIndicator` 的页码顺序和点击区域也随方向镜像，竖向从上到下。普通复选框、单选按钮和开关把指示器放在起始侧，对应的列表项控件把它放在末尾侧。替换部件沿用同样的摆放规则。开关的打开状态、横向单选组的方向键和进度条的填充也遵循布局方向。运行 `bazel run //examples/controls -- --rtl-feedback` 可在同一窗口切换验证。

## 日期和时间

`Calendar` 的 `date` 和 `display-month` 使用 `Date { year: 2024, month: 2, day: 29 }`，可分别双向绑定。点击日期或按方向键选择，Home / End 跳到该月可用的首尾日期。翻月按钮和 Page Up / Page Down 更新显示月份并触发 `month-changed(month)`。选中日期变化时触发 `changed(date)`。

日历使用公历，年份范围为 1–9999，`minimum-date` 和 `maximum-date` 可进一步限制选择。默认日期是 `1970-01-01`，应用可传入当天或业务日期。`week-start` 按周一为 1 到周日为 7 编号，`weekday-labels` 接收从周一起排列的自定义星期名。

`Calendar`、`MonthGrid` 和 `DatePickerPopup` 可绑定 `disabled-weekdays: [6, 7];`，例如预约表单要排除周末。编号从周一 1 到周日 7，与 `week-start` 的显示顺序无关。重复或越界的编号会忽略，空数组允许所有星期。鼠标、键盘和无障碍都遵守同一规则。方向键跳过禁用日期，Home 和 End 只在当前月内寻找可选日期。规则变化会保留应用已经设置的 `date`，日期弹层会等到草稿可以选择时才允许确认。当当前显示的草稿变为禁用时，方向键从该日期沿指定方向寻找可用日。

三个日期控件也接受 `disabled-dates`，类型为 `[Date]`。它与 `disabled-weekdays`、`minimum-date`、`maximum-date` 同时生效。重复或无效的日期忽略，选择规则随共享模型的修改更新。运行 `bazel run //examples/controls:controls -- --calendar`，点击「闰日停约」，再从 2 月 28 日向右移动，可直接跳到 3 月 1 日。

`DateField` 是可导入的 `.xr` 组件，定义在 `examples/form/widgets.xr`。它使用 `initial-date` 初始化，分别提供只读的 `date`、`draft-text` 和 `acceptable-input`。输入支持 `YYYY-MM-DD` 与数字粘贴。Enter 提交合法日期，Escape 恢复到已提交日期。日历按钮打开 `DatePickerPopup`，确认和键盘提交共用 `committed(date)`。应用可调用 `set-date(date)` 同步已提交日期和草稿。两种输入路径共享上下限、禁用星期和停约日期。运行 `bazel run //examples/form:form -- --date-field` 体验。

`DatePickerPopup` 通过 `open` 打开，复用 `Calendar` 的日期范围、星期设置和键盘导航。打开时从 `date` 读取独立草稿，翻月只改变视图。确认时按当前范围调整草稿，写回双向绑定并触发 `accepted(date)`，取消或按 Escape 触发 `canceled()`。`title`、`accept-text` 和 `cancel-text` 可自定义。运行 `bazel run //examples/controls:controls -- --calendar` 体验内联日历和日期弹层。

`MonthGrid` 提供六行日期网格，可与 `DayOfWeekRow` 和 `WeekNumberColumn` 组合。让星期行和月份网格同宽、周数列和月份网格同高，并为三个控件绑定同一 `week-start`。周数列和月份网格共享 `display-month`。周数采用 ISO 8601，每行按该行的星期四归属，跨年时可显示上一年最后一周或下一年第一周。

日历、日期弹层、月份格和星期标题继承 `layout-direction`。RTL 下每周首日在右侧，按左键选择下一天，上个月按钮在右侧。自定义 `DayCell` 随对应日期移动，周数列可和月份格一起放在继承方向的 `GridLayout` 中。运行 `bazel run //examples/controls -- --rtl-calendar` 切换体验。

月份网格中的 `date` 和 `display-month` 可分别双向绑定。点击可用的相邻月份日期只更新选择，Page Up / Page Down 翻月。方向键改变日期，超出可见网格时随日期翻月。点击、Enter 或空格触发 `activated(date)`，包括再次触发同一日期。应用可传入 `today` 标记当天。运行 `bazel run //examples/controls:controls -- --month-grid` 体验自定义标题和周数列。

`MonthGrid { fixed-month: true; }` 只允许选择显示月份中的日期，输入操作保持 `display-month` 的绑定，位于虚拟 `ListView` 中时，Page Up 和 Page Down 将焦点移到上一行或下一行的月份格，保留当前选中日期。将 `Dates.months(from, to)` 绑定给虚拟 `ListView`，每一行用月份格显示一个月，即可共享日期选择、禁用日和停约列表。用 `Dates.month-index` 配合 `position-view-at-index` 跳到指定月。运行 `bazel run //examples/controls:controls -- --calendar-browser` 可以滚动 1,200 个月，或直接跳到 2080 年 2 月。

`Calendar` 和 `MonthGrid` 可绑定 `highlight-start` 和 `highlight-end`，以闭区间显示日期。两个端点都是 `Date`，可以从后往前指定。只有端点都有效才显示区间。区间覆盖的停约日仍保留禁用状态，固定月份格只标出本月日期。辅助技术可以读取区间起点、终点或内部日期。

在多月日历示例中点击「区间选择」，点击或按 Enter 选中起点，再激活另一个日期确认终点。选择终点期间，方向键移动会更新预览。示例把两个端点和选择阶段保存在同一条记录中。

在 `MonthGrid` 内声明一个匿名 `DayCell`，即可自定义每个日期的视觉内容。模板可读取 `cell-date`、零基 `cell-index`、`cell-weekday`、`cell-week`、`cell-week-year`、`cell-selected`、`cell-today`、`cell-in-month` 、`cell-enabled`、`cell-in-range`、`cell-range-start` 和 `cell-range-end`。`cell-week` 和 `cell-week-year` 表示该日期的 ISO 周数和周所属年份。网格为每个单元分配并裁剪内容区域，同时处理选择、键盘导航和无障碍日期动作。模板的 `accessible-description` 可补充日程信息。每个日期独立缓存模板绑定。运行 `bazel run //examples/controls:controls -- --calendar-agenda` 查看带日程标签的月份网格。

`TimePicker` 和 `TimePickerPopup` 使用 `Time { hour: 23, minute: 58, second: 41 }`。字段省略时默认为零。构造时小时限制到 0–23，分秒限制到 0–59。`time` 可双向绑定，内联滚轮改值后触发 `changed(time)`。默认显示 24 小时制，设置 `use-24-hour-format: false;` 可显示 12 小时制和 AM/PM。`show-seconds: true;` 显示秒，隐藏时保留原值。滚轮支持鼠标、键盘和辅助技术操作。

`TimePickerPopup` 通过 `open` 打开，打开时读取一次 `time` 作为草稿。确认后写回并触发 `accepted(time)`，取消或按 Escape 触发 `canceled()`。可用 `title`、`accept-text` 和 `cancel-text` 自定义文字。`minimum-time` 和 `maximum-time` 可限制弹层的确认范围。滚轮可以经过超出范围的中间值，小于下限或大于上限时禁止确认。运行 `bazel run //examples/controls:controls -- --time` 体验。

在 iOS 上，VoiceOver 读取滑块、步进器和时间滚轮的当前值，只开放会实际改变值的调节方向。双手柄滑块分别调节上下限，小数步进器按界面的小数位数和分隔符读值，固定或只读值仍可读取。时间滚轮拖动预览时保留已提交的当前值，落位后更新。

日期和时间弹层会根据可用尺寸排布内容。确认和取消按钮为触控预留至少 44 个逻辑像素的宽高。横屏高度紧张且宽度足够时，按钮移到选择区旁边，给日历格和时间滚轮保留更多高度。窄屏通常把按钮放在选择区下方，较长的按钮文字可以分行排列。iOS 的无障碍树会公开单个日期、翻月和操作按钮，并把时间滚轮公开为可调节控件。见 [Pocket Tasks 的巡检页](../examples/ios/README.md)。

iOS 用户增大系统字号时，日历会尽量加高日期格，紧凑窗口仍显示整个月份网格。时间滚轮会增大行高，空间不足时减少同屏行数。拖动和键盘翻页按重新布局后的行高和行数工作。已打开的弹层会随系统设置重新布局并保留当前草稿。

iOS 日期弹层翻月时，无障碍焦点会跟到当前月的可选日期。翻月本身保留草稿，按回车可选择当前焦点日期。当月没有可选日期时，焦点落到可用的翻月按钮。如果翻月也不可用，焦点落到取消按钮。关闭弹层后，焦点回到打开它的控件。

`TimeField` 定义在 `examples/form/widgets.xr`，可直接导入。它组合文本编辑器和时间弹层，支持输入或粘贴纯数字并用 Enter 提交，Escape 恢复已提交时间。`show-seconds` 同时控制文本和滚轮是否显示秒，隐藏时提交分钟会保留原秒数。文本使用 24 小时制，`use-24-hour-format` 可单独调整弹层。可用 `set-time(time)` 同步程序写入，两种输入路径共用 `minimum-time`、`maximum-time` 和 `committed(time)` 回调。运行 `bazel run //examples/form:form -- --time-field` 体验。

`ColorPickerPopup` 支持 RGBA 滑条、0–255 数值输入和十六进制编辑。`show-color-plane: true;` 打开 HSV 色板和色相滑条，鼠标或触摸可拖动色板，方向键分别调整饱和度和明度。所有输入共享草稿并实时更新棋盘格上的预览。十六进制输入接受六位 RGB，也可以在 `show-alpha: true;` 时输入八位 RGBA。六位输入和隐藏透明度控件都会保留原有 alpha。格式无效时保留上一次有效预览，修正后可确认。

`open` 和 `color` 支持双向绑定。确认时写回颜色并触发 `accepted(color)`，取消或按 Escape 丢弃草稿并触发 `canceled()`。运行 `bazel run //examples/controls:controls -- --color` 体验带色板的版本。

`FontPickerPopup` 通过 `open` 打开，使用当前窗口渲染器的系统字体和内嵌字体。可选择字体族、逻辑像素字号、1–1000 的字重以及斜体，`preview-text` 自定义预览内容。`font` 支持双向绑定，确认时一次写回整个 `Font` 并触发 `accepted(font)`，取消或按 Escape 丢弃草稿并触发 `canceled()`。用 `Text { font: root.selected; }` 应用选择，或将窗口的 `default-font` 绑定到同一值。单独设置的 `font-family`、`font-size`、`font-weight` 和 `font-italic` 覆盖对应字段。构造示例为 `Font { family: "sans-serif", size: 16px, weight: 400, italic: false }`。运行 `bazel run //examples/controls:controls -- --font-picker` 体验。

颜色和字体弹层在受限高度下滚动编辑内容，确认与取消按钮留在可见操作区，按钮至少为 44×44 逻辑像素。Android 宿主将输入法实际占用区域纳入窗口布局，编辑期间仍可访问操作区。[Android 颜色与字体回归](../examples/android/README.md#颜色与字体回归) 提供手机示例、像素验证和 AOT／解释器对照命令。

Android 的基础编辑快捷键沿用共享编辑器语义，包括全选、复制、剪切、粘贴、撤销／重做和 Shift 选字。平台会在失焦时清除修饰键状态，编辑器只读时仍可复制。[Android 键盘回归](../examples/android/README.md#键盘快捷键回归) 检查编辑控件和独立 Shortcut 的实际按键路径。

在组件根部声明命名的 `FileDialog`，从回调调用 `picker.open()`。`mode` 可取 `open-file`、`open-files`、`save-file`、`select-folder` 或 `select-folders`。可设置 `directory`、`file-name` 和 `name-filters`，扩展名过滤用 `"Text files (*.txt *.md)"` 这样的形式。宿主异步显示系统面板，并在 UI 线程触发 `accepted([string])`、`canceled()` 或 `failed(string)`。应用收到 UTF-8 本地路径后自行读写文件。同一窗口串行显示面板，同一个 `FileDialog` 的请求未结束时会合并重复调用。发起组件离开活动场景后，宿主丢弃该请求的结果。运行 `bazel run //examples/form:form -- --files` 体验。

`FileDialog.selected-files` 和 `selected-file` 可绑定到界面，分别提供上次确认的全部路径和第一条路径。确认结果在 `accepted` 执行前提交。取消、失败和弃用回执保留上一次结果。`FileDialog` 与 `MessageDialog` 都提供只读的 `pending`，排队和显示面板期间为真，结果回调执行前恢复为假。

Android 宿主通过 `XRackActivity` 接入系统文档选择器，支持 `open-file`、`open-files` 和扩展名过滤。选中文档后，宿主在后台复制到应用私有缓存，再把可读取的本地路径交给 `accepted`。应用可自行保存需要长期保留的内容。Rust 宿主还可通过 `DesktopWindow.export_file()` 导出现有文件，通过 `select_directory()` 获取 `AndroidDirectory`，在工作线程列举和读取授权目录中的文档。DSL 的 `FileDialog.save-file`、`select-folder` 和 `select-folders` 仍待接入。构建和设备回归命令见 [Android 示例](../examples/android/README.md)。

iOS 的 `open-file`、`open-files`、`select-folder` 和 `select-folders` 打开系统 Files 选择器。扩展名过滤和起始目录由系统处理，界面标题由 Files 提供。使用 `select-folders` 时，可继续添加目录，确认后再导入整批。`accepted` 返回沙盒副本的普通路径，保留目录中的子目录与文件。目录副本随窗口关闭清理，长期使用的内容可移到应用管理的位置。若其中一个目录复制失败，整个批次触发 `failed`。[iOS 目录导入示例](../examples/ios/README.md#声明式目录导入)

iOS 的 `save-file` 仍会触发 `failed`。导出现有文件可用 Rust 宿主的 `DesktopWindow::export_file()`。

`MessageDialog` 也通过 `open()` 打开系统对话框，可设置 `title`、`text`、`informative-text` 和 `level`。`buttons` 可取 `ok`、`ok-cancel`、`yes-no` 或 `yes-no-cancel`。确认和拒绝分别触发 `accepted()` 与 `rejected()`，随后 `finished(StandardButtonKind)` 报告具体结果。系统关闭或取消返回 `cancel`，宿主检测到的打开错误通过 `failed(string)` 返回。文件面板与消息对话框按请求顺序共用窗口的异步队列。运行 `bazel run //examples/form:form -- --message` 体验。

iOS 使用 UIKit 系统提示样式显示 `MessageDialog`，四种按钮组合都可用。应用可在 `Localizable.strings` 中翻译 `OK`、`Yes`、`No` 和 `Cancel`。iOS 提示样式统一，`level` 不改变提示外观。

Android 使用系统 `AlertDialog`，支持四种按钮组合。点击按钮后在 XRack UI 线程交付对应的 `StandardButtonKind`，返回键和点击外部区域按 Cancel 处理。`level` 沿用同一系统提示样式。

Android 的 Rust 宿主还可在 `spawn_local` 中调用 `DesktopWindow.share_text(text)` 打开系统分享面板。返回 `Ok(true)` 表示系统确认用户已选择目标应用，`Ok(false)` 表示关闭分享面板，`Err` 报告打开失败。调用结束后可继续使用原窗口和组件状态。接收应用后续的发送或保存由它自身处理，设备回归用本地测试接收应用校验完整文本与 `text/plain` 类型。命令见 [Android 示例](../examples/android/README.md)。

Android 的 `XRackActivity` 为可编辑文本控件提供 `InputConnection`，把预编辑与已提交文本分别传给 XRack。预编辑只影响当前编辑器的显示，提交后才更新绑定属性。焦点或控件身份变化会取消旧会话，延迟返回的输入回执按会话身份丢弃。输入法使用的 UTF-16 选区会转换为 UTF-8 字节边界，按码点删除可完整移除 emoji。设备回归通过测试输入法覆盖这些操作，命令见 [Android 示例](../examples/android/README.md)。

iOS 通过 UIKit 的无障碍取消动作关闭当前菜单、下拉选项或弹层。每次只关闭最上层，遵守 `close-on-escape: false;`。关闭后沿用弹层的焦点还原，日期和时间弹层取消时保留打开前的值。

Android 系统返回键优先关闭当前菜单、下拉框、弹层或对话框，然后处理页面栈。`close-on-escape: false;` 的弹层会保留，按键松开和长按重复事件不会继续关闭下一层。日期和时间弹层通过返回取消时会丢弃未确认的草稿。`//examples/android:picker_ui_test` 覆盖日期、时间和旋转过程，命令见 [Android 日期与时间回归](../examples/android/README.md#日期与时间回归)。

Android 页面返回优先处理焦点所在的栈。焦点在页外时，会查找唯一可返回的活动 `StackView`，嵌套时优先内层。多个独立栈同时可返回且没有焦点归属时保持页面。查找只使用已准备好的可见场景，不展开虚拟列表的离屏行；禁用、隐藏和离屏的栈不会成为默认候选。[Android 导航回归](../examples/android/README.md#页面导航与触摸回归) 覆盖关闭全局抽屉后继续返回页面的路径。

iOS 的 Rust 宿主可在 `spawn_local` 中调用 `DesktopWindow::share_text(text)`、`share_file(path)` 或 `export_file(path)`。前两个打开系统分享面板，`export_file` 通过 Files 保存现有文件的副本。传文件时使用绝对路径，并保留源文件直至异步调用结束。返回 `Ok(true)` 表示操作完成，`Ok(false)` 表示取消，`Err` 报告无法打开或执行失败。同一窗口已有系统面板时，请等它关闭再发起操作。窗口句柄来自 `Desktop::open()` 或能同时接收可用尺寸的 `Desktop::open_with_viewport()`。用法见 [Pocket Tasks iOS 示例](../examples/ios/README.md)。

iOS 的 Rust 宿主也可以在 `spawn_local` 中调用 `DesktopWindow::select_directory().await`。选择成功返回 `IosDirectory`，取消返回 `Ok(None)`。把句柄移到工作线程后，用 `with_read_access("", |path| ...)` 读取目录列表，或传入相对路径读取子项。系统授权和经过协调的文件路径在回调期间有效，应用应在回调结束前完成读取。

`SplitView` 把直接子元素排成横向或纵向面板，支持嵌套。面板沿分栏方向使用 `min-width`、`preferred-width`、`max-width` 或对应的高度属性。拖动分隔线只调整相邻的可见面板，并在当前实例中保存比例。用 Tab 聚焦分隔线后，可用方向键步进，Home 和 End 到达两个边界。`handle-size` 和 `handle-color` 设置分隔线，`resized()` 在比例更新后触发。隐藏面板不占空间，再显示时会恢复其比例。

iOS 会把仍可移动的分隔线公开为可调节控件。无障碍增减操作以 8 个逻辑像素为一步，沿用触摸拖动和键盘步进的尺寸限制。空间不足、分隔线已无法移动时，只公开当前尺寸。[iOS 分栏示例](../examples/ios/README.md#ipad-分栏示例)复用桌面示例的 `.xr`。

横向分栏继承 `layout-direction`，RTL 下第一个面板在右侧。拖动和方向键按屏幕方向移动分隔条，Home、End 和无障碍数值对应声明顺序中前一个面板的尺寸。切换方向后可继续使用已保存的比例或侧栏尺寸。运行 `bazel run //examples/split -- --rtl-split` 切换验证。

`SplitView.save-state()` 返回可持久化的布局字符串，`restore-state(state)` 在回调中恢复比例。命名面板按 ID 匹配，匿名面板按声明位置匹配。恢复会校验版本、面板集合和比例，再按当前尺寸约束布局。Rust 可用 `save_split_state(id)` 和 `restore_split_state(id, state)` 读写状态，后者返回是否接受了字符串。保存数据由应用负责。运行 `bazel run //examples/split:split`，用「保存布局」和「恢复布局」体验。

[Android 分栏布局回归](../examples/android/README.md#分栏布局回归) 检查宽于可见分隔线的触摸热区、两层分栏的尺寸约束、RTL 拖动、禁用以及保存恢复。

`handle-hit-size` 设置鼠标和触摸可抓取的宽度，纵向分栏时设置高度。命中尺寸默认等于可见分隔线的尺寸，可设得更大以便触摸操作。命中区域重叠时选最近的可拖动分隔线。`resizing` 是只读布尔属性，鼠标或触摸抓住分隔线时变为 `true`，释放、取消或分隔线失效时恢复为 `false`。键盘仍按步长调整。示例的 `--fill-pane` 模式用 `handle-color: self.resizing ? #ffbf69 : #81a6e1;` 显示拖动反馈。

原生辅助技术可聚焦可调整的分隔条，读取其方向、前侧面板尺寸及可调范围，按 8 个逻辑像素步进或设置具体尺寸。分隔条优先使用前侧面板的 `accessible-name`，例如给导航栏面板设置 `accessible-name: "导航"`。辅助技术的调整沿用面板尺寸约束和 `resized()` 回调。

在 `SplitView` 中声明一个匿名 `SplitHandle` 模板，可以为每一条可见分隔线绘制自己的内容。模板和它的视觉子元素可以读取 `handle-hovered`、`handle-pressed`、`handle-orientation` 和 `handle-index`。索引是分隔线前方的面板槽位，从 0 开始，隐藏面板后仍保留原槽位。分隔条由 `SplitView` 统一处理命中、拖动和键盘焦点。运行 `bazel run //examples/split:split -- --custom-handle` 查看两个分隔条各自的状态。

```xr
SplitView {
    handle-size: 8px;
    handle-hit-size: 28px;
    SplitHandle {
        background: handle-pressed ? #ffbf69 :
            handle-hovered ? #58c9c7 : #485e7e;
    }
    Rectangle { background: #26374e; }
    Rectangle { background: #304663; }
}
```

`SplitView { fill-pane: editor; }` 让指定的直接子面板吸收剩余空间。两侧面板使用首选尺寸，拖动后记住相邻面板的逻辑像素尺寸。窗口空间不足或填充面板触及尺寸限制时，布局会按当前约束调整。在受限布局中拖动会保存当时的可见尺寸，让其他侧栏留在原位。原填充面板隐藏时由最后一个可见面板填充。此模式用 `xrack-split/2` 保存像素尺寸，普通比例模式继续使用 `xrack-split/1`。运行 `bazel run //examples/split:split -- --fill-pane` 可体验。

`MenuBar` 和 `ContextMenuArea` 分别通过顶部菜单和右键打开 `Menu`。菜单支持嵌套子菜单、禁用项、分隔线和可勾选的 `MenuItem`。`activated()` 在选中可用菜单项时触发。方向键导航，Escape 逐层关闭。右键菜单也可用 Shift+F10 打开。

触屏上，按住 `ContextMenuArea` 约 500 毫秒会打开同一个菜单，松手不会触发底层控件的点击。手指先滑动时，滚动和拖拽仍由原来的控件处理。触摸菜单的基准行高为 44 个逻辑像素，菜单过长时可在面板内滑动浏览，不会带动底部列表。默认文本编辑菜单和显式 `ContextMenuArea` 共用长按手势，控件有自定义菜单时优先打开它。用 [Pocket Tasks](../examples/ios/README.md) 长按任务行体验。

触摸点按 `MenuBar` 打开的菜单及其子菜单也使用 44 个逻辑像素的基准行高。Android 导航示例的 **Open commands** 覆盖行高、共享命令和返回键关闭层级。[Android 示例](../examples/android/README.md#触摸菜单栏回归)

菜单继承 `layout-direction`。RTL 下菜单栏从右向左排列，弹层和文字按起始侧对齐，图标、勾选标记和快捷键栏随布局换边。左键进入子菜单，右键返回。子菜单优先向左展开，空间不足时换边。运行 `bazel run //examples/menu -- --rtl-menu` 切换体验。

在 iOS 上，具有上下文菜单的控件会在 VoiceOver 的「操作」转子中提供 **Show context menu**。打开后可以逐项选择菜单内容，从当前菜单项发起无障碍翻页，或使用系统的无障碍取消动作返回。子菜单单独翻页，翻页后 UIKit 收到当前可见范围。

iOS 和 iPadOS 的菜单项会随系统文本大小调整字号及点击区域。菜单在窗口安全区内滚动，横向空间不足时显示省略号，VoiceOver 仍可读取完整名称。

单行、多行、搜索框、可编辑下拉框、数字输入框和正在编辑的表格或树单元格提供默认编辑菜单。右键、Shift+F10 或辅助技术均可打开。菜单与键盘共用选区、剪贴板和撤销记录。只读框可复制，密码框禁用复制和剪切。数字框的撤销和重做作用于草稿，提交时再更新绑定的数值。`ContextMenuArea` 优先，`context-menu-enabled: false` 可关闭默认菜单。菜单打开后，外部修改当前文本或选区会使旧操作失效。运行 `bazel run //examples/form:form -- --editor-menus` 体验。

`MenuItem` 的 `shortcut: "Primary+Shift+E";` 与点击共用 `activated()`。菜单在正文和快捷键都能完整放下时显示组合键。`Primary` 在 macOS 上表示 Command，在其他平台表示 Ctrl。也可以组合 `Ctrl`、`Alt`、`Shift` 和 `Super`，主键支持英文字母、数字和 F1 到 F12。字母和数字需要 Ctrl、Alt、Super 或 Primary 中的一个修饰键。无效字符串不会注册快捷键，可将绑定设为空字符串清除快捷键。

菜单栏快捷键在当前窗口或最上层模态区域内生效。打开右键菜单时，只处理该菜单内的快捷键。隐藏和禁用项不参与匹配，同一作用域内的重复组合键不会触发命令。文本编辑保留选择、剪贴板和撤销重做快捷键。运行 `bazel run //examples/menu:menu` 可体验。

`Shortcut` 可声明在布局或对话框内，不占空间。用 `sequence: "Primary+K, Primary+P";` 声明最多四段组合键，相邻两段须在一秒内输入。`sequences` 可提供多个别名，`focus-within: true;` 将范围缩到父元素的焦点子树，`auto-repeat: true;` 允许单段快捷键随按住重复触发。不同命令匹配同一序列时，独立 `Shortcut` 会收到 `activated-ambiguously()`。短序列同时是长序列的前缀时，短序列报告冲突，长序列可以继续输入。用 `action` 复用命令时，未指定序列会采用命令的 `shortcut`，同一命令的多个入口只执行一次。运行 `bazel run //examples/menu:menu -- --shortcuts` 体验。

iOS 和 iPadOS 上，外接键盘的 F1–F12 可触发 `Shortcut`，并进入焦点作用域的按下和释放回调。例如，编辑草稿时也可使用 `F2` 和 `Primary+F12` 执行命令。Application 键和 Shift+F10 可打开当前焦点控件的上下文菜单。[iOS 功能键示例](../examples/ios/README.md#外接键盘功能键)

在组件根部声明命名的 `Action` 后，`Button`、`ToolButton`、`MenuItem` 和虚拟列表中的 `ItemDelegate` 可以通过 `action: command-id;` 共享命令。命令集中声明 `text`、`icon`、`shortcut`、`enabled`、`checkable`、`checked` 和 `activated()`。可勾选命令先切换 `checked` 再执行回调，也可用 `checked <=> root.some-flag;` 关联组件属性。控件可以覆盖显示文本和图标，也可额外限制自身的 `enabled`。同一命令的多个控件共用一个快捷键，按钮单独展示时也可以触发命令。每个组件实例独立持有命令状态。

`ActionGroup` 可以包含命名的 `Action`，也可以让组件根部的命令用 `group: group-id;` 加入。互斥组默认只勾选 `current-value` 对应的命令。命令的 `value` 在组内唯一，省略时用命令 ID。用 `current-value <=> root.mode;` 和业务状态双向绑定，也可在一个成员上用 `checked: true;` 初始化。再次触发已勾选的命令会继续执行回调并保持选中。组内的普通命令可设 `checkable: false;`。

设置 `exclusive: false;` 后，每个成员独立维护 `checked`。两种组都支持 `enabled`，禁用后菜单、工具栏和快捷键统一停止触发。命令自己的 `activated()` 在状态更新后先执行，随后调用组的 `activated(value)`。运行 `bazel run //examples/menu:menu -- --action-groups` 体验。

`ToolBar` 默认横向排列子项，可用 `orientation: vertical;` 改为纵排。设置 `wrap: true;` 后可随可用空间换行或换列。工具按钮支持独立的 `clicked()`，也可绑定命令以同步勾选状态。`Button` 和 `ToolButton` 可用 `icon-size` 和 `icon-only` 控制图标显示。纯图标模式在没有图标时回退到文字。`icon` 使用已声明的 PNG 或 SVG 资源。运行 `bazel run //examples/menu:menu` 可体验共享图标和工具栏换行。

`Button`、`ToolButton`、`RoundButton` 和 `DelayButton` 可用 `display: ButtonDisplay.text-under-icon;` 将图标放在文字上方，也支持 `text-beside-icon`、`text-only` 和 `icon-only`。图标和文字并排时，图标位于逻辑起始侧。用 `spacing` 调整二者间距，用 `colorize-icon: true;` 让图标跟随文字颜色。显式 `display` 优先于旧的 `icon-only` 属性，纯图标模式在缺少图标时显示文字。自定义 `ButtonContent` 接管内容布局，按钮仍负责焦点和激活。运行 `bazel run //examples/menu:menu -- --button-display` 切换排列方向并体验。

在按钮上显式设置 `primary: true;` 使用主题强调色，设置 `primary: false;` 使用中性表面。普通按钮省略该属性时沿用默认配色。`flat: true;` 隐去静止按钮的默认表面，悬停、按下或选中时仍显示反馈。可用显式 `background` 自定义可用按钮的填充。普通按钮的这些样式可随绑定更新，按钮大小和激活规则与样式无关。运行 `bazel run //examples/menu:menu -- --button-appearance` 查看示例。

`RoundButton` 沿用按钮的文字、图标、键盘激活和 `Action` 绑定。默认半径为宽高中较短边的一半，正方形尺寸会绘制为圆形，也可用 `radius` 指定半径。绘制和指针命中共用圆角几何，透明角落会把点击交给下层。`ToolSeparator` 默认绘制竖线，可在纵向工具栏里设 `orientation: horizontal`。`stroke-width`、`padding` 和 `color` 分别控制线宽、端部留白和颜色。分隔线参与布局，点击可以穿过它。运行 `bazel run //examples/menu:menu -- --round-tools` 可试用。

在控件或视觉元素内声明 `Tooltip { text: "操作说明"; }`，可在悬停或通过 Tab 聚焦后显示提示。每个元素可附加一个 `Tooltip`，内容可用纯文本或单个自定义布局。`delay` 默认 600 毫秒，`timeout` 默认持续到离开、失焦或发生输入。提示浮在窗口内，靠近边缘时会避让，点击会穿过浮层。菜单和模态作用域会限制提示显示。

触摸设备也可长按显示提示，等待时间取 `delay` 和 500 毫秒中的较长者。长按显示后松开不会激活控件。提示保留到下一次触摸、其他输入或 `timeout` 到期。文本选择、上下文菜单和专用长按手势优先。计时完成前滑动仍可滚动容器。[iOS 长按示例](../examples/ios/README.md#长按提示)

iOS 会把 `visible` 为真的纯文本 `Tooltip` 提供给 VoiceOver，作为所属控件的提示。文本绑定更新时，提示也随之更新。控件的 `accessible-description` 和框架已有的操作提示优先。自定义布局的提示可在所属控件上用 `accessible-description` 提供朗读内容。

## 图像和路径

`Image` 的 `source: @image-url("assets/icon.svg");` 引用相对于 `.xr` 的本地 PNG 或 SVG。图片以 `!xr.image` 通过 MLIR，编译器检查资源后把编码字节嵌入 Rust。支持 `image-fit: fill | contain | cover | preserve`。布局内默认 `contain`，其他位置默认 `fill`。SVG 支持独立的矢量图形，由 resvg 按显示尺寸和 DPI 栅格化。SVG 文本和外链资源暂未支持。

`source-clip-x`、`source-clip-y`、`source-clip-width` 和 `source-clip-height` 以源像素选择图块。未指定的宽高延伸到源图边缘，超出源图的部分裁掉。裁剪后的宽高参与隐式尺寸和比例计算，`image` 值的 `.width`、`.height` 仍返回完整资源尺寸。`horizontal-alignment` 和 `vertical-alignment` 决定图像在未平铺轴上的位置，`image-rendering: pixelated;` 使用最近邻采样。普通图像和 GIF 帧也可按轴使用 `horizontal-tile-mode` 和 `vertical-tile-mode`。SVG 直接栅格化裁剪区域，PNG 图块使用独立纹理区域以隔离相邻像素。运行 `bazel run //examples/media:media -- --atlas`。

`Image` 和 `AnimatedImage` 的 `colorize` 接受纯色、线性或径向渐变画刷，并以图像透明度作为遮罩。完全透明的画刷恢复原图。渐变以完整源图坐标计算，裁剪后保留对应的颜色区间，平铺重复该区间，九宫格的各块沿用同一画刷。纯色使用缓存的单通道纹理，渐变在 GPU 上合成。运行 `bazel run //examples/media:media -- --colorize`。

用 `@image-url("assets/frame.svg", nine-slice(4 6 8 10))` 创建九宫格图像。边距按上、右、下、左顺序指定源像素数，也可以给一个统一值，或给纵向和横向两个值。编译器会检查图像是否保留了正尺寸的中心。九宫格填满 `Image`，四角保留逻辑尺寸，空间不足时同轴边距按比例收缩。`horizontal-tile-mode` 和 `vertical-tile-mode` 独立支持 `stretch`、`repeat` 和 `round`，分别拉伸、按原尺寸平铺并裁剪末片、缩放为整数个完整的平铺单元。Rust 可以用 `Image::with_nine_slice` 为已有资源设置边距。运行时图片可用 `Image::Loaded` 提供，图片值通过 `clone()` 共享底层数据。运行 `bazel run //examples/media:media -- --nine-slice` 查看对照。

`AnimatedImage` 可播放通过 `@image-url` 内嵌的本地 GIF。编译器合成差分帧并保留帧时长和文件循环次数，普通 `Image` 显示首帧。用 `playing` 开始或停止，停止会回到首帧。用 `paused` 暂停，或用 `speed` 调整播放速率。`loops: 0;` 沿用文件设置，`-1` 持续循环，正数表示总播放轮数。完成有限循环后保留末帧并触发 `finished()`。只读的 `current-frame` 和 `frame-count` 在窗口同步播放状态后可用于绑定。仍在场景中但不可见的控件冻结播放时钟，重新可见时继续。运行 `bazel run //examples/media:media -- --animated` 体验。

Android 导航示例的 **Open images** 通过设备截图核对 PNG 图集的裁剪和尺寸适配，以及 GIF 首末帧和完成回调。设备回归目标为 `//examples/android:images_ui_test`。[Android 示例](../examples/android/README.md#图像和-gif-回归)

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

### SoundEffect

`SoundEffect` 在组件根部以具名的非视觉对象声明。启用 Cargo 的 `media`
feature 或 Bazel 的 `--config=media` 后，它会在组件挂载时异步预加载本地
WAV。相对路径以声明所在的 `.xr` 文件为准。支持 8、16、24、32 位整数 PCM
和 32、64 位浮点 WAV。短音效最多 10 秒，文件最多 16 MiB。

```xr
confirmation := SoundEffect {
    source: "click.wav";
    volume: 0.35;
}
confirm := Button {
    text: "确认";
    clicked => { confirmation.play(); }
}
```

`volume` 的范围为 0 到 1，默认 1。`muted` 默认 false。`ready`、
`playing`、`status` 和 `error` 为只读。`status` 可为 `empty`、`loading`、
`ready`、`playing` 或 `failed`。加载中的 `play()` 会在音效就绪后触发一次播放。
回调中切换 `source` 后可以直接调用 `play()`，新文件加载完成后开始播放。
同一实例连按时从头重播，不同实例可以同时播放。
`loops` 默认 1，0 也播放一次，-1 持续循环。播放中修改它会更新剩余次数，
并从当前采样位置继续。只读的 `loops-remaining` 包含正在播放的这一轮，
无限循环时为 -1。`stop()` 会停止当前音效，也可取消预加载期间排队的 `play()`。

运行 `bazel run --config=media //examples/sound_effect`，也可以在 VS Code
打开 [示例](../examples/sound_effect/sound_effect.xr) 并启动实时预览。
点击按钮或在聚焦的预览面板按 F2 触发音效。勾选「循环播放」可持续播放，
点击停止按钮或按 F3 结束。

### MediaPlayer

`MediaPlayer` 接收本地文件或 HTTP(S) URL。相对路径以声明控件的 `.xr` 文件所在目录为准。`autoplay` 默认为 `false`，`volume` 范围是 0 到 1，`muted` 可静音。`playback-rate` 支持 0.25 到 4 倍速，当前倍速会改变音高。`loops` 默认为 1，设为 -1 可循环播放可定位的媒体。

使用 `xrackc --emit mlir -o` 导出时，编译器会记录媒体声明相对于输出文件的位置。移动 MLIR 和媒体文件时，保持二者的相对目录关系即可。

用 `play()`、`pause()`、`stop()` 和 `seek(seconds)` 控制播放。`position`、`duration` 以秒为单位。可绑定的 `status` 值有 `empty`、`loading`、`seeking`、`buffering`、`paused`、`playing`、`stopped`、`ended` 和 `failed`，`buffer-progress` 在 0 到 1 之间。拖动进度时可用 `Slider { live: false; }`，根据 `seekable` 启禁滑块，绑定 `minimum: player.seek-start;` 和 `maximum: player.seek-end;`，在 `changed(seconds)` 中定位。

`has-audio`、`has-video`、`media-title` 和 `audio-tracks` 提供基本元数据。`audio-track: -1;` 自动选轨，非负整数按音轨顺序从 0 开始选轨。播放支持纯音频。`subtitle-tracks` 提供内嵌字幕轨道数量，`subtitle-track` 可选取其中的文本轨，-1 关闭内嵌字幕。`subtitle-file` 可覆盖为本地 UTF-8 SRT 或 WebVTT 文件，`subtitle` 可绑定到 `Label`。音频设备或字幕异常可分别读 `audio-error` 和 `subtitle-error`。

```sh
bazel run --config=media //examples/video:video
bazel run --config=media //examples/video:video -- /absolute/path/to/video.mp4
bazel build --config=media //crates/xrack-preview
bazel test --config=media //crates/xrack-media:media_test //tests:media_player_test //tests:gpu_test
```

媒体配置包含 MP4、Matroska/WebM、MP3、WAV、Ogg、FLAC 和 HLS 的基础播放链路。HTTPS 校验证书和主机名。如果运行环境缺少默认 CA 信任链，可以通过 `SSL_CERT_FILE` 指定 PEM 证书文件。

HLS 点播和直播都支持动态码率。多码率源从低码率开始，在缓冲稳定时升档，在音频供给不足时降档。`bitrate` 展示当前选择。视频通过 RGBA 帧更新复用的 GPU 纹理。解释器和 Rust AOT 共用该运行时。

`network-bandwidth` 可用于调试 HTTP(S) HLS。它以 bit/s 为单位限制压缩数据送入播放缓冲的速度，设为 `0` 则解除限制。修改它会保留当前媒体会话。

直播时 `live` 为真，`seek-start` 和 `seek-end` 随服务器保留的完整分片滑动，`position` 和回看范围使用同一条秒数时间线。定位会限制在当前可播放范围内。`go-live()` 跳到安全直播位置并开始播放，`at-live-edge` 表示已追上该位置。点播的回看范围是 0 到 `duration`，直播的 `duration` 为 0。

## 焦点作用域

`FocusScope` 可包裹复合控件。用 `initial-focus: editor;` 指定首次进入时优先选择的后代。作用域默认记住最后聚焦的有效子控件，`restore-focus: false;` 可让每次进入都采用初始目标。Tab 和 Shift+Tab 可以离开作用域。`focus-on-tab-navigation` 和 `focus-on-click` 控制作用域本身是否接管这两种入口，子控件保留各自的输入行为。

iOS 将有可访问子控件的 `FocusScope` 作为 UIKit 无障碍容器。辅助技术可逐个聚焦、编辑或激活其中的控件。

事件处理器可以调用 `scope.focus()` 或 `scope.clear-focus()`。窗口在回调结束后用当前布局处理命令。隐藏、禁用或被模态弹层挡住的目标不会获得焦点。每个组件实例保存独立的请求身份和焦点记录。运行 `bazel run //examples/layout:layout -- --focus-scopes`，在两个编辑区域之间跳转，按 Escape 清除当前区域的焦点。

`has-focus` 表示当前控件直接持有键盘焦点。`FocusScope` 的 `focus-within` 在作用域或其子控件持有焦点时为真。同一区域内切换子控件时，依赖 `focus-within` 的样式保持有效。窗口失活会清除这两个读值，重新激活后恢复有效的逻辑焦点。`focus-gained(reason)`、`focus-lost(reason)`、`focus-changed-event(reason)` 和 `focus-within-changed(focused, reason)` 可读取有类型的 `FocusReason`。回调运行时，失焦和得焦两侧的读值均已发布。

`capture-key-pressed(event)` 和 `capture-key-released(event)` 沿当前焦点路径从外到内运行。窗口和控件处理后，剩余按键通过 `key-pressed(event)` 和 `key-released(event)` 从内到外传递。返回 `EventResult.accept` 停止传递，返回 `EventResult.reject` 则继续。`KeyEvent` 提供 `key`、`text`、`repeat` 和 `modifiers`。逻辑键名可用 `"F2"`、`"Escape"` 或 `"ArrowLeft"`，`modifiers.primary` 在 macOS 上对应 Command，其他桌面平台对应 Ctrl。

键盘路由以事件开始时的焦点为准。回调改变焦点后，新控件从下一个事件开始接收。模态弹层和菜单限制分发范围，IME 预编辑沿用原输入路径。运行 `bazel run //examples/layout:layout -- --key-routing`，聚焦输入框后比较 `x` 被拦截、`a` 正常输入和 `F2` 冒泡。

[Android 焦点作用域示例](../examples/android/README.md#焦点作用域与按键路由回归) 用嵌套录入区域演示 Tab 导航、按键捕获和冒泡，以及原输入框禁用后的焦点回退。

## 窗口内拖放

`DragArea` 通过 `data` 绑定字符串载荷，通过 `drag-key` 标记用途。鼠标或触摸移动 6 个逻辑像素后开始拖动并冻结载荷，短距离按放仍可点击子控件。`allow-copy`、`allow-move`、`allow-link` 声明源允许的操作。按 Ctrl 请求复制，按 Shift 请求移动，同时按两键请求链接。源可以用 `drag-finished(DragAction)` 根据最终结果更新模型。取消时结果为 `none`。

触摸设备可在 `DragArea` 设置 `touch-drag-delay: 420;`，单位为毫秒。手指需先保持在按下位置附近，再移动达到拖放阈值。等待结束前越过阈值会放弃本次拖放，让外层列表继续处理滚动。默认 `0`，负值也按 `0` 处理，鼠标仍按移动距离启动。iOS 的普通列表重排示例使用这个属性。

`ListView`、`StandardListView` 和 `GridView` 可设置 `drag-auto-scroll: true;`。在窗口内拖放中，指针停在可接收区域的列表上边缘或下边缘时，列表会持续滚动，并更新指针下的落点。虚拟列表按源行标识继续追踪移出视口的记录，松手时才提交到当前落点。源记录被移除后取消拖放。属性默认关闭。

虚拟列表根据记录标识识别同一个落点。记录换到新显示槽位时，原有悬停保持有效。拖离仍在模型中的记录时，即使它已经滚出视口，`exited()` 也会清理该记录的落点状态。成功释放时，目标仍通过 `dropped(...)` 完成操作并清理预览。

`DropArea` 用 `keys` 过滤拖动源，也可以用 `drop-action` 限制操作。嵌套目标优先匹配最深的可接收区域。`entered(string, DragAction)`、`position-changed(length, length)`、`exited()` 和 `dropped(string, DragAction)` 提供有类型的事件，位置坐标相对于目标区域。成功释放时先调用目标的 `dropped`，再通知源。拖动复用窗口的裁剪、弹层范围和虚拟行标识。设置任一区域的 `enabled: false;` 只停用该区域的拖放行为，子控件仍可交互。

[Android 应用内拖放回归](../examples/android/README.md#应用内拖放回归) 覆盖真实触点、拒绝与外层目标回退、复制和移动，以及应用切到后台时取消未提交的拖放。

运行 `bazel run //examples/form:form -- --drag`，把编号拖到右侧。移动后左侧编号消失，按 Ctrl 拖动则保留编号。当前载荷是窗口内的字符串，适用于卡片、条目和同窗口控件之间的操作。

macOS 上，`DropArea { accept-files: true; }` 也可接收 Finder 或其他应用拖入的本地文件。`file-extensions: ["png", "jpg"];` 可筛选扩展名，匹配忽略大小写，可带前导点。整个批次必须符合规则。`files-entered([string])` 和 `files-dropped([string])` 收到有序的 UTF-8 路径列表。外部文件操作按复制处理，应用按收到的路径自行读取文件。运行 `bazel run //examples/form:form -- --drop-files`，分别向图片区和文档区拖入一个或多个文件。

iOS 使用同一套 `DropArea` 接收系统文件拖放。拖动时可用 `position-changed` 和 `exited` 提供反馈。系统给出的扩展名用于预判，缺少扩展名时以实际文件为准。松手后，宿主异步复制整批文件并复查原目标。复制成功才依次触发 `files-entered` 和 `files-dropped`，并按原顺序提供沙箱副本的路径。窗口关闭时清理这些副本。应用需要长期保存的文件应复制到自己的存储目录。[iPad 文件拖放示例](../examples/ios/README.md#文件拖放)

如果 iOS 已接受释放，随后读取或保存文件失败，仍有效的原接收区域会收到一次 `files-failed(string)`。回调参数用于显示失败原因。文件批次按整体交付，应用可以保留上一次成功接收的内容。

## 声明式计时器

`Timer` 是组件根部的命名声明，`interval` 使用整数毫秒，默认 1000，非正数暂停调度。`running` 默认关闭，可双向绑定。默认触发一次并在 `triggered()` 前清除 `running`，设置 `repeat: true;` 可循环。`triggered-on-start: true;` 会在启动时额外触发一次。处理器可调用 `timer.start()`、`timer.stop()` 或 `timer.restart()`。

计时器随拥有它的组件进入或离开活动布局而启动或撤销。回调在 UI 线程运行，间隔变化会重新计时，重复计时器错过的周期合并为一次回调。运行 `bazel run //examples/controls:controls -- --timer` 体验倒计时、暂停和单次触发。

[Android 采样示例](../examples/android/README.md#计时与进度回归) 用计时器驱动进度条和圆形进度，并在等待报告时显示忙碌指示器。关闭面板会撤销单次报告的调度，重新打开时，仍处于运行状态的计时器从一个新的间隔开始等待。

## 首版边界

当前的状态转场作用于视觉属性及组件实例的布局属性。标准控件仍在扩充。文件扩展名约定为 `.xr`，语法取 Slint 风格子集。

桌面宿主通过 AccessKit 发布可见控件、当前焦点和活动模态窗口的无障碍树。`accessible-name` 和 `accessible-description` 可补充控件语义，例如纯图标按钮的名称。当前支持聚焦、点击、文本值替换和数值步进。`ComboBox` 和菜单公开弹层内的可见选项，子菜单会跟随当前展开层级。滚动容器公开偏移和范围，可按项、按页或指定偏移滚动。

单行和多行文本框及可编辑 `ComboBox` 公开已提交的文本、光标和选区。辅助技术可读取文本范围并设置选区，操作沿用编辑器的 Unicode 字素边界与视觉换行。可见文本按方向拆分范围并发布字符坐标。IME 预编辑期间保留已提交的文本，选区操作在预编辑结束后恢复。

`xrack-layout` 计算线性布局、绝对坐标和祖先裁剪。隐藏元素不占布局空间。鼠标点击需要在同一目标内按下和释放，禁用或被裁剪的控件不会触发点击。winit 将鼠标、Tab / Shift+Tab 和 Enter / Space 事件交给该状态机。绑定变化、窗口缩放和 DPI 变化会触发重绘。

原生 GPU 路径目前在 macOS / Metal 验证。离屏 GPU 测试需要适用的图形适配器和支持中文的系统字体，需显式运行。单行文本输入已接入 IME 预编辑和提交。
