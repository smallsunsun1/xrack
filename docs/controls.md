# 控件与布局参考

[返回项目首页](../README.md)

列表、树和表格见 [模型与虚拟视图](models.md)。控件的完整可运行用例见 [examples](../examples/)。

## 主题、布局和动画

窗口可设置 `color-scheme: system;` 跟随宿主系统主题，或用 `light`、`dark` 固定应用内容的配色。`accent-color` 可绑定到应用状态，用于按钮、滑条、焦点和选中态。主题同时应用到窗口内的菜单和弹层。控件显式指定的背景和前景会覆盖调色板默认值。省略主题属性时沿用原有配色。运行 `bazel run //examples/controls:controls -- --theme` 可实时切换三种模式。

Android 的 Activity 把系统夜间模式变化转发给 Rust，由 Rust 更新窗口调色板。示例在 Manifest 的 `android:configChanges` 中声明 `uiMode`，让原窗口接收配色变化并保留页面状态。 [Android 系统配色回归](../examples/android/README.md#颜色与字体回归)

iOS 的 `system` 模式读取 UIKit 当前窗口的外观，应用运行中切换浅色或深色时会刷新调色板，回到前台时也会重新读取。[iOS 系统外观示例](../examples/ios/README.md#系统外观示例) 演示了这一过程。

iOS 开启「增强对比度」后，默认文字、输入边框、焦点指示、选中态和滚动滑块使用对比更强的配色。`system`、`light` 和 `dark` 都会响应这个设置，控件显式指定的颜色仍由其属性决定。

iOS 开启「不以颜色区分」后，`TabBar` 和 `TabWidget` 为当前标签加下划线，`ComboBox` 为已选项加勾标，并用轮廓标出当前键盘高亮项。自定义选项内容会为勾标留出空间，整个选项仍可点击。应用运行中切换设置或返回前台时会刷新标记。[iOS 选择示例](../examples/ios/README.md#系统外观示例)

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

普通 `for` 模板中的匿名 `StyledText` 也支持 `link-activated(target)`。回调可以读取触发链接的记录。记录已移走或链接内容已更新时，旧场景中的链接不会触发回调。

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

表单对话框可以把输入项放进一个 `ScrollView` 或 `Flickable`，再把 `DialogButton` 或 `DialogButtonBox` 作为紧随其后的直接子项。纵向空间足够时按钮留在正文下方。键盘或小视口让正文不足一行时，弹层会在安全区内收紧上下外边距。横向有余量时，按钮移到逻辑末端，正文继续滚动。RTL 下操作区移到左侧。[键盘表单示例](../examples/ios/README.md#键盘辅助输入)

iOS 的编辑器在旋转和停靠键盘调整视口期间会保留仍有效的 UIKit 编辑焦点。布局稳定后，滚动视口会继续显示当前字段。控件被隐藏、禁用或移出当前弹层时，焦点正常转移。

iPad 的系统「收起键盘」按钮会保留当前字段的草稿和逻辑焦点。窗口尺寸变化后键盘继续保持收起，点进原字段可恢复编辑。

[Android 联系人表单回归](../examples/android/README.md#模拟器测试) 检查邮件、电话和网址的系统键盘类型，以及 Next／Done 的校验、焦点切换和收起行为。

iOS 的系统字体回退使用 Core Text 绘制简体中文苹方字形。常规字重的回退字形与文本测量、光标定位和选区共用同一份排版数据，应用可以继续用 `font-family` 指定自己的字体。

单行和多行编辑器都支持按词操作。macOS 使用 Option，Windows 和 Linux 使用 Ctrl。配合左右方向键可移动一个 Unicode 词段，再按住 Shift 可扩展选区。配合 Backspace 或 Delete 可删除光标前方或后方的词段，也可删除已有选区。方向键遵循双向文字的视觉顺序，删除使用文本中的逻辑前后顺序。每次删除可以撤销。运行 `bazel run //examples/form:form -- --bidi` 试用混合方向文字。

`TextInput`、`LineEdit`、`TextEdit`、`TextArea`、`SearchField` 和可编辑的 `ComboBox` 支持 `mouse-selection-mode`。默认 `MouseSelectionMode.characters` 按 Unicode 字素拖选，`MouseSelectionMode.words` 让拖动和 Shift+点击按完整 Unicode 词段扩选。两种模式下双击选中词段，三击选中由换行分隔的整段。按住最后一次点击后拖动，可继续按词或按段扩选。单行输入框的三击会选中整行。密码字段按词或按段选择时以整段文字为单位。

`MouseSelectionMode.caret` 让鼠标只负责聚焦和放置光标。键盘、无障碍和程序仍可设置选区。运行 `bazel run //examples/form:form -- --editor-selection`，点击模式按钮切换三种行为。

触摸屏上，长按文本可选中光标处的词，保持手指接触并拖动可扩展选区，松手后打开编辑菜单。单行和多行编辑器的选区端点会显示手柄，松手后仍可重新拖动手柄来调整边界。菜单使用适合触摸的行高，复用编辑器的复制、剪切和粘贴规则。只读控件仍可选择和复制文字。可在 [Pocket Tasks](../examples/ios/README.md) 的备注页体验。

iOS 长按选词或拖完选区手柄后使用 UIKit 编辑菜单。菜单动作沿用 XRack 的选区、只读、密码保护和撤销历史，键盘可保持打开。iOS 跨应用粘贴可由系统弹窗请求授权。允许后，XRack 会在恢复活跃时核对原字段、文字和选区，再完成这次粘贴。拒绝授权会保留原文，应用真正进入后台时会取消尚未完成的粘贴。

iOS 编辑菜单打开时，Escape、无障碍取消或从屏幕起始边缘返回会先关闭菜单，保留文本选区及当前对话框。下一次独立的取消操作会处理下层控件。

iOS 原生编辑菜单使用当前可见的文字选区定位，空选区使用光标。编辑器重新布局后，菜单会跟随当前位置。浮动键盘盖住部分选区时使用仍可见的部分，选区完全不可见时菜单会收起。

iOS 的 VoiceOver「操作」转子为正在编辑的文本控件提供全选、拷贝、剪切、粘贴、撤销和重做。操作随选区和编辑历史更新。只读字段可全选和拷贝，密码字段隐藏拷贝与剪切，输入法组字期间暂停这些操作。[iOS 编辑示例](../examples/ios/README.md)

iOS 上可用 VoiceOver 双击可编辑的单行、多行、搜索或数值输入框，进入 UIKit 编辑。收起键盘后，可以双击当前字段继续输入，草稿和选区会保留。

在 `TextArea` 或 `TextEdit` 中按住鼠标拖出编辑区域时，内容会沿该方向自动滚动并继续扩展选区。离边缘越远，滚动越快。不换行的长行也可向左右拖选。松开鼠标、移回编辑区域或失去焦点时停止滚动。运行 `bazel run //examples/form:form -- --long-text-selection` 可体验字素和词段两种拖选方式。

触摸长按选词后，可保持手指接触并拖到编辑区边缘，让长文本持续滚动并扩展选区。已显示的选区手柄也支持同样的操作。移回编辑区、松手或目标文本被替换时停止滚动。[iOS 选区示例](../examples/ios/README.md)

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

iPadOS 的鼠标滚轮与触控板滚动会将本次手势的修饰键传给 `WheelHandler`。Command 对应 `event.modifiers.primary`。控件可以消费组合滚动，后续普通滚动仍交给外层视口处理。[iPad 指针示例](../examples/ios/README.md#ipad-鼠标与触控板)

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

记录下拉框可以用 `enabled-role: "access.available";` 指定布尔字段。禁用项仍显示并向辅助技术标记为不可用，鼠标、键盘和补全也无法提交该项。提交时会重新核对当前模型。程序可保留指向禁用项的当前值。非编辑下拉框支持按前缀定位，短时间连续输入会合成前缀，重复按同一个字符会在匹配项之间循环。弹层关闭时直接选中，打开时只移动高亮，按 Enter 才提交。

给 `ComboBox` 添加一个匿名的 `ComboOption` 子项，可以为字符串模型和记录模型定制下拉选项的外观。模板使用只读的 `option-text`、`option-index`（从 0 开始）、`option-enabled`、`option-selected` 和 `option-highlighted`。每个可见选项都使用自身的数据和状态绘制，鼠标、键盘和辅助技术仍通过下拉框提交原选项。重复列表中的下拉框可把模板放在各自的组件内。运行 `bazel run //examples/selection -- --record-combo` 查看示例。

模板里的 `option-data` 保留模型行的类型。字符串模型可直接显示它，记录模型可读取 `option-data.notes` 或 `option-data.access.available` 等字段。字段由编译器校验。修改一项的说明或图标会更新正在显示该项数据的弹层。

`ComboContent` 是 `ComboBox` 的单个直接子项，用来替换不可编辑时的选中标签。可以用 `choice.current-text` 和 `choice.current-value` 组合文字、编号和图标。内容布局在文字区域内，并和下拉箭头一起适应 RTL。设为不可见会恢复默认标签。切换到编辑模式会暂停求值自定义内容，输入区由自带的编辑器接管。点击、键盘和无障碍仍使用 `ComboBox` 的语义。

在 `ComboBox` 内声明一个 `ComboPopup`，可绑定 `width`、`max-height`、`row-height`、`padding`、`background`、`border-color`、`border-width` 和 `border-radius`。`max-height` 包含内边距，弹层根据窗口剩余空间向上或向下展开。`width` 的百分比相对于控件宽度，`max-height` 的百分比相对于窗口高度。默认行高是 28px，默认最多显示八行，默认宽度取控件宽度与 96px 中较大者。设置 `scroll-indicator: true` 后，内容溢出时会在逻辑末端显示滚动位置，滚轮和键盘仍用于浏览选项。运行 `bazel run //examples/selection -- --popup-style` 体验。

iOS 和 iPadOS 的 `ComboBox` 下拉项及 `SearchField` 建议会随系统文字大小调整字号和行高。`row-height` 是常规字号下的基准，显式弹层宽度和最大高度仍约束布局。弹层避开系统安全区、停靠键盘和浮动键盘。键盘移开后，仍打开的弹层会恢复位置。选项过多时可在弹层里滑动或用 VoiceOver 翻页。点击下拉选项会更新选值，点击搜索建议会填入输入框，随后可用搜索按钮或键盘搜索键提交。

iOS 的 VoiceOver 可从可见选项发起翻页，UIKit 会收到可见的选项范围和总数。`ComboBox` 整页禁用时仍可浏览和继续翻页。只有激活可用选项才会改变已选值。[iOS 翻页示例](../examples/ios/README.md)

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

`MenuItem` 的 `shortcut: "Primary+Shift+E";` 与点击共用 `activated()`。菜单在正文和快捷键都能完整放下时显示组合键。`Primary` 在 macOS 上表示 Command，在其他平台表示 Ctrl。也可以组合 `Ctrl`、`Alt`、`Shift` 和 `Super`，主键支持英文字母、数字、方括号、Tab 和 F1 到 F12。字母、数字、方括号和 Tab 需要 Ctrl、Alt、Super 或 Primary 中的一个修饰键。普通 Tab 和 Shift+Tab 用于焦点遍历。`MouseBack` 和 `MouseForward` 对应鼠标侧键，可独立绑定。无效字符串不会注册快捷键，可将绑定设为空字符串清除快捷键。

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

### CustomRender

`CustomRender` 将 Rust 应用的绘制放在普通布局节点中。设置尺寸和 `renderer: "注册名"`，用 `data: string` 和 `revision: int` 向 painter 传参数。它遵循布局、祖先裁剪、变换、透明度和 `z` 顺序。应用需要触发更新时，可以修改绑定到 `data` 或 `revision` 的属性。

`register_gpu_pass` 把同一个 painter 接入视图的绘制顺序。整像素对齐且符合裁剪条件的顶层控件可在主目标上录制一个 GPU pass。需要组透明度、旋转或复杂裁剪时，运行时用离屏目标承接同一个 painter。管线按传入的目标格式创建，混合使用预乘 alpha，视口和 scissor 由运行时设置。

`register_shader_effect` 接受 WGSL 函数 `shade(uv: vec2<f32>, frame: vec4<f32>) -> vec4<f32>`。UV 是控件内归一化坐标，`frame` 依次包含目标像素宽高、缩放和控件修订号。函数返回直通 alpha 的颜色，渲染器在写入透明纹理时乘入 alpha。

```xr
CustomRender {
    width: 240px;
    height: 96px;
    renderer: "level-meter";
    data: "green";
}
```

在运行窗口的 UI 线程注册并持有返回的句柄。AOT 和解释器使用同一组注册函数。没有注册 painter 时，节点仍参与布局，其普通背景与子节点可作为占位。在独立 VS Code 预览进程中会显示这个占位，运行应用内的热重载可以使用应用已注册的 painter。

```rust
use xrack::{Color, layout::Rect, renderer::{Geometry, register_geometry}};

let _painter = register_geometry("level-meter", |context| {
    vec![Geometry::Rectangle {
        bounds: Rect::new(0.0, 0.0, context.bounds.width * 0.75, context.bounds.height),
        color: Color(0x39a76aff),
    }]
})?;
// 用 xrack::run(ui) 启动应用，句柄在窗口运行期间保持有效。
```

`register_geometry` 接受本地逻辑像素坐标的矩形、折线、多边形、图像和文本。`register_gpu` 提供当前设备、命令编码器和节点纹理视图，应用可创建自己的 pipeline 或深度纹理并录制 render pass。颜色目标为 `Rgba8Unorm`，输出采用预乘 alpha。XRack 按布局计算纹理像素尺寸，先执行这些命令，再把纹理合成到场景。应用可通过回调捕获自己的资源缓存，按设备和尺寸复用资源。[GPU 注册与绘制用例](../tests/custom_render_test.rs)

### SceneView3D

`SceneView3D` 是一个参与普通 2D 布局的 3D 视口。`model` 接收记录列表。可以给记录起任意名称并添加应用字段，编译器会检查绘制和拾取共用的字段。

`scene-background` 使用 `#RRGGBBAA` 颜色时会保留透明度。例如 `#00000000` 让下层 2D 界面从视口空白处透出，`#24405480` 添加半透明着色。实体材质仍遮挡下层，半透明材质、抗锯齿边缘、体积雾、泛光和发光粒子按覆盖率合成。乘法混合只处理视口内已有的颜色。启用环境天空盒时，天空在相机画幅内覆盖背景。

景深、HDR 和调色保留视口的透明度。玻璃的屏幕空间折射采样 3D 场景，透射路径上的空白处则允许下层 2D 内容以透射亮度参与合成。静止视口可以用 `cache-rendering: true;` 复用带透明度的纹理，后方 2D 内容更新时只需重新合成。 [透明视口示例](../examples/puzzle3d/transparent_view.xr)

```xr
struct SceneObject3D {
    id: string,
    mesh: string,
    x: float, y: float, z: float,
    scale-x: float, scale-y: float, scale-z: float,
    yaw: float, pitch: float, roll: float,
    color: color,
}
```

`id` 是物体的标识，给可交互物体使用唯一的 ID。`mesh` 可以引用内置图元或 `@mesh-url("models/marker.glb")`。`scale-x`、`scale-y` 和 `scale-z` 独立缩放网格，再依次应用 X 轴 pitch、Y 轴 yaw、Z 轴 roll 以及平移。未知网格键或无效变换会跳过该行。

| 图元 | 模型坐标和材质槽 |
| --- | --- |
| `primitive:box` | 边长 1，中心在原点。材质槽为 `#0`。 |
| `primitive:sphere` | 半径 1，中心在原点。材质槽为 `#0`。 |
| `primitive:rectangle` | 边长 1，中心在 XY 平面的原点。正面朝 +Z。材质槽为 `surface` 或 `#0`。 |
| `primitive:cylinder` | 半径 0.5，高度 1，沿 Y 轴居中。三个材质槽是 `side`、`top`、`bottom`，对应 `#0`、`#1`、`#2`。 |
| `primitive:cone` | 底面半径 0.5，底面中心在原点，尖端在 Y=1。材质槽是 `side`、`base`，对应 `#0`、`#1`。 |

圆柱和圆锥均使用 32 等分。侧面沿周向展开 UV，盖面使用独立的圆盘 UV。矩形可以作为图面或通过旋转作地面，默认可从两面看到和拾取。射线查询使用可见材质的 alpha 裁切规则，球体扫掠使用图元的几何表面。相同图元共享准备后的顶点数据，并可用作粒子模型或网格发射表面。 [基础图元示例](../examples/puzzle3d/primitive_meshes.xr)

#### 参数化几何

给场景行增加 `geometry` 记录，就能把尺寸、细分数和形状绑定到 `.xr` 属性。带 `geometry` 的行可以省略 `mesh` 字段。如果两个字段都存在，非空的 `shape` 优先使用参数化几何。将 `shape` 设为空字符串可回到 `mesh` 或只提供变换的空节点。

```xr
struct ShapeSpec {
    shape: string, radius: float, tube-radius: float, segments: int, rings: int,
}
```

例如 `ShapeSpec { shape: "torus", radius: root.radius, tube-radius: 0.2, segments: 32, rings: 16 }` 在 XY 面生成一个带孔圆环。

| `shape` | 尺寸参数及默认值 | 材质槽 |
| --- | --- | --- |
| `"plane"` | XY 面居中，`width`、`height` 默认 1 | `surface` |
| `"box"` | 原点居中，`width`、`height`、`depth` 默认 1 | `front`、`back`、`right`、`left`、`top`、`bottom` |
| `"sphere"` | 原点居中，`radius` 默认 0.5 | `surface` |
| `"cylinder"` | Y 轴居中，`radius` 默认 0.5，`height` 默认 1，`top-radius` 默认等于底半径 | `side`、`top`、`bottom` |
| `"cone"` | Y = 0 为底面，`radius` 默认 0.5，`height` 默认 1，`top-radius` 默认 0，可设为正值生成截锥 | `side`、截锥的 `top`、`base` |
| `"capsule"` | Y 轴居中，`radius` 默认 0.5，含两个半球的总 `height` 默认 2 | `surface` |
| `"torus"` | XY 平面居中，`radius` 是到管中心的半径，默认 0.75，`tube-radius` 默认 0.25 | `surface` |

尺寸字段使用 `float`。尺寸范围为 0.001 到 200，锥顶半径还可取 0。胶囊总高度至少为直径，圆环管半径须小于主半径。无效尺寸会跳过该行。圆柱和圆锥可用 `capped: false` 去掉端盖。

`segments` 和 `rings` 使用 `int`，最大值为 64。它们分别控制周向和纵向细分。平面和盒面使用两个方向的网格细分，胶囊的 `rings` 表示每个半球的细分数。GPU 绘制、阴影、射线和球体扫掠共用三角网格，圆环的空心区域可以穿过。相同参数共享准备后的网格，改颜色、材质或物体变换时复用网格。 [参数化网格示例](../examples/puzzle3d/parametric_geometry.xr)

运行时可为生成网格、导入的 glTF 和各级 LOD 使用 GPU 索引。只有位置、法线、UV、颜色以及适用的切线、光照贴图坐标、关节权重和形变增量全部一致时才合并顶点。颜色、阴影、深度和运动向量共用该索引，并沿用三角形顺序和材质分段。Rust 的 `Renderer::scene_draw_stats()` 提供 `mesh_vertex_bytes_uploaded`、`mesh_index_bytes_uploaded`、`mesh_deformation_bytes_uploaded` 和 `indexed_mesh_uploads`。`color_vertices` 统计提交的顶点引用数。

#### 自定义三角网格

`geometry` 记录也可声明 `vertices: [Vertex]` 和可选的 `indices: [int]`，直接绑定应用生成的顶点和三角形。两张表都为空时，该行可以使用自己的 `mesh` 或作为只提供变换的节点。

```xr
struct Vertex {
    position: point3d, normal: vector3d, u: float, v: float,
    u1: float, v1: float, color: color,
}
struct MeshSubset { offset: int, count: int, name: string }
struct MeshData { vertices: [Vertex], indices: [int], subsets: [MeshSubset] }
```

顶点必须声明 `position`。省略 `normal` 或设置零向量时，运行时根据共享顶点相邻的三角形生成面积加权法线。`u`、`v` 默认为 0。`u1`、`v1` 可为使用第二组 UV 的材质贴图提供坐标，省略时沿用第一组的对应分量。顶点色默认白色。索引从 0 开始，每三个索引组成一个三角形，正面按照逆时针绕序定义。省略索引表或传空表时，按每三行顶点直接生成一个三角形。

可选的 `subsets` 表为连续的三角形指定材质槽。例如 `MeshSubset { offset: 6, count: 9, name: "trim" }` 将从第 6 个索引开始的三个三角形交给 `trim`。无索引表时，范围直接按顶点计算。`offset` 和 `count` 都须为 3 的倍数，范围不得重叠或越界，每张表最多 256 段。未分配部分使用 `surface`。同名分段共用材质，连续的同名分段合并绘制。按名称或 `#N` 从 `material-slots` 选择材质，数字按槽位第一次出现的三角顺序编号。VS Code 可以从同一记录里的分段字面量补全槽名。

修改分段会更新绘制范围和逐槽透明裁切的拾取行为，并继续复用 GPU 顶点缓冲。非法范围会跳过该网格，修复后自动恢复。 [多材质分段示例](../examples/puzzle3d/mesh_subsets.xr)

每个网格最多 60,000 个顶点和 180,000 个索引。位置和 UV 值的绝对值上限为 10,000。越界索引、不完整或退化的三角形会让这一行暂时跳过绘制和查询，修复数据后会恢复。用 `Model.set` 修改顶点表或索引表可直接更新绘制、阴影、拾取和球体扫掠。外层物体列表可保持原有行身份，相同顶点和索引数据共享准备好的网格。 [可编辑三角网格示例](../examples/puzzle3d/authored_mesh.xr)

记录可以添加 `parent: string`、`visible: bool` 和 `pivot-x`、`pivot-y`、`pivot-z` 三个 `float` 字段。父节点用同一列表里的唯一 `id` 引用，空父引用表示根节点。写 `mesh: ""` 可以创建只提供变换的节点，例如先定义 `id: "hinge"` 的门轴，再在门的记录里设置 `parent: "hinge"`。父节点可以在子节点之后声明。子节点的位置、旋转和缩放都相对父节点，局部枢轴会映射到该行的 `x`、`y`、`z`。省略这些可选字段时，父引用为空、可见、枢轴为原点。

父节点隐藏、父引用缺失或有歧义、或者链路形成循环时，对应子树会从绘制、投影和拾取中跳过。多级旋转和非均匀缩放按仿射变换合成，拾取与光照使用同一世界坐标。相机绕视和灯光更新会复用场景几何未变的实例缓冲区。

`@mesh-url` 以声明所在的 `.xr` 文件为基准，接收 glTF 2.0 的 `.glb` 或 `.gltf`。导入器读取默认场景的节点变换、三角网格和每个图元的底色材质。底色由 glTF 的 `baseColorFactor`、顶点色、PNG/JPEG 底色纹理和记录的 `color` 相乘。保留模型原底色时可将 `color` 设为白色。纹理支持内嵌数据和相对文件、重复与镜像寻址，以及最邻近和线性采样。当前使用纹理的基础层。

用 `@mesh-url("models/ridge.png")` 可以从 8 位或 16 位灰度 PNG 生成高度图地形。每个像素提供一个线性高度样本，黑色对应局部 Y = 0，白色对应 Y = 1。图片从左到右映射到局部 X，从上到下映射到局部 Z。地形在 XZ 平面内居中，基础宽深各为 1，用模型的 `scale-x`、`scale-y`、`scale-z` 设置实际范围。

每个网格单元生成两个三角形，网格边缘和内部使用邻近高度计算平滑法线。物体使用默认的粗糙漫反射材质，可用记录的 `color` 上色。绘制、阴影、视口点选和世界射线查询都使用该网格。PNG 每边至少需要两个样本，单个地形最多 150,000 个网格单元。

可用 `@mesh-url("models/ridge.png", lightmap-uv, auto-lod)` 生成烘焙 UV 和远景简化网格。光照 UV 用单张 XZ 投影图表覆盖地形。AOT 构建时准备顶点，解释器在加载时准备，在帧间复用 GPU 网格。VS Code 可补全 PNG 路径、打开源文件，并在悬停时查看尺寸和位深。 [地形点选与放置示例](../examples/puzzle3d/heightfield.xr)

glTF 材质使用金属度与粗糙度模型。导入器读取 `metallicFactor`、`roughnessFactor`、`metallicRoughnessTexture`、自发光和遮蔽参数。金属度和粗糙度分别来自组合贴图的 B、G 线性通道。遮蔽使用 R 通道调节环境光，自发光贴图按 sRGB 采样。每个材质可以使用两组纹理坐标。声明 `KHR_materials_unlit` 的材质只使用底色和透明度。

`KHR_materials_clearcoat` 支持独立的清漆强度、粗糙度和法线。强度和粗糙度因子分别乘以线性贴图的 R、G 通道。清漆根据自身的法线和菲涅耳反射影响直射光及环境反射，基础材质的透明度和拾取规则仍由底色决定。强度为 0 时跳过清漆贴图与着色。清漆法线可以使用第二组 UV，此时使用对应的切线空间。

`KHR_materials_emissive_strength` 在材质准备时把发光倍数乘进线性发光因子。HDR 视口会保留高于 1 的发光值供色调映射使用。可在 VS Code 悬停 `@mesh-url` 查看资源声明的这两项材质扩展。 [材质对照示例](../examples/puzzle3d/materials.xr)

`normalTexture` 使用线性 RGB 表示切线空间法线。`scale` 控制贴图在切线和副切线方向上的扰动强度。导入器使用模型提供的切线，缺少切线时在原始几何上用 MikkTSpace 生成，再应用节点的镜像和缩放变换。法线贴图影响光照。拾取继续使用模型的三角面。

`OPAQUE` 以不透明表面绘制。对 `MASK`，绘制和拾取都应用 alpha 阈值，射线可以穿过被裁剪的区域。`BLEND` 在不透明和裁剪子网格之后绘制。透明子网格按包围盒中心的相机深度从远到近排序，测试已有深度，在同一个视口附件中以线性颜色混合。相交的透明面采用子网格排序。拾取选择最近的可见表面，跳过零 alpha 的混合表面，纹理 alpha 以基础层的放大滤波方式采样。

设置 `order-independent-transparency: true;` 可以让视口使用加权顺序无关透明。它按每个片元的相机深度和不透明度加权，在相交处也能反映两边不同的前后关系。加权结果是透明叠色的近似，适合重叠的半透明模型。默认仍使用子网格排序。

加权透明复用不透明几何的深度，允许同网格的透明实例合批。开启 MSAA 时按样本合成，HDR 视口在合成后做色调映射。视锥裁剪后没有可见透明实例时，视口沿用普通颜色通道。`Renderer::scene_draw_stats()` 的 `transparency_composites` 记录累计合成次数。拾取仍返回实际最近的可见表面。 [相交透明面示例](../examples/puzzle3d/transparency.xr)

#### 声明式 PBR 材质

可以给场景对象增加 `material` 字段，值是一个自定义记录。比如场景记录声明 `material: SurfaceStyle` 后，可以绑定下面的材质因子。

```xr
struct SurfaceStyle {
    base-color: color,
    metalness: float,
    roughness: float,
}
```

`SurfaceStyle { base-color: #b87648, metalness: 0.65, roughness: root.roughness }` 可以用在内置球体、盒体或导入的 glTF 上。内置模型默认使用白色底色、金属度 0 和粗糙度 0.5。对 glTF，同一记录的因子覆盖对应的各子网格材质，未声明的字段继承原值。原始贴图、UV、法线贴图和场景记录中的 `color` 仍参与着色。若材质记录有 `enabled: bool` 字段，可以设为 `false` 恢复模型原有材质。

| 字段 | 类型和行为 |
| --- | --- |
| `base-color` | `color`，保留 alpha。底色贴图与物体的 `color` 继续与它相乘。 |
| `metalness`、`roughness` | `float`，范围 0 到 1。现有组合贴图继续调节这些因子。 |
| `occlusion-amount` | `float`，控制环境遮蔽贴图的强度，范围 0 到 1。 |
| `specular-amount`、`specular-color` | `float` 和 `color`，分别控制介电材质的镜面反射强度和颜色。强度范围 0 到 1。 |
| `thickness`、`attenuation-distance`、`dispersion` | `float`，控制体积厚度、吸收的参考距离和色散。 |
| `attenuation-color` | `color`，表示光穿过参考距离后各通道保留的比例。 |
| `emissive-color`、`emissive-strength` | `color` 与 `float`。发光颜色使用线性值乘以强度和原始发光贴图，强度范围 0 到 64，默认倍数 1。 |
| `alpha-mode`、`alpha-cutoff` | `string` 与 `float`。支持 `"opaque"`、`"mask"`、`"blend"`。裁切阈值默认 0.5，拾取与阴影共用材质的透明规则。 |
| `material-shader`、`shader-parameters` | `string` 与 `vector3d`。引用 WGSL 材质程序并传入三个可绑定参数。 |
| `shader-displacement` | `float`，限制顶点程序在每条局部坐标轴上的最大位移。 |
| `shader-textures` | 记录列表。按索引向 WGSL 提供图片、实时二维纹理和命名场景捕获。 |
| `unlit` | `bool`。显式 `unlit: false` 可以让导入的不受光材质使用 PBR。 |
| `clearcoat`、`clearcoat-roughness` | `float`，范围 0 到 1。控制清漆反射强度和粗糙度。 |
| `transmission`、`index-of-refraction` | `float`，范围分别为 0 到 1 和 1 到 3。透射使用视口当前的背景和深度，导入模型的厚度或体积参数继续生效。 |

材质调整属于绘制更新。网格、蒙皮和已有的贴图可以在参数变化时留在 GPU 上。只改变粗糙度、清漆或底色 RGB 时，阴影图可继续复用。 [声明式材质示例](../examples/puzzle3d/authored_material.xr)

#### 可编程材质

将 `@material-shader-url("effects/material_surface.wgsl")` 绑定到模型材质或指定槽位的 `material-shader` 字段。路径相对当前 `.xr` 文件。Bazel 的 `xr_ui_library` 需要声明该 WGSL 文件。空字符串使用内建着色。

```xr
struct ProgramStyle { material-shader: string, shader-parameters: vector3d }

ProgramStyle {
    material-shader: @material-shader-url("effects/material_surface.wgsl"),
    shader-parameters: Vector3D.new(0.9, 0, 0),
}
```

片元材质使用 `fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface`。输入提供两套 UV、世界坐标、观察方向、顶点色、正反面标记和三个参数。材质程序可以修改已经采样的线性底色、自发光、世界空间法线、粗糙度和金属度。PBR 材质用修改后的数据求光照，无光照材质以底色加自发光输出。透明度和裁切继续用材质原本的设置，编译器会拒绝在自定义函数中使用 `discard`。

绑定参数可以随交互或动画变化，保持相同 WGSL 时可以复用着色器管线和顶点缓冲。VS Code 在保存 WGSL 后更新材质预览。 [逐槽着色示例](../examples/puzzle3d/material_shader.xr) · [WGSL 材质](../examples/puzzle3d/effects/material_surface.wgsl)

`shader-textures` 的记录用 `texture: string` 引用 `@texture-url(...)`，用 `item: string` 引用当前窗口中 Rectangle 的 `texture-source`，或用 `capture: string` 引用当前视口的命名画面。输入优先级为 `capture`、`texture`、`item`。每项可设置 `enabled: bool`、`filter: string`（`nearest` 或默认 `linear`）、`wrap-u` 和 `wrap-v`（默认 `clamp`，也可选 `repeat` 和 `mirror`）。图片的 `color-space` 默认为 `srgb`，数据图可选 `linear`。实时二维源按其捕获编码转换。

WGSL 用 `material_texture(index, uv)` 取得直通 alpha 和线性 RGB。索引为从 0 开始的 `u32`，采样使用第 0 层 mip。可用 `material_has_texture(index)` 查询输入，`material_texture_size(index)` 读取像素尺寸。输入缺失、禁用或超出 GPU 槽位时，查询返回 false、零尺寸和透明黑。最多提供四项。只有 16 个片元纹理槽位的设备可为普通材质提供前三项，启用清漆后槽位全部用于 PBR。 [贴图输入示例](../examples/puzzle3d/material_texture_inputs.xr) · [WGSL 采样](../examples/puzzle3d/effects/material_texture_inputs.wgsl)

同一 WGSL 文件可以定义多个材质函数。`fn scene_vertex(input: XrVertexInput) -> vec3<f32>` 修改顶点位置。顶点输入包括蒙皮和 morph 后的局部坐标 `input.position` 以及 `input.parameters`。返回值是局部坐标中的附加位移。正值 `shader-displacement` 开启位移，默认值为 0，允许范围为 0 到 10000。运行时会按该值限制各轴的位移并扩展渲染包围盒。颜色、阴影、深度和运动矢量使用同一顶点函数，TAA 使用上一帧的位移参数。

顶点函数可实现风摆、波浪和局部滑动。表面法线仍由网格与片元材质提供，`shader-textures` 的采样函数用于片元阶段。CPU 拾取、碰撞和烘焙使用原始网格及其蒙皮和 morph 姿态。需要精确交互时，可在 CPU 上用与视觉位移相符的碰撞体。 [顶点示例](../examples/puzzle3d/vertex_program.xr) · [WGSL 顶点函数](../examples/puzzle3d/effects/vertex_program.wgsl)

#### 可编程光照

受光材质可定义 `scene_light(input: XrLightInput, response: XrLightResponse) -> XrLightResponse`。引擎对方向光、点光和聚光分别调用该函数。用 `input.kind` 与 `XR_DIRECTIONAL_LIGHT`、`XR_POINT_LIGHT` 或 `XR_SPOT_LIGHT` 匹配。输入还提供世界空间光照方向、线性灯光颜色、衰减、阴影可见度和菲涅耳响应。`input.shading` 包含表面参数和材质输入，包括可绑定的 `shader-parameters`。返回线性漫反射与镜面反射响应。默认响应已包含表面与灯光方向夹角的余弦，引擎在返回后乘以灯光颜色、衰减和阴影可见度。

`scene_ambient(input: XrAmbientInput, diffuse: vec3<f32>) -> vec3<f32>` 处理动态环境漫反射。输入包含场景与探针的环境辐照度、遮蔽因子和材质参数。返回的线性颜色参与环境遮蔽、清漆分层和屏幕空间间接光。烘焙光照贴图负责它所覆盖的漫反射项。两个函数可以独立声明，也可与顶点和表面函数搭配。 [光照工作台示例](../examples/puzzle3d/material_lighting.xr) · [WGSL 光照函数](../examples/puzzle3d/effects/material_lighting.wgsl)

#### 各向异性高光

用 `anisotropy-strength: float` 控制拉丝金属等材质的方向性，范围 0 到 1，默认为 0。用 `anisotropy-rotation: float` 沿网格切线平面旋转高光，单位为度。强度增加时，GGX 高光沿所选方向展宽。

`anisotropy-map` 可接收 `@texture-url(...)` 或带采样设置的记录。贴图按线性数据读取，RG 从 0 到 1 映射为切线空间的 −1 到 1 方向，B 与材质的强度相乘。网格可以使用导入的切线，也可以从 UV 导数提取方向。没有有效 UV 或切线时使用模型局部坐标作为基准。

glTF 的 `KHR_materials_anisotropy` 会保留强度、旋转和方向贴图。glTF 文件里的旋转单位为弧度。直射光使用各向异性 GGX，环境探针和屏幕空间反射用偏转法线近似反射方向，保留原有的粗糙度预过滤。调节参数时复用网格、贴图和环境探针。 [拉丝金属示例](../examples/puzzle3d/material_anisotropy.xr)

#### 菲涅耳反射

菲涅耳曲线决定正视表面和斜看表面时的反射比例。底层材质使用下表字段，清漆层使用对应的 `clearcoat-fresnel-*` 字段，可以分别调节两层。

| 字段 | 类型和行为 |
| --- | --- |
| `fresnel-power` | `float`，范围 0.125 到 64，默认 5。较小的指数让反射更早随视角变亮。 |
| `fresnel-scale` | `float`，范围 0 到 8，默认 1。乘以反射曲线。 |
| `fresnel-bias` | `float`，范围 −1 到 1，默认 0。在缩放后加到反射曲线上，结果限制在 0 到 1。 |
| `fresnel-scale-bias-enabled` | `bool`，默认 `false`。开启后应用缩放和偏移。指数独立生效。 |

直射光用修改后的 Schlick 曲线计算高光和剩余的漫反射能量。环境探针和屏幕空间反射共享同一份 GGX 积分近似，借助角度映射应用新的指数。未自定义曲线的材质使用普通着色管线。绑定参数可以在保留 GPU 网格和探针的情况下更新。 [菲涅耳示例](../examples/puzzle3d/material_fresnel.xr)

#### 顶点颜色和材质遮罩

glTF 的 `COLOR_0` 默认与材质底色相乘，顶点 alpha 也参与 `"mask"` 和 `"blend"` 模式。材质记录可用 `vertex-colors: bool` 控制这一步，默认 `true`。设为 `false` 时，深度、阴影、运动向量、拾取和绘制会一起跳过顶点 alpha，材质、物体和贴图的透明规则仍生效。

材质也可以用插值后的顶点颜色做参数遮罩。每个遮罩字段接收 `"r"`、`"g"`、`"b"`、`"a"` 中的一个值，用所选通道乘以对应的材质参数。使用 `"none"` 关闭该遮罩，包括清除先前层级的覆盖。遮罩可在 `vertex-colors: false` 时继续使用。

| 遮罩字段 | 被调节的参数 |
| --- | --- |
| `vertex-metalness-channel`、`vertex-roughness-channel` | 金属度、粗糙度 |
| `vertex-normal-channel` | 底层法线贴图强度 |
| `vertex-occlusion-channel`、`vertex-specular-channel` | 环境遮蔽贴图强度、介电材质高光强度 |
| `vertex-clearcoat-channel`、`vertex-clearcoat-roughness-channel`、`vertex-clearcoat-normal-channel` | 清漆强度、粗糙度和法线贴图强度 |
| `vertex-transmission-channel`、`vertex-thickness-channel` | 材质透射、导入体积材质的厚度 |

例如 `vertex-colors: false, vertex-roughness-channel: "g"` 让 G 通道控制局部粗糙度，网格底色继续由材质和贴图决定。未提供 `COLOR_0` 的网格使用白色顶点值。改变遮罩时会复用 GPU 网格。 [顶点遮罩示例](../examples/puzzle3d/vertex_masks.xr)

#### 在 .xr 中定义实例表

在模型记录中声明 `instance-of: string` 和 `instance-template: bool`。原型可以是内置图元或导入的 glTF。实例行用 `instance-of: "shape"` 引用同一列表中唯一的原型 ID。原型行设为 `instance-template: true` 时只提供模型资源。`false` 或省略时，原型也能在自身的位置绘制。

实例继承原型选择的网格、材质、材质变体、动画、形变权重和 LOD。每行独立控制父节点、变换、颜色、可见性、图层、光照贴图、投影和交互标志。拾取返回实例自己的 `object-id` 和 `row-index`。若原型本身包含 glTF 实例组，也可以从 `mesh-node-index` 和 `instance-index` 定位组内的副本。缺失、重复或指向其他实例的原型引用会跳过该行的几何。导入资源的灯光和相机仍由资源的普通模型行挂载。

渲染器为引用同一原型的行共享已准备的材质和动画姿态，并批量提交可见实例。改变某行的位置或颜色会复用 GPU 网格。同样大小的实例缓冲可以更新连续的变化行，视锥裁剪后压紧的颜色批次也适用。大范围变化、行数变化或同一帧已使用该缓冲时，渲染器会准备独立的完整缓冲。Rust 的 `Renderer::scene_draw_stats()` 提供实例上传字节数、部分更新次数和缓冲分配次数。编译器将这类模型绑定作为绘制更新处理。VS Code 可以在同一列表的记录字面量里补全原型 ID。 [实例表和 LOD 示例](../examples/puzzle3d/instance_table.xr)

#### glTF GPU 实例化

带有 `EXT_mesh_gpu_instancing` 的 glTF 或 GLB 可以用同一个 `@mesh-url` 引入一组物体。节点上的 `TRANSLATION`、`ROTATION` 和 `SCALE` 属性分别提供每个实例的平移、四元数旋转和缩放，省略时使用单位变换。节点的动画或父级变换会作用于整组。该节点的子节点仍沿普通层级绘制一次。

实例共用网格和材质，在视锥内按可见批次提交。拾取和碰撞使用每个实例的变换。`hit3d.row-index` 指向所属的 `.xr` 模型记录，`mesh-node-index` 是 glTF 源节点索引，`instance-index` 是实例表中的零基索引。缺少对应身份或未命中时，相应字段为 `-1`。可以为整组绑定 `material-variant`，各实例共用选中的材质。资产的自定义实例属性以 `_` 开头，XRack 校验它们的访问器和数量。当前实例数据最多准备 4096 个模型位置。VS Code 悬停 `@mesh-url` 可查看实例组的源节点及实例数。 [GPU 实例化示例](../examples/puzzle3d/gpu_instancing.xr)

#### glTF 材质变体

带有 `KHR_materials_variants` 的模型可以在记录中声明 `material-variant: string`。例如，`mesh: @mesh-url("models/car.glb"), material-variant: root.finish` 可以根据应用状态切换整车涂装。选择值可以是唯一的变体名称或零基索引，例如 `"#0"`。留空、名称有歧义或找不到选择值时，沿用原始材质。变体中没有映射的图元也沿用各自的原始材质。

XRack 先应用 glTF 变体，再应用 `material` 和 `material-slots`。槽位名称或索引指向变体选中的 glTF 材质。每个变体使用自己的贴图、UV 和透明规则。着色和拾取共用该规则。布局兼容的材质变体会共用 GPU 顶点缓冲，使用不同 UV 或切线的变体则保留各自的顶点数据。将 `@mesh-url` 写在同一条记录时，VS Code 可以补全变体名称和索引。 [材质变体示例](../examples/puzzle3d/material_variants.xr)

#### 逐槽材质覆盖

给模型记录声明 `material-slots: Palette` 后，可以单独覆盖各个材质槽。调色板记录里的每个字段都是一个材质记录，可包含底色、粗糙度、贴图和透明模式等字段。

```xr
struct Finish {
    slot: string,
    base-color: color,
}
struct Palette {
    body: Finish,
    slot-0: Finish,
}
```

例如，`Palette { body: Finish { slot: "Body finish", base-color: #b27453 }, slot-0: Finish { slot: "", base-color: #8bb5cc } }` 会分别修改名为 `Body finish` 的槽和原文件的第 0 个材质槽。

| 选择方式 | 含义 |
| --- | --- |
| `slot: "Body finish"` | 按 glTF 原始材质名称精确匹配，同名槽都会应用。 |
| `slot: "#2"` | 按 glTF 的 `materials` 数组中的零基索引匹配。 |
| `slot: "#default"` | 选择源文件中未显式指定材质的图元。 |
| 省略或留空 `slot` | 使用调色板字段名。 `slot-2` 等价于 `"#2"`，`default-slot` 等价于 `"#default"`，其他名称按原名匹配。 |

若名称本身以 `#` 或 `name:` 开头，可加前缀 `name:` 按原名查找。内置图元的槽位见上表。每个覆盖记录可以通过 `enabled: false` 暂停。整体 `material` 先应用，随后按调色板字段名排序叠加匹配的覆盖，后应用的值覆盖前值。每个材质记录只覆盖它声明的因子和贴图。

原始槽身份随资源进入 AOT，并随自动 LOD 和光照 UV 预处理保留。材质变化继续复用几何缓冲。透明裁切会同步影响该槽的绘制、拾取和阴影。将 `@mesh-url` 写在同一个模型记录里时，VS Code 可以补全槽名称和索引。 [逐槽材质示例](../examples/puzzle3d/material_slots.xr)

#### 材质混合模式

材质记录可声明 `blend-mode: string`。默认的 `"source-over"` 按源 alpha 合成，`"screen"` 使用滤色提亮，`"multiply"` 使用正片叠底压暗。运算在线性颜色空间进行。滤色与叠底的源 RGB 限制在 0 到 1。

`alpha-mode` 独立决定覆盖率。可用 `"blend"` 保留材质和贴图的半透明，`"mask"` 保留通过阈值的片元，二者的拾取规则与绘制一致。使用滤色或叠底的网格参与透明排序和深度测试。当它们在视锥内时，同一视口的透明网格按深度合成，模型粒子仍可使用加权透明。编译器将修改混合方式标记为绘制更新，GPU 网格与实例数据可继续复用。 [材质混合示例](../examples/puzzle3d/material_blend.xr)

#### 材质面剔除

材质记录可以声明 `cull-mode: string`。`"back"` 剔除背面，`"front"` 剔除正面，`"none"` 保留双面。颜色、深度、阴影、运动向量、射线拾取和框选使用同一规则。背面命中返回朝向射线的法线。

省略该字段时，内置图元使用其默认模式，导入资产使用 glTF 的 `doubleSided` 和体积材质规则。模型槽位上的剔除设置按材质覆盖顺序生效。glTF 镜像变换保留网格的正面朝向。切换剔除模式属于绘制更新，可复用 GPU 网格。 [正反面示例](../examples/puzzle3d/material_cull.xr)

#### 材质深度写入

材质记录可声明 `depth-draw-mode: string`。四种模式都保留深度测试。

| 值 | 行为 |
| --- | --- |
| `"opaque-only"` | 默认值。不透明和裁切材质在颜色通道写深度。 |
| `"always"` | 在颜色通道为通过覆盖率和深度测试的片元写深度，包括半透明片元。可见的半透明材质使用深度排序。 |
| `"never"` | 颜色通道只做深度测试，该材质在阴影和环境遮蔽深度通道中跳过。 |
| `"opaque-prepass"` | 先用深度通道记录完全不透明的片元，再绘制颜色。裁切材质沿用其阈值。可用于有孔洞的半透明贴图及阴影，并支持 MSAA 和加权透明。 |

透射材质在抓取不透明背景之后执行自己的深度预通道。预通道只在有可见材质请求时提交。切换深度模式属于绘制更新，拾取继续依照几何、alpha 覆盖率和 `pickable`。 [深度写入示例](../examples/puzzle3d/material_depth.xr)

#### 材质贴图

`@texture-url("models/tile.png")` 生成 3D 贴图资源键。支持 PNG 和 JPEG，路径相对声明所在的 `.xr` 文件。Bazel 用 `xr_ui_library(assets = [...])` 声明资源。AOT 会嵌入预处理的像素和 mip 层，解释器也在加载场景资源时完成同样的处理。底色、发光和高光颜色作为 sRGB 采样，其余贴图作为线性数据采样。

在材质记录中可以声明下面任意一种贴图字段。每个字段可以直接接收 `string` 资源键，也可以使用具备 `source: string` 的自定义记录。省略字段时继承 glTF 上的贴图。空资源键清除该贴图。

| 材质字段 | 通道 |
| --- | --- |
| `base-color-map` | RGB 与底色相乘，alpha 参与透明裁切及混合。 |
| `metallic-roughness-map` | B 调节金属度，G 调节粗糙度。 |
| `emissive-map`、`occlusion-map` | 分别使用 RGB 发光和 R 环境遮蔽。 |
| `normal-map`、`clearcoat-normal-map` | RGB 切线空间法线，可分别设置 `normal-strength` 和 `clearcoat-normal-strength`。 |
| `clearcoat-map`、`clearcoat-roughness-map`、`transmission-map` | 分别使用 R 清漆强度、G 清漆粗糙度和 R 透射强度。 |
| `specular-map`、`specular-color-map` | 分别用线性 A 通道调节高光强度、用 sRGB 颜色调节介电高光颜色。 |
| `thickness-map` | 用线性 G 通道调节体积厚度。 |
| `anisotropy-map` | 线性 RG 表示拉丝方向，B 表示强度。 |

材质也可以为每个用途选取通道。字段类型为 `string`，`"r"`、`"g"`、`"b"`、`"a"` 分别选择 RGBA 通道，可让同一张贴图为多个材质参数提供数据。省略字段时沿用导入资产或默认通道。

| 通道字段 | 默认通道 |
| --- | --- |
| `base-color-channel`、`emissive-channel`、`specular-color-channel` | `"rgb"` |
| `metalness-channel`、`roughness-channel`、`occlusion-channel` | `"b"`、`"g"`、`"r"` |
| `specular-channel`、`transmission-channel`、`thickness-channel` | `"a"`、`"r"`、`"g"` |
| `clearcoat-channel`、`clearcoat-roughness-channel` | `"r"`、`"g"` |

颜色用途选单通道时，采样值复制到 RGB 三个分量。底色选单通道时，贴图的覆盖率按 1 处理，材质底色及顶点的 alpha 仍参与透明规则。深度、阴影和拾取遵循相同的覆盖率。高光和厚度的通道设置可用于声明式贴图和导入的 glTF 贴图。单独切换通道可复用已上传的图像、采样器和网格。 [材质通道示例](../examples/puzzle3d/material_channels.xr)

```xr
struct Sampler {
    source: string,
    wrap-u: string,
    wrap-v: string,
    uv-scale-u: float,
    uv-scale-v: float,
}
```

例如，声明 `base-color-map: Sampler` 后可以绑定 `Sampler { source: @texture-url("models/tile.png"), wrap-u: "repeat", wrap-v: "repeat", uv-scale-u: 2, uv-scale-v: 2 }`。

要把 2D 界面用作底色贴图，在同一窗口的可见 `Rectangle` 上写 `texture-source: "dashboard";`，再在场景材质记录中声明 `base-item: string`，并填入同一个名字。矩形的背景和子项会一起画进 GPU 纹理。多个材质可共享同一源。用 `unlit: true` 可保留面板配色，也可以交由 PBR 材质照明。底色使用 UV0；`alpha-mode: "blend"` 会合成源的透明区域。

镜头移动时复用已有贴图。源内容变化时重画到已有纹理，源尺寸变化时分配新的纹理。每窗最多保留 8 个贴图源，单张纹理边长上限为 2048 个设备像素，总预算为 16 MiB。隐藏、缺失或重名时使用材质原有底色贴图或底色。源子树要由 2D 内容组成。编辑器可补全当前组件里声明的名称。 [实时 2D 材质贴图示例](../examples/puzzle3d/item_texture.xr)

在来源矩形上设置 `texture-mipmaps: true;` 可为远景或斜视角启用 mip 过滤，默认关闭。GPU 在线性光中对预乘 alpha 的颜色做面积过滤，再按屏幕上的覆盖范围选级。源重新绘制时更新整条链，转相机和复用已缓存视口时沿用已有级别。普通贴图源优先使用显存预算，剩余空间用于可选的 mip 链。

只在 3D 面片上显示控件时，在来源矩形上设置 `texture-only: true;`。用正的 `width` 和 `height` 指定贴图的逻辑尺寸。矩形在独立坐标系中布局，退出父级排版、2D 绘制和窗口命中。仍可用 `visible: false;` 隐藏整个贴图源。启用输入的可见视口引用它时，原控件可接收焦点和键盘操作。多个面片共用的控件在无障碍树里保留同一个语义节点。

`SceneView3D.texture-input: true;` 会把 3D 面片上的鼠标或触摸接到原 2D 控件，默认关闭。路由沿用视口的相机、画幅、内容层和 LOD。最近的可见三角面从底色 UV 找到控件，支持按钮点按、滑块拖动和 `TouchArea.pointer-event` 的局部坐标。可见遮挡物会挡住转发，即使它自己不可拾取。同一贴图有多份面片时，一次按压始终绑定最初的面片。移动到另一份或移出面片会取消这次点击。控件的焦点、悬停和按压样式也进入贴图。

滚轮使用同样的面片拾取。源内的 `WheelHandler` 收到自身的局部 `x` 和 `y`，可以接收事件。事件继续分发时，开启滚轮调节的范围控件或最近的滚动容器按普通 2D 规则处理。滚动到边界后，余量先传给源内的外层容器，再传给 3D 视口在窗口中的父级滚动容器。

用触摸或鼠标中键拖动时，窗口坐标决定拖动阈值，面片的 UV 决定实际滚动距离。拖动和松手后的惯性可在贴图源的嵌套滚动容器之间传递。拖动过程中移出面片、遇到遮挡或移到同一贴图的另一块面片时，当前手势结束。

采样记录中的 `wrap-u` 和 `wrap-v` 可选 `"repeat"`、`"mirror"` 或 `"clamp"`。缩小、放大和 mip 过滤使用 `min-filter`、`mag-filter`、`mip-filter`，默认都为 `"linear"`，可选 `"nearest"`，mip 过滤还可设为 `"none"`。UV 先按 `uv-scale-u/v` 缩放，再绕 UV 原点按 `uv-rotation` 旋转，最后加 `uv-offset-u/v`。旋转以度计，缩放默认 1，偏移和旋转默认 0。底色使用 UV0，其余贴图可通过 `uv-set: int` 选择导入模型的 UV1。

内置盒体每个面都有完整 UV，球体使用经纬 UV。声明式法线贴图根据变换后的 UV 计算切线空间。绘制、阴影和拾取共享底色的 alpha 与采样参数。调整 UV 或材质因子时，原有 GPU 网格和贴图继续复用。 [贴图示例](../examples/puzzle3d/authored_texture.xr)

含动画的 glTF 可以用记录字段 `animation: string` 选择片段，`animation-time: float` 以秒采样，`animation-loop: bool` 控制循环。编辑器会为直接绑定到同一记录的 `@mesh-url` 补全片段名。片段可按名字选择，也可以用 `"#0"` 等原文件索引选择。空值或找不到片段时显示初始姿态。时间默认 0，循环默认关闭，超过片段末尾时停在末帧。可以把时间绑定到 `Timer` 驱动的属性，或在编辑器里直接定位播放位置。

同一资产的两个片段可以交叉过渡。用 `animation-to: string` 选择目标，`animation-to-time: float` 和 `animation-to-loop: bool` 控制它自己的时间和循环。`animation-blend: float` 默认为 0，限制在 0 到 1，0、1 分别取原片段和目标片段。把权重从 0 动画到 1 即可切换动作。缺少的轨道使用该节点的初始值，因此空选择器也可用于过渡到初始姿态。两个时间都先按各自轨道的插值规则采样，再混合节点的局部姿态。旋转沿四元数最短路径插值，手动 `morph-weights` 在混合后应用。

蒙皮、拾取、阴影和 glTF 内置相机与灯光共用混合姿态。权重在端点时只采样对应片段。过渡时 GPU 复用顶点数据。VS Code 在 `animation` 和 `animation-to` 字符串里提供片段名和原文件索引补全。 [动画过渡示例](../examples/puzzle3d/animation_blend.xr)

节点的平移、旋转和缩放支持 glTF 的 `STEP`、`LINEAR` 和 `CUBICSPLINE` 插值。不同帧复用同一份网格和材质，绘制、投影及拾取使用当帧的节点变换，拾取返回外层记录的 ID。 [glTF 动画示例](../examples/puzzle3d/README.md#gltf-节点动画) 可以暂停并切换片段。使用 Bazel 时，含动画的 `.glb` 或 `.gltf` 及外置资源与静态模型一样加入 `assets`。

同一套动画字段也适用于 glTF 骨骼蒙皮。每个蒙皮可用最多 128 个关节，每个顶点可通过 `JOINTS_0/WEIGHTS_0` 和 `JOINTS_1/WEIGHTS_1` 使用最多 8 个权重。关节层级和逆绑定矩阵在导入时检查，AOT 嵌入预处理后的数据。GPU 复用顶点、权重和关节缓冲，颜色通道和阴影通道共享蒙皮函数。CPU 根据关节变换更新包围盒，并在射线穿过包围盒时采样三角面做拾取。

glTF 的形变目标（morph targets）通过权重改变网格，适合表情、机关变形和网格之间的过渡。每个网格可用最多 32 个目标，支持位置、法线和切线的稀疏及密集增量。初始权重优先取节点的 `weights`，再取网格的 `weights`，都省略时使用零。动画用同一套 `animation`、`animation-time` 和 `animation-loop` 播放权重轨道，支持三种插值。形变在骨骼蒙皮前应用，颜色、阴影和拾取使用同一顺序。模型形变数据留在 GPU 上，播放时只更新变化的权重或关节矩阵。

要直接用界面或业务状态控制形变，在场景对象记录里添加 `morph-weights`，它的类型是只包含 `float` 字段的自定义记录。例如，让 `Actor` 声明 `morph-weights: PanelWeights`，再把形变绑定到组件属性。

```xr
struct PanelWeights {
    extend: float,
    target-0: float,
}
```

在使用 `Actor` 的记录字面量中写 `morph-weights: PanelWeights { extend: root.extension, target-0: 0.125 }`。字段 `extend` 对应导出器在 `mesh.extras.targetNames` 里写入的同名目标，`target-0` 对应第一个目标。每个网格按自己的目标顺序解析字段。`target-N` 保留作索引，每个场景对象实例都有自己的权重。同一个目标同时匹配名称和索引时，名称优先。未匹配的字段、超出范围的索引和非有限权重会跳过。模型没有足够明确的目标名时，可用从零开始的 `target-N`。在 VS Code 里悬停 `@mesh-url` 可以查看资源里各网格的索引和导出名称。

手动绑定的目标覆盖动画和初始权重。未绑定的目标继续由片段或初始权重决定，节点动画与蒙皮也可以同时播放。权重 0 表示该目标不参与，1 表示应用该目标的完整增量，也可以用负数或大于 1 的值来外推。 [形变滑块示例](../examples/puzzle3d/animation.xr) 可以暂停后交互调整 `extend`。

glTF 材质默认剔除背面。`doubleSided` 使背面参与绘制和拾取，背面光照和命中法线使用反向法线。导入时保留镜像节点的正面朝向。

使用 Bazel 时，把模型文件、外置二进制缓冲区和纹理都加入 `xr_ui_library(assets = [...])`。AOT 在构建时解析模型并嵌入三角数据和压缩像素，解释器在加载资源时解析。GPU 网格和纹理由引用同一资源的物体共享。

视口可以用 `camera: "overview"` 选择模型中的具名相机。相机行设置 `camera-kind: "perspective"` 或 `camera-kind: "orthographic"`，通常用 `mesh: ""` 声明为只提供变换的节点。相机沿局部 −Z 观察，可以随父节点移动、旋转和显隐。父节点的缩放会影响相机位置，镜头方向由局部和父节点的旋转决定。

透视相机使用 `camera-fov` 设定垂直视角，默认 45°。正交相机通过 `camera-height` 设定垂直覆盖的世界单位，默认 10。两个投影都用当前视口宽高比确定横向范围。可选的 `camera-look-at` 接收同一模型里的节点 ID，目标或相机移动时会重新朝向目标的世界原点。目标缺失、隐藏或与相机重合时沿用声明的旋转。可选的 `camera-near` 和 `camera-far` 裁剪距离默认分别为 0.1 和 100。空相机名使用 `camera-yaw`、`camera-pitch` 和 `camera-distance` 控制的绕视镜头。显式相机找不到、被隐藏或投影参数无效时，视口显示背景且拾取返回空字符串。

使用 `camera-kind: "frustum"` 可声明偏轴透视镜头。在相机记录中添加 `camera-left`、`camera-right`、`camera-bottom` 和 `camera-top` 四个 `float` 字段。它们是 `camera-near` 平面上相对相机光轴的局部坐标，单位与场景一致。每个轴的下界必须小于上界。省略某条边时使用 `±camera-near × tan(22.5°)`，正负号取决于边所在的一侧。四条边会映射到整个视口，独立决定横向和纵向的覆盖范围。一起平移同一轴的两条边可以移动构图，保留相机的位置和朝向。

投影、反投影、拾取、视锥裁剪、级联阴影、天空和屏幕空间环境遮蔽使用同一视锥。更改边界会更新受影响的视口，不会重新上传未变化的场景网格和实例缓冲。VS Code 在记录字面量的 `camera-kind` 字符串中补全 `frustum`。 [非对称视锥示例](../examples/puzzle3d/off_axis_camera.xr)

`frustum-culling` 默认开启。视口会根据当前相机、父子变换、蒙皮和形变后的包围盒，跳过完全在视锥外的颜色绘制。横跨裁剪面的实例会保留，阴影继续使用灯光自己的投影范围。刚性场景移动相机时会复用已有的空间索引和实例缓冲；如果一个批次的可见行之间有间隔，渲染器会把这些可见行集中提交，保留原有的绘制顺序和批次数。可设为 `false` 对照画面和提交开销。Rust 端的 `Renderer::scene_draw_stats()` 提供累计的颜色绘制调用、实例提交、裁剪和颜色专用实例缓冲上传次数。

同一程序内，共享同一个模型的视口、拾取和射线查询会复用准备好的 3D 场景。具名相机的投影查询也可以复用完整场景。单独改变视口镜头时，可见性和 LOD 按新镜头计算，已准备的层级和网格姿态继续使用。编辑、插入、删除和重排行会更新模型版本，后续查询使用新的行顺序。可通过 Rust 的 `update_profile::measure` 查看 `scene_projections_built` 和 `scene_projection_cache_hits`。

模型、灯光和探针记录可声明 `layers: int`，默认 `1`。这是一个 64 位掩码，`1` 和 `2` 代表前两层，`3` 同时包含它们，`-1` 表示全部位。对象各自设置层，父子关系仍负责变换和整体显隐。具名相机使用所在记录的 `layers`，绕视相机默认用第一层。`SceneView3D.layer-mask: int` 可为当前视口覆盖相机掩码。

视口会绘制与掩码有交集的对象，并从同一集合取阴影投射体及接收体。显式和导入的灯光也按视口掩码筛选。反射探针从模型和视口的共同层中匹配。拾取函数 `pick`、`hit`、`hit-all` 和具名相机版本都接受可选的尾部 `layer-mask` 参数。需要与视口覆盖值一致时传入同一绑定。`ray-hit` 和 `ray-hit-all` 也接受该参数，省略时使用第一层。

两个视口可以对同一模型使用不同掩码。掩码筛选保留原有模型行和 GPU 实例索引，与视锥裁剪共同缩小绘制集合。变化量可通过 `Renderer::scene_draw_stats().layer_filtered_instances` 查看。 [多视口内容层示例](../examples/puzzle3d/content_layers.xr)

`multisampling: true;` 为当前视口启用 4× MSAA，默认关闭。它平滑三角形轮廓，场景仍用同一组绘制调用。普通视口解析到 2D 合成用的纹理，HDR 视口先解析再做色调映射。阴影图和视口外的 2D 控件沿用各自的采样方式。重复渲染会复用同尺寸的采样附件，`Renderer::scene_draw_stats()` 的 `multisample_resolves` 记录累计解析次数。

`texture-width` 和 `texture-height` 可分别指定 3D 画幅离屏纹理的物理像素数，类型为 `int`。默认 0，非正值都按自动处理。自动轴跟随画幅的逻辑尺寸、DPI 和画布缩放。`render-scale` 只作用于自动轴，默认 1，范围为 0.125 到 1。例如两轴都自动时设为 `0.5`，光栅面积约为原先的四分之一。设定的纹理尺寸会受设备最大纹理尺寸和额外像素预算限制。

2D 合成直接将这张纹理映射到相机的逻辑画幅。布局、投影、拾取和 LOD 使用逻辑像素。景深会分别按两个轴的像素比例计算采样半径。开启 `supersampling` 时，会在选定的纹理尺寸上超采样，再解析回该尺寸。Rust 可通过 `Renderer::scene_draw_stats()` 读取最后一个视口的 `last_texture_size`、`last_raster_size` 和累计 `raster_pixels`。 [视口纹理示例](../examples/puzzle3d/texture_size.xr)

设置 `cache-rendering: true` 可以跨帧复用 `SceneView3D` 已完成的颜色纹理。刷新旁边的 2D 控件、平移视口或改变合成透明度时，视口可以直接用该纹理。相机、模型、灯光、效果或画幅发生变化时会重新渲染。尺寸和渲染输入相同的视口可以共用已完成的纹理。

该属性默认关闭。每个渲染器最多保留八张视口纹理，合计至多 32 MiB。可以通过 `Renderer::scene_draw_stats()` 的 `viewport_renders`、`viewport_cache_hits` 和 `viewport_cache_allocations` 分别查看渲染次数、缓存命中次数和缓存纹理分配次数。 [视口缓存示例](../examples/puzzle3d/viewport_cache.xr)

`supersampling: 1.5;` 将视口的内部渲染长宽放大 1.5 倍，再按像素覆盖面积在线性颜色空间缩回窗口尺寸。范围为 1 到 2，默认 1。放大生效时使用单样本的高分辨率附件。设备纹理尺寸限制和额外约 420 万光栅像素的预算会降低实际倍率。如果尺寸无法放大，`multisampling` 仍可按设置启用。

超采样会过滤几何边缘、纹理和着色细节，代价是更多的像素着色和附件显存。加权透明和景深在放大后的画面合成，泛光和色调映射使用缩回后的画面。布局、拾取和 LOD 继续用逻辑视口尺寸。未启用超采样时无需额外解析通道。`Renderer::scene_draw_stats()` 的 `supersample_resolves` 和 `supersample_pixels` 分别记录解析次数与处理的输入像素数。 [画质示例](../examples/puzzle3d/quality.xr)

设置 `progressive-aa: 8;` 后，每次画面变化先显示当前画面。视口稳定时，接下来的帧使用不同的亚像素偏移，并在线性颜色空间累积。可选值为 2、4、8，默认 0，其他值按关闭处理。相机、模型、灯光、反射探针或渲染尺寸变化会重新开始，拾取仍以原相机为准。可与 MSAA、SSAA 一起使用，累积在景深和超采样解析之后、泛光和色调映射之前进行。

原生窗口和 VS Code 预览会自动绘制所需帧。累积完成后释放历史纹理，在视口缓存预算内复用最终结果。`Renderer::scene_draw_stats().progressive_aa_samples` 记录实际累积的样本数。显存预算不足时沿用当前帧的渲染设置。 [画质示例](../examples/puzzle3d/quality.xr)

开启 `temporal-aa: true;` 后，视口在运动时交替偏移亚像素采样位置，并把当前帧与上一帧的原始场景颜色在线性空间等权混合。用 `temporal-aa-strength` 控制每轴偏移的像素量，默认 0.3，最大 1，非正值关闭时域滤波。画幅尺寸、具名相机、内容层、背景或环境切换会清掉历史。快速移动的物体可能残留一帧影像。

视口停稳后补一帧消掉运动历史，随后复用结果。若同时设置 `progressive-aa`，停稳后直接开始渐进累积。开启 `multisampling` 时暂停时域滤波。时域采样发生在景深和超采样解析之后、色调映射之前。AOT 和解释器共享调度，保留的两个原始颜色纹理受视口缓存预算约束。 [运动对照示例](../examples/puzzle3d/temporal_aa.xr)

使用 `temporal-aa-mode: "motion-vectors";` 可启用运动向量重投影，默认模式为 `"blend"`。运动通道用相机、实例的世界变换以及骨骼和 morph 的前后两帧姿态求出历史坐标。重投影检查物体身份和深度，并用当前邻域限制历史颜色，减少遮挡变化和快速运动时的拖影。天空、透明和透射覆盖区域使用当前帧颜色。

运动向量模式在静止时独立收敛八个亚像素样本，不进入渐进 AA 序列。运动通道仅在该模式启用且视口需要更新时执行，使用视口输出纹理尺寸，并保留两张身份贴图。可通过 `Renderer::scene_draw_stats()` 的 `motion_vector_draws` 和 `temporal_reprojections` 查看绘制及历史重投影次数。 [运动对照示例](../examples/puzzle3d/temporal_aa.xr)

开启 `screen-space-reflections: true;` 后，视口用不透明场景的颜色、深度和材质法线追踪反射。反射落在可见表面时按材质 BRDF 混合屏幕上的颜色，离屏、被挡住或缺少可信交点时保留天空或局部反射探针。反射接收器使用带 PBR 材质的不透明 glTF 网格，基础层和清漆层中选出较明显的一层做屏幕追踪。混合透明绘制在之后，透射材质可以折射已经合成的屏幕反射。

`ssr-strength` 控制屏幕结果的权重，默认 1。 `ssr-distance` 和 `ssr-thickness` 使用场景世界单位，默认分别为 24 和 0.2。 `ssr-steps` 是单条射线的采样上限，默认 64，可设 8 到 256。 `ssr-roughness-cutoff` 默认 0.65，更粗糙的表面沿用探针。 `ssr-scale` 控制追踪长宽相对于场景光栅的比例，默认 0.5，可设 0.25 到 1。低分辨率结果按深度和法线合成，避免跨越材质边缘。可与 4× MSAA、超采样和时域抗锯齿配合。与 MSAA 配合时，反射追踪读取最近深度，合成会按每个颜色样本的表面信息修正反射，因此几何边缘和后续透明混合仍使用各自的样本覆盖率。

HDR、景深及色调映射继续在反射和透明合成之后处理。可以通过 `Renderer::scene_draw_stats()` 的 `screen_reflection_resolves` 和 `screen_reflection_pixels` 检查实际工作量。 `opaque_depth_captures` 记录最近深度解析次数，屏幕反射和体积透射可以共用一次解析。 `cache-rendering` 可复用已经完成的静态视口。 [屏幕空间反射示例](../examples/puzzle3d/screen_reflections.xr)

开启 `screen-space-global-illumination: true;` 后，视口从可见不透明表面向半球追踪，估算一次漫反射反弹及环境遮蔽。着色器按接收材质的漫反射分量增加间接光，遮蔽仅扣减原有环境漫反射。镜面反射和直接照明分开合成。离屏和被遮挡的几何继续依靠场景原有的环境光。

`ssgi-strength` 默认 1，范围 0 到 8。设为 0 时只计算环境遮蔽。 `ssgi-occlusion` 默认 0.5，范围 0 到 1，大于 0 时接管环境遮蔽。设为 0 时可继续使用原有 `ambient-occlusion`。两者都为 0 时跳过全局光照。 `ssgi-radius` 和 `ssgi-thickness` 的默认值分别为 3 和 0.22，单位为场景世界单位。

`ssgi-rays` 默认 8，可设 2 到 32。 `ssgi-steps` 默认 24，可设 4 到 64。 `ssgi-scale` 默认为 0.5，可设 0.25 到 1，控制追踪和降噪纹理的长宽。合成按深度和法线保护材质边缘。屏幕反射和全局光照共用一次不透明材质绘制，4× MSAA 时逐样本合成。静态缓存继续适用，`Renderer::scene_draw_stats()` 可查询 `screen_indirect_resolves` 和 `screen_indirect_pixels`。 [间接光示例](../examples/puzzle3d/screen_indirect.xr)

静态模型记录可以用 `lightmap: image` 绑定预烘焙光照贴图，例如 `lightmap: @image-url("room-bake.png");`。贴图使用 sRGB PNG，RGB 表示经归一化的漫反射光照乘数，`lightmap-intensity: float` 在线性空间缩放，默认 1。网格用 glTF 的 `TEXCOORD_1` 做光照 UV，材质贴图继续使用它们各自的 UV 集。

`lightmap-mode: "indirect"` 用贴图替换运行时环境漫反射，仍计算实时灯的漫反射、阴影和镜面高光。`"diffuse"` 用贴图提供完整漫反射，实时灯仍提供视角相关的镜面高光。适用于不透明或带遮罩的静态 PBR 表面。缺少光照 UV 的三角形继续使用动态光照。启用 SSGI 时，烘焙表面可作为其他表面的光源，其自身继续使用已烘焙的漫反射。

同一张图集可供多个实例共享。`lightmap-u`、`lightmap-v`、`lightmap-width`、`lightmap-height` 是归一化的左上角坐标和长宽，默认 `0, 0, 1, 1`。采样限制在对应区域内，使用同一图集的相邻实例仍能合批。AOT 嵌入贴图和光照 UV，解释器从相同资源加载。示例展示手工绑定已有图集。 [光照图集示例](../examples/puzzle3d/lightmaps.xr)

缺少专用光照 UV 时，可以用 `@mesh-url("models/room.gltf", lightmap-uv)` 在静态 glTF 资源准备阶段展开光照图表。生成结果使用独立 UV 通道，原有材质 UV 和顶点顺序保留。图表沿可投影的相邻面展开，遇到重叠会拆分，并在最小烘焙分辨率上保留采样间距。与 `auto-lod` 一起使用时先生成光照 UV，再简化网格。AOT 嵌入生成结果，解释器在加载资源时生成一次。

准备好作者提供的 `TEXCOORD_1` 或生成的光照图表后，可用 `xrackc --emit lightmaps` 重新生成图集。工具从组件的初始状态读取 `SceneView3D`，与实时渲染共用节点变换、灯光层、遮罩和几何射线查询。它追踪点光、聚光和方向光的遮挡，并用确定性的半球路径估算环境遮蔽和漫反射反弹。间接光沿表面降噪，直接光的阴影边界保留。贴图中只存光照，运行时再乘材质漫反射颜色。

```sh
bazel run //crates/xrack-compiler:xrackc -- \
  "$PWD/examples/puzzle3d/lightmap_bake_scene.xr" --emit lightmaps \
  --bake-receiver plate --bake-receiver figure --bake-size 64 --bake-samples 48 --bake-bounces 1 \
  -o "$PWD/baked-room"
```

输出为 `atlas.png`、`lighting.xr` 和 `lightmaps.json`。从 `lighting.xr` 导入 `BakedLightmaps`，将 atlas、mode、intensity 和对象的 region 绑定到已有光照贴图字段。输出目录遵循包生成命令的约定，使用新路径。文件有多个可选组件或视口时，用 `--bake-component` 和 `--bake-viewport` 指定。设为 `--bake-mode indirect` 时保留实时直接漫反射。

目前 CPU 烘焙使用解析灯光和环境常量，支持不透明及遮罩网格。启用自动展开的网格会记录图表所需的最小烘焙分辨率。图集生成、AOT 和加载同一图集的解释器路径可在烘焙房间示例中比较。 [源场景](../examples/puzzle3d/lightmap_bake_scene.xr) · [生成图集的使用方式](../examples/puzzle3d/lightmap_baked_view.xr)

`specular-aa: true;` 缓和镜头移动时镜面高光的闪烁，默认关闭。着色器根据相邻片元的法线变化调宽 GGX 高光，在 PBR 基材和清漆层分别计算，方向光、点光、聚光灯的高光以及环境探针反射共用过滤后的粗糙度。法线贴图也参与计算。它只使用当前帧，和 MSAA、超采样及透明合成可以一起使用。高光可能会变宽，视口分辨率和绘制通道与开关关闭时相同。 [镜面高光示例](../examples/puzzle3d/specular_aa.xr)

glTF 材质从采样器读取消除缩小锯齿的过滤方式。显式 `NEAREST` 和 `LINEAR` 只使用原图。四个 `*_MIPMAP_*` 模式分别控制每级的采样，以及级与级之间用最近点还是线性混合。省略 `minFilter` 时用三线性过滤。颜色和发光贴图在线性光空间缩小，法线和材质通道按线性数据缩小，并包含奇数尺寸的边缘像素。

AOT 在构建期生成并嵌入需要的 mip 链，解释器在准备资源时使用同一套算法。一个纹理用于多处时复用同类 mip 链和 GPU 纹理。镜头移动时选择已有的级别，`Renderer::scene_draw_stats().material_mip_levels_uploaded` 可检查上传次数。VS Code 悬停 `@mesh-url` 可查看资源的采样器设定。GPU 以各通道的屏幕覆盖范围采样，几何射线以原图 alpha 测试命中，因此小于一个屏幕像素的透明细节可能出现差别。 [纹理缩小示例](../examples/puzzle3d/texture_mips.xr)

glTF 的 `KHR_texture_transform` 可对同一材质的每张贴图单独设置偏移、旋转、缩放和输入 UV 通道。每个材质最多使用两组源 UV。变换按缩放、旋转、平移的顺序应用，之后才做重复、镜像或夹取。AOT 和解释器在准备资源时计算变换矩阵，渲染、遮罩深度和几何拾取用同一套 UV 映射。法线贴图也沿变换后的坐标建立切线方向。镜头移动时 mip 级别按变换后的纹理覆盖范围选择。在 VS Code 中悬停 `@mesh-url` 可看到模型声明的扩展。 [UV 图集示例](../examples/puzzle3d/uv_atlas.xr)

glTF 的 `KHR_materials_ior` 和 `KHR_materials_specular` 控制介电基材的镜面反射。折射率默认 1.5，正视反射率为 4%。合法值从 1 起，0 表示 glTF 的无限折射率模式。`specularFactor` 和强度贴图的 alpha 通道相乘，`specularColorFactor` 与 sRGB 颜色贴图相乘。两张贴图可以设置独立的 UV 变换。

直接光和 HDR 环境探针共用镜面反射参数。色彩变化也会改变留给漫反射的能量。纯金属使用自身颜色确定反射，清漆层独立计算。AOT 和解释器共用材质数据，VS Code 悬停 `@mesh-url` 会列出这两个扩展。 [镜面反射示例](../examples/puzzle3d/dielectric.xr)

glTF 的 `KHR_materials_transmission` 支持薄壁玻璃。透射强度乘以贴图的线性 R 通道，基材颜色给透出的画面着色，粗糙度控制模糊程度。`alphaMode` 独立控制表面覆盖，透明裁剪孔仍显示原背景。默认的 `OPAQUE` 也可透射。

视口从不透明画面读取背景，玻璃按屏幕位置采样。多层玻璃共用该背景，阴影通道跳过透射面，体积折射和有色透射阴影属于独立能力。有可见玻璃时才保存背景，纯光滑玻璃直接采样原分辨率，需要模糊时才生成低分辨率层。支持 HDR、MSAA、超采样和加权透明合成。开发者可用 `transmission_captures` 和 `transmission_mip_passes` 查看这部分工作量。VS Code 悬停 `@mesh-url` 可查看扩展。 [薄壁透射示例](../examples/puzzle3d/transmission.xr)

厚度和体积吸收可以直接写在 `.xr` 材质记录里。例如 `transmission: 1, thickness: 1.6, attenuation-distance: 1, attenuation-color: #ed804a`。正厚度启用体积折射，设为 0 时回到薄壁。声明式厚度使用 XRack 模型的局部坐标，即 glTF 静态节点变换应用后、场景实例缩放前的坐标。`attenuation-distance` 使用世界单位，设为 0 表示无限距离。正 `dispersion` 可在体积透射中分离颜色。 [光学材质示例](../examples/puzzle3d/material_optics.xr)

导入带厚度的玻璃使用 glTF 的 `KHR_materials_volume`。`thicknessFactor` 以网格坐标为单位，并乘以厚度贴图的线性 G 通道。`attenuationDistance` 以世界坐标为单位，省略时表示无限距离。`attenuationColor` 是光走过该距离后留下的线性颜色。渲染器在入射面折射，使用模型缩放计算光路长度，再按 Beer–Lambert 定律逐通道吸收。`thicknessFactor: 0` 回到薄壁模型。

CPU 在资源编译时保留已烘焙 glTF 节点的逆变换，GPU 与实例变换组合。标准材质仍走不含体积计算的管线。体积从正面入射，并用估算的出射点采样视口的不透明背景。出射点落到视口外、位于不透明前景之后或缺少可用深度时，回退到入射点采样。只有可见体积材质需要额外保存不透明深度，可通过 `volume_depth_captures` 查看次数。VS Code 悬停 `@mesh-url` 可查看扩展。 [体积吸收示例](../examples/puzzle3d/volume.xr)

glTF 的 `KHR_materials_dispersion` 可在体积折射上增加色散。系统从基础折射率和 `dispersion` 值计算 RGB 三条光路，按各自的出射位置采样背景并计算吸收。深度遮挡和离屏回退对每条光路分别判断。三个通道共用同一份背景、深度和模糊层。零厚度、零色散和 glTF 的无限折射率模式会用单条光路。示例采用夸大的色散值，便于在黑白条纹上看清颜色分离。 [色散示例](../examples/puzzle3d/dispersion.xr)

`SceneView3D` 在材质着色阶段计算距离雾和高度雾。`fog-density` 控制整体浓度，范围 0 到 1，默认 0。`fog-color` 默认取 `scene-background`。雾色按线性颜色混合，物体的透明度、天空和拾取沿用各自的规则。

- `fog-depth-enabled` 默认开启。`fog-near` 和 `fog-far` 按相机前向深度定义雾由浅到浓的范围，默认 0 和 100。`fog-depth-curve` 默认 1，越大则雾越集中到远端。远端小于或等于近端时关闭距离雾。
- `fog-height-enabled` 默认关闭。`fog-height-start` 和 `fog-height-end` 分别设世界 Y 轴上由清晰到最浓的高度，默认 10 和 0。互换高度即可反转方向，相同高度时关闭该层。`fog-height-curve` 默认 1，调节两端之间的平滑过渡。
- `fog-transmit-enabled` 默认关闭。距离雾生效时，打开它会让亮于雾色的分量从近处保留到雾区中，远端仍接近雾色。`fog-transmit-curve` 越大，亮部保留得越远，默认 1。三个曲线的有效范围为 0.01 到 16，非正值按默认值处理。

距离雾和高度雾同时打开时，取较强的一层再乘以密度。运行时更新雾参数只使视口颜色失效，网格和实例缓冲继续复用。雾计算在原有颜色通道中完成。 [山谷晨雾示例](../examples/puzzle3d/fog.xr)

`fog-volumes` 接收局部体积雾的记录列表。每条记录需要 `position: point3d`、`size: vector3d`、`density: float` 和 `color: color`。位置是世界坐标中的中心。尺寸是三个轴的完整宽度。可选的 `fog-shape: string` 默认为 `"box"`，`"sphere"` 随三轴尺寸可形成椭球。可用 `visible: bool` 控制显隐，`layers: int` 默认 1，沿用视口的内容掩码。

雾体记录可用 `yaw`、`pitch` 和 `roll` 旋转盒形或椭球边界。旋转顺序与场景物体一致，三个值默认 0，单位为度。遮挡积分在旋转后的区域内进行，视锥筛选和阴影按旋转后的世界范围计算。

记录内的 `fog-height-enabled: true` 启用局部密度过渡。`fog-height-start` 是密度为零的世界 Y，`fog-height-end` 是达到记录密度的世界 Y，默认分别为 10 和 0。两点之间线性过渡，互换高度可倒置雾层。两点相同会采用均匀密度。高度沿世界 Y 计算，可以独立旋转雾体边界。 [旋转与高度衰减示例](../examples/puzzle3d/fog_profiles.xr)

浓度对应每世界单位的消光系数，大于 64 时按 64 处理。透过的背景比例为 `exp(-浓度 × 穿过的距离)`。颜色按线性空间解释为雾体散射的颜色。视口会筛掉掩码和视锥外的记录，最多处理前八个有效雾体。零浓度、无效尺寸和未知形状会跳过。

每条视线按从近到远混合遇到的雾体。重叠处相加消光并按浓度混合颜色。相机裁剪面和模型表面截断积分，因此空背景也能显示雾体，雾体前方的物体保持清晰。普通透明继续做覆盖率混合，屏幕空间折射复用已有背景采样。与材质雾并用时，先对表面着色，再算相机与表面之间的体积雾。

雾体消光按均匀或线性变化的密度解析积分。不同颜色的高度层重叠时，每段以双点积分近似颜色。开启局部雾时复用已有颜色通道，并绘制支持雾效的天空或背景。没有可见雾体时使用普通着色管线。雾记录按绘制属性编译，调整位置或密度可复用网格和实例缓冲。 [局部体积雾示例](../examples/puzzle3d/volumetric_fog.xr)

用 `fog-volume-scattering` 开启场景灯光的单次散射，默认 0，范围 0 到 16。雾色同时给散射光着色。方向光、点光源和聚光灯使用视口当前灯光列表，沿用灯光强度、距离衰减、聚光锥和内容层。开启 `shadows` 时，雾中的采样点查询同一份场景阴影图。移动光源或遮挡物会改变雾里的光斑和阴影。

`fog-volume-anisotropy` 默认为 0，范围 −0.85 到 0.85。正值增强沿入射光方向的散射，负值增强反向散射。`fog-volume-samples` 为每条视线的采样数，默认 8，有效值 1 到 16。采样按整条视线上被雾衰减的光量分配，随雾体重叠仍共用同一预算。消光和恒定雾色仍用解析积分，采样只估算随空间变化的直接散射光。调节浓度、散射强度和采样数时可复用既有阴影图。 [体积光与雾影示例](../examples/puzzle3d/fog_lighting.xr)

在雾体记录中设置 `fog-noise-amount`（0 到 1，默认 0）可调节三维密度变化的强度。`fog-noise-scale` 是噪声格子的世界单位长度，默认 0.8，有效范围 0.05 到 1000。`fog-noise-offset: vector3d` 默认是零向量，沿世界坐标移动噪声。绑定到计时器即可让雾团随风移动。噪声会和高度衰减相乘，在旋转后的雾体内部采样，周期为格子长度的 256 倍。

`SceneView3D.fog-volume-density-samples` 控制噪声密度积分，默认 32，有效值 8 到 64。采样只分配给视线穿过的雾段，每段至少一次，重叠雾共用预算。步长较大时会减弱高频细节。开启受光时，光照继续使用独立的 `fog-volume-samples` 预算。关闭噪声时，消光沿用均匀密度或高度线性过渡的解析积分；调整偏移可复用网格、实例数据和同一份阴影图。 [流动体积雾示例](../examples/puzzle3d/fog_noise.xr)

`ambient-occlusion: true;` 启用屏幕空间环境遮蔽，默认关闭。`ao-strength` 控制强度，范围 0 到 1，默认 0.8。`ao-distance` 是采样半径，默认 1 个世界单位。`ao-bias` 以世界单位抑制小尺度的自遮蔽，默认 0.025。遮蔽调暗接触区域的环境光和环境反射，直射光、自发光、天空和拾取保持原有规则。普通实体和 Alpha Mask 参与深度计算，`BLEND` 继续按自己的透明规则合成。屏幕空间估算使用可见深度，画面外和被遮住的几何只能通过场景原有的灯光与阴影体现。

开启时，视口使用长宽减半的深度和遮蔽纹理，并按深度差插值，减少轮廓处的串色。灯光和强度变化复用遮蔽结果，半径或偏移变化复用相机深度。强度为 0、半径不大于偏移、没有环境光或没有可见接收面时跳过两个通道。支持透视、正交、HDR、MSAA 与加权透明。 [环境遮蔽示例](../examples/puzzle3d/occlusion.xr)

`scene-hdr: true;` 可以在没有环境探针时使用线性 HDR 渲染和色调映射。`scene-exposure` 默认 1，调节色调映射前的曝光。使用环境探针或开启有效的泛光时也会自动通过 HDR 通道。

`scene-tone-map` 接受 `SceneToneMap` 枚举，显式绑定会开启 HDR。默认 HDR 曲线是 `reinhard`，按最大通道压缩亮部以保留颜色比例。`linear` 应用曝光并裁到显示范围。`none` 直接输出场景颜色，跳过曝光和曲线。`aces` 使用 ACES 风格的拟合曲线，`hejl-dawson` 和 `filmic` 使用各自的电影曲线。`scene-white-point` 默认 11.2，只用于归一化 `filmic` 的白点。

`scene-brightness`、`scene-contrast` 和 `scene-saturation` 的范围都是 0 到 4，默认 1。它们依次调整最终显示颜色的亮度、以 18% 线性灰为中心的对比度，以及按 Rec. 709 亮度计算的饱和度。饱和度为 0 时输出灰度。HDR 视口在色调映射通道内完成调整，LDR 视口保持原有色彩空间。 [色调与颜色调整示例](../examples/puzzle3d/tone.xr)

`scene-lut: @image-url("models/grade.png");` 可为 HDR 或 LDR 视口添加调色表。使用宽 `N²`、高 `N` 的静态 PNG 横条，`N` 可取 2 到 64。每块内红色由左到右增加，绿色由上到下增加，蓝色从第一块到最后一块增加。`scene-lut-size` 默认为 0，从高度推导；显式值必须匹配 PNG。直接绑定的文件在处理资源时检查格式和尺寸，运行时动态提供的无效图片会沿用原画面。

`scene-lut-strength` 默认 1，范围 0 到 1，用于在原色与查表后的 sRGB 显示颜色间混合。查表在泛光、色调映射和颜色调整之后进行，用三线性插值平滑过渡。重复切换已缓存的图片或调整混合强度会复用 GPU 纹理，强度为 0 时跳过查表。 [LUT 调色示例](../examples/puzzle3d/lut.xr)

**自定义后处理**

`scene-effect` 支持为一个 3D 视口运行全屏 WGSL 效果。用 `@shader-url` 引用相对于当前 `.xr` 文件的本地文件。编译器校验 WGSL 接口，构建时将源码嵌入应用，VS Code 预览会在着色器保存后刷新。Bazel 应用在 `xr_ui_library(assets = [...])` 中声明该文件。

```xr
SceneView3D {
    scene-effect: @shader-url("effects/hologram.wgsl");
    scene-effect-depth: true;
    scene-effect-strength: 0.75;
}
```

定义 `fn scene_effect(uv: vec2<f32>) -> vec4<f32>`。输入 UV 的原点在视口左上角，返回直通 alpha 的线性 RGBA。颜色在抗锯齿之后、泛光和色调映射之前处理，混合时保留透明度。效果可调用以下函数。

| 函数 | 数据 |
| --- | --- |
| `scene_color(uv)` | 线性 RGBA，可读取相邻像素。 |
| `scene_depth(uv)` | 深度缓冲中的归一化深度。近端为 0，远端和空白为 1。开启 `scene-effect-depth` 后保留深度，MSAA 下取最近的采样。 |
| `scene_surface(uv)` | 返回 `XrEffectSurface`，包含最近不透明表面的世界坐标 `position`、世界空间 `normal`、`roughness`、原始 `depth` 和 `valid`。 |
| `scene_size()` | 效果输出纹理的像素宽高。 |
| `scene_input_size()` | 当前通道读入的颜色纹理像素宽高。 |
| `scene_original_color(uv)` | 自定义效果开始前的线性 RGBA。 |
| `scene_effect_parameters()` | 当前通道的三个浮点参数，默认全为 0。 |

未申请几何数据时，`scene_depth` 返回 1。`scene-effect-strength` 默认是 1，取值限制在 0 到 1，设为 0 时跳过效果。相同着色器和深度模式共享 GPU 管线。 [全息轮廓示例](../examples/puzzle3d/custom_effect.xr) · [WGSL](../examples/puzzle3d/effects/hologram.wgsl)

编译器从 `scene_effect` 出发检查 WGSL 调用链。调用 `scene_surface` 的效果共享一遍按需执行的表面捕获。该缓冲用当前相机在渲染栅格上单采样，屏蔽透明、透射和禁用深度的材质，保留 Alpha 裁切。法线和粗糙度来自材质求值，深度包含蒙皮、形变和顶点程序的结果。同一通道调用 `scene_depth` 时读取匹配的捕获深度。效果缩小分辨率后仍通过视口 UV 读取表面。无表面时 `valid` 为 false、深度为 1。 [表面检查示例](../examples/puzzle3d/effect_surface.xr) · [WGSL 检查函数](../examples/puzzle3d/effects/effect_surface_inspect.wgsl)

`scene-effects` 接收记录列表，按列表顺序执行。首通道的 `scene_color` 读取场景，后续通道读取上一个实际执行的输出。记录名可自定义，`shader: string` 必填。可选的 `strength: float` 和 `depth: bool` 对应单效果的混合强度和深度开关，`parameters: vector3d` 将绑定值传给 WGSL。`scale: float` 默认为 1，限制在 0.25 到 1，按视口纹理宽高缩放并向上取整。强度为 0 的记录会跳过。视口绑定 `scene-effects` 时，以此列表决定执行内容。

```xr
struct BlurPass { shader: string, scale: float, parameters: vector3d }

export component FilteredView inherits SceneView3D {
    scene-effects: [
        BlurPass { shader: @shader-url("effects/blur.wgsl"), scale: 0.5, parameters: Vector3D.new(8, 0, 0) },
        BlurPass { shader: @shader-url("effects/blur.wgsl"), scale: 0.5, parameters: Vector3D.new(0, 4, 0) },
        BlurPass { shader: @shader-url("effects/halo.wgsl"), scale: 1, parameters: Vector3D.new(0, 0, 0) }
    ];
}
```

同尺寸的连续通道交替复用中间纹理。最后一个通道以低分辨率输出时，最终合成会做线性采样。示例先用两张半分辨率纹理做横向、纵向模糊，再合回清晰的原始场景。 [多阶段效果示例](../examples/puzzle3d/effect_chain.xr)

`output: string` 可以为当前通道的颜色纹理命名。`input: string` 指定 `scene_color(uv)` 和 `scene_input_size()` 使用的主输入。空名字沿用上一步，指定的名字没有可用输出时使用原始场景。通过 `buffers: [BufferInput]` 可以为着色器额外提供中间纹理，`BufferInput` 是任意带 `name: string` 字段的记录。着色器按列表下标从 0 开始访问前 8 个绑定。

```xr
struct BufferInput { name: string }
struct Pass { shader: string, input: string, output: string, buffers: [BufferInput] }

export component NamedView inherits SceneView3D {
    scene-effects: [
        Pass { shader: @shader-url("effects/grade.wgsl"), input: "", output: "accent", buffers: [] },
        Pass { shader: @shader-url("effects/blur.wgsl"), input: "accent", output: "soft", buffers: [] },
        Pass {
            shader: @shader-url("effects/named_composite.wgsl"), input: "accent", output: "",
            buffers: [BufferInput { name: "soft" }, BufferInput { name: "accent" }]
        }
    ];
}
```

WGSL 通过 `scene_buffer_color(index, uv)` 读取颜色，通过 `scene_buffer_size(index)` 查询物理像素尺寸，通过 `scene_has_buffer(index)` 判断纹理是否可用。缺失的额外输入返回零尺寸和透明色。默认读取当前帧较早通道的输出。同名输出更新该名字，合成前会根据最后一次读取保留所需的纹理，之后可复用。 [命名纹理示例](../examples/puzzle3d/named_effect_buffers.xr)

**纹理格式与采样**

通道的 `output-format: string` 选择输出格式，默认 `"rgba16f"`。颜色和混合使用线性 RGBA。单通道和双通道适合遮罩、距离场和其他数据，分别把 WGSL 返回值的 `r` 和 `rg` 原样写入可用通道，采样时缺少的颜色分量为 0，alpha 为 1。把数据纹理用于可见合成时，可以在最后一个通道转换成 RGBA。

| 格式 | 每像素数据 | 用途 |
| --- | --- | --- |
| `r8`、`rg8` | 1 或 2 字节 | 归一化到 0 至 1 的遮罩和数据 |
| `r16f`、`rg16f` | 2 或 4 字节 | 可保存负值和超出 1 的浮点数据 |
| `rgba8`、`rgba16f` | 4 或 8 字节 | 线性颜色与 alpha。`rgba16f` 保留 HDR 高光 |

额外输入记录可带 `filter: string`、`wrap-u: string` 和 `wrap-v: string`。过滤可选 `"linear"` 或 `"nearest"`，两个方向的寻址各自可选 `"clamp"`、`"repeat"` 或 `"mirror"`。默认是线性过滤和边缘扩展。采样设置属于读取方，同一张纹理在一个通道中绑定两次时可以分别设置。改变采样选项会复用已分配的纹理和着色器管线。 [采样对照示例](../examples/puzzle3d/effect_buffer_formats.xr)

**文件贴图和实时二维输入**

`buffers` 的记录可以通过 `texture: string` 绑定 `@texture-url` 读入的 PNG 或 JPEG，也可以通过 `item: string` 引用当前窗口里声明了 `texture-source` 的二维矩形。文件贴图用 `color-space: "srgb"`（默认）解释颜色，也可用 `"linear"` 按线性数据读取。在 Bazel 中将文件加入 `xr_ui_library` 的资源清单。

```xr
struct TextureInput { texture: string, item: string, color-space: string, enabled: bool }
struct Effect { shader: string, buffers: [TextureInput] }

export component BufferView inherits SceneView3D {
    scene-effects: [
        Effect {
            shader: @shader-url("effects/texture_inputs.wgsl"),
            buffers: [
                TextureInput { texture: @texture-url("models/channel-swatch.png"), item: "",
                    color-space: "linear", enabled: true },
                TextureInput { texture: "", item: "live-swatch", color-space: "", enabled: true }
            ]
        }
    ];
}
```

一个输入选用非空的 `capture`、`texture`、`item` 或 `name`，优先级按此顺序。可选的 `enabled: bool` 默认开启，关闭或资源缺失时按缺失输入处理。WGSL 沿用相同的 `scene_buffer_color` 和 `scene_has_buffer`，读取结果为线性 RGB 和直通 alpha。文件贴图可以与模型材质共享 GPU 上传。二维输入沿用 `texture-source` 的离屏捕获与缓存，源内容更新会使引用它的视口重绘。 [文件与二维输入示例](../examples/puzzle3d/effect_texture_inputs.xr)

**命名场景捕获**

`SceneView3D.scene-captures` 接受记录列表。每条记录用 `name: string` 命名，可选 `camera: string`、`layers: int`、`scale: float`、`background: color` 和 `enabled: bool`。相机默认使用当前视口相机。指定相机时可以使用模型 ID 或导入相机路径。图层默认使用所选相机的掩码，也可以显式选取主视口以外的图层。`scale` 默认 1，范围为 0.25 到 1。固定宽高比的相机按有效画幅输出。背景默认透明，`enabled` 默认开启。

```xr
struct Feed { name: string, camera: string, layers: int, scale: float }
struct FeedInput { capture: string }
struct FeedEffect { shader: string, buffers: [FeedInput] }

export component CameraFeedView inherits SceneView3D {
    scene-captures: [Feed { name: "detail", camera: "side-eye", layers: 2, scale: 0.5 }];
    scene-effects: [
        FeedEffect {
            shader: @shader-url("effects/scene_capture_display.wgsl"),
            buffers: [FeedInput { capture: "detail" }]
        }
    ];
}
```

捕获以单采样渲染线性 HDR 颜色，采用所选相机下的材质、光照和透明合成。后处理通过 `scene_buffer_color`、`scene_buffer_size` 和 `scene_has_buffer` 读取画面。禁用的捕获、缺失的名称或找不到的相机返回缺失输入。仅活跃通道引用的画面发出绘制，相同相机、图层、尺寸和背景的画面共用纹理。捕获输入稳定时，改变后处理参数可以复用上一帧的画面。 [多相机示例](../examples/puzzle3d/scene_capture.xr)

同一个捕获可供材质与后处理读取。材质在 `shader-textures` 的记录里设置 `capture`，WGSL 用 `material_has_texture`、`material_texture_size` 和 `material_texture` 查询和采样。运行时按参与图层、所选 LOD 和可用纹理槽位解析材质依赖，先渲染生产方，再渲染依赖它的捕获。配置相同的别名共享一张纹理。捕获内容更新时可以保留分配和材质绑定，读取它的下游捕获按内容版本失效。依赖环内的输入按缺失纹理处理，包括捕获自己的别名。场景反射探针也可以读取这些纹理。 [材质与后处理共享示例](../examples/puzzle3d/capture_material.xr)

捕获输入可用 `attachment: string` 选附件，默认是 `"color"`。材质和后处理用各自的现有纹理查询函数读取。

| 附件 | 数据 |
| --- | --- |
| `color` | 线性 HDR 颜色和 alpha。 |
| `depth` | `R32Float` 的 R 通道保存相机原始深度，近端为 0，远端和背景为 1。使用最近邻取样。 |
| `normal-roughness` | `RGBA16Float` 的 RGB 保存带符号的世界空间法线，A 保存材质粗糙度。背景为零向量，数据使用直通编码。 |
| `base-color-metalness` | `RGBA16Float` 的 RGB 保存直通的线性基础色，A 保存金属度。 |
| `emissive-occlusion` | `RGBA16Float` 的 RGB 保存线性 HDR 发光，A 保存材质遮蔽系数。无材质遮蔽贴图时表面的 A 为 1。 |

同一相机、图层和尺寸的几何附件在一遍绘制里输出，按实际读取分配纹理。光照后的 `color` 由单独阶段生成。几何阶段遵守 Alpha 裁切，采集最近的写深度不透明表面，应用网格形变、纹理通道和 `scene_material` 修改。利用几何附件实现延迟光照时，只移动后处理的光源不会使几何捕获失效。材质在读取其他捕获附件时也参与依赖排序。 [深度与法线示例](../examples/puzzle3d/capture_attachments.xr) · [自定义延迟光照](../examples/puzzle3d/capture_material_channels.xr)

从 `depth` 附件重建世界坐标时，后处理可调用 `scene_buffer_world_position(index, uv)`，材质片元函数可调用 `material_texture_world_position(index, uv)`。两者都返回 `XrCapturePosition` 的 `position: vec3<f32>` 与 `valid: bool`。引擎用该深度附件的相机矩阵和所采像素的中心执行反投影，支持正交和透视，包括偏轴视锥。远端背景、缺失输入、无反矩阵的相机和其他附件类型会返回无效位置。只有调用链使用重建函数时才为程序绑定相机矩阵。相同投影表可在材质和后处理之间复用。 [双相机反投影示例](../examples/puzzle3d/capture_positions.xr)

探针需要的命名画面会在立方体捕获前准备，后处理可以读取同一份输出。这个阶段使用静态环境光和全景探针，场景立方体在之后发布。`on-change` 探针追踪这些输入的内容版本，`first-frame` 使用已提交的立方体，直到 `capture-revision` 等捕获设置更新。读实时画面的探针会在同一帧绘制六个方向，分帧配置将预滤波分散到后续帧。所有面完成后一起发布。 [探针与画面依赖示例](../examples/puzzle3d/probe_capture_feed.xr)

**在主视口合成捕获颜色与深度**

`scene-color-capture: string` 和 `scene-depth-capture: string` 分别引用同一 `SceneView3D` 的 `scene-captures` 名称。两项可以独立设置，默认空字符串。颜色和深度在主视口的不透明几何通道里通过一次全屏绘制载入。主视口后续的不透明、透明和粒子绘制用同一个深度缓冲。

```xr
SceneView3D {
    camera: "eye";
    layer-mask: 1;
    scene-captures: [
        Feed { name: "surface", camera: "eye", layers: 2 },
        Feed { name: "lighting", camera: "eye", layers: 4 },
    ];
    scene-color-capture: "lighting";
    scene-depth-capture: "surface";
}
```

例子把表面数据放在图层 2，让图层 4 的材质从中计算延迟光照，再用图层 1 绘制前向玻璃。导入的深度按像素参与硬件深度测试，落在捕获表面后方的玻璃会被遮挡。没有声明 `scene-depth-capture` 时会单独合成颜色。缺失或禁用的捕获会跳过，相机投影不一致时跳过深度导入。需要对齐遮挡边缘时，颜色与深度捕获应使用相同相机和分辨率。 [延迟光照与前向透明混合示例](../examples/puzzle3d/capture_composite.xr)

**串联命名颜色捕获**

`scene-captures` 的记录还可以声明 `color-source: string` 和 `depth-source: string`，两项都可以留空。颜色捕获先载入指定的上游颜色和深度，再按当前记录的 `layers` 绘制自己的几何。深度源的投影必须和消费它的捕获一致，才能参与硬件深度测试。几何附件仍按各自的相机和图层生成，供颜色通道选用。

```xr
struct Feed {
    name: string, camera: string, layers: int, color-source: string, depth-source: string,
}

SceneView3D {
    camera: "eye";
    layer-mask: 0;
    scene-captures: [
        Feed { name: "forward", camera: "eye", layers: 1,
            color-source: "lighting", depth-source: "surface" },
        Feed { name: "lighting", camera: "eye", layers: 4, color-source: "", depth-source: "" },
        Feed { name: "surface", camera: "eye", layers: 2, color-source: "", depth-source: "" },
    ];
    scene-color-capture: "forward";
}
```

引擎根据引用确定执行顺序，只生成可达的附件。循环内部的引用按缺失输入处理，投影不匹配的深度也会跳过。渲染配置和已解析的输入相同的捕获别名可共用输出。上游内容更新时，下游通过内容版本重新绘制，可沿用已有纹理分配。 [离屏合成示例](../examples/puzzle3d/capture_composite.xr)

**过滤捕获阶段的几何**

捕获记录可用 `renderables: string` 与 `layers` 一起选择要绘制的内容。默认 `"all"` 绘制全部类型。`"opaque"` 选择不透明和 Alpha 裁切材质。`"transparent"` 选择混合材质、广告牌粒子和淡入淡出的模型粒子。`"none"` 保留背景和上游输入，用作纯合成阶段。

过滤发生在材质子网格级别，同一个 glTF 模型可以分到不同阶段。被排除的材质不会触发它引用的命名捕获。深度、法线等几何附件取不透明与裁切表面。使用默认清除值时，设置 `"transparent"` 或 `"none"` 的捕获提供颜色附件。自定义深度清除值还可生成常量深度。

**覆盖捕获阶段的材质**

捕获记录可声明 `override-material`。这个记录接受模型材质的字段，包括基础色、透明模式、贴图、`material-shader` 和 `shader-textures`。省略该记录或将其中的 `enabled` 设为 `false` 时使用场景材质。启用覆盖后，从默认材质开始应用记录字段。

捕获先按图层和原材质筛选子网格，再应用覆盖记录。例如，`renderables: "transparent"` 可以选中透明子网格，随后用不透明材质绘制遮罩。绘制继续使用原顶点流、骨骼和形变数据。覆盖材质自己的透明模式与深度设置决定颜色阶段如何提交。几何附件按覆盖后的不透明表面生成。自定义片元着色器可以通过 `shader-textures` 引用其他命名捕获，引擎仅调度该阶段实际使用的材质输入。

```xr
struct MaskStyle { enabled: bool, base-color: color, unlit: bool, alpha-mode: string }
struct Feed {
    name: string, layers: int, renderables: string, override-material: MaskStyle,
}

SceneView3D {
    scene-captures: [
        Feed { name: "mask", layers: 1, renderables: "transparent",
            override-material: MaskStyle {
                enabled: true, base-color: #fff, unlit: true, alpha-mode: "opaque",
            } },
    ];
    scene-color-capture: "mask";
}
```

**配置捕获阶段的管线状态**

捕获记录可声明 `pipeline-state`。可选的 `enabled: bool` 控制整条记录，未指定的字段沿用材质行为。记录中的深度和剔除设置用于捕获实际绘制的网格与粒子，输入的颜色和深度按捕获自身的合成规则载入。

| 字段 | 含义 |
| --- | --- |
| `depth-test: bool` | 设为 `false` 时深度比较总是通过。深度测试与写入可以独立控制。 |
| `depth-compare: string` | `material`、`never`、`less`、`less-equal`、`equal`、`not-equal`、`greater-equal`、`greater`、`always`。导入深度后可用 `greater` 选出上游表面背后的片元。 |
| `depth-write: bool` | 显式指定捕获几何是否写深度。关闭写入时仍然可以测试导入的深度，该捕获的 `depth` 附件按缺失输入处理。 |
| `cull-mode: string` | `material`、`back`、`front`、`none`。 |
| `polygon-mode: string` | `fill` 填充三角形，`line` 绘制三角形边缘。默认 `fill`。 |
| `blend-mode: string` | `material`、`replace`、`source-over`、`additive`、`multiply`、`screen`。这些设置直接作用于颜色输出，几何数据附件按数值写入。 |
| `viewport: Bounds` | 将相机画面映射到捕获纹理的指定区域。 |
| `scissor: Bounds` | 把网格与粒子的写入限制在指定区域。 |

矩形记录可自定义名称。它需要 `x`、`y`、`width` 和 `height` 四个 `float` 字段，可选 `enabled: bool`。坐标以纹理左上角为原点，纹理宽和高各对应 1。矩形与纹理求交，裁剪还会和视口求交。空裁剪跳过几何绘制，背景和上游颜色仍覆盖整张纹理。

命名深度使用映射后的投影重建世界坐标。导入深度时，来源与消费方的相机和视口映射需要匹配。裁剪范围可以不同。移动矩形复用捕获纹理和 GPU 管线。

显式指定比较或写入时，捕获跳过材质的深度预处理，在颜色通道里执行给定的深度状态。显式混合或强制写深度启用直接透明绘制。透明网格按深度排序，模型粒子和广告牌沿用发射器次序。

```xr
struct GlassState { depth-test: bool, depth-write: bool, blend-mode: string }
struct Feed { name: string, layers: int, pipeline-state: GlassState }

SceneView3D {
    scene-captures: [
        Feed { name: "overlay", layers: 4, pipeline-state: GlassState {
            depth-test: false, depth-write: false, blend-mode: "source-over",
        } },
    ];
    scene-color-capture: "overlay";
}
```

**选择捕获颜色格式**

捕获记录可用 `color-format: string` 选择主颜色附件的存储格式。默认是 `"rgba16f"`。所有格式直接存储场景线性值，可通过 `scene-color-capture`、`color-source`、材质着色器和效果读取。

| 格式 | 通道 | 每像素 |
| --- | --- | ---: |
| `"rgba16f"` | RGBA，16 位浮点 | 8 字节 |
| `"rgba8"` | RGBA，8 位归一化 | 4 字节 |
| `"rg16f"` | RG，16 位浮点 | 4 字节 |
| `"rg8"` | RG，8 位归一化 | 2 字节 |
| `"r16f"` | R，16 位浮点 | 2 字节 |
| `"r8"` | R，8 位归一化 | 1 字节 |

8 位归一化格式将通道值限制在 0 到 1。用单、双通道保存遮罩或其他数据时，读取缺失的 G、B 得到 0，缺失的 A 得到 1。几何与深度附件沿用各自格式。两个同配置捕获只有颜色格式一致时才共享颜色输出。改格式会重新分配该颜色附件，其他捕获可复用已有结果。

运行 `bazel run //examples/puzzle3d:play_capture_composite`，开启 **Offscreen** 后点 **Capture R8** 或 **Capture HDR** 对比。格式字段支持 VS Code 补全。

**初始化捕获深度**

捕获记录可用 `depth-clear: float` 设置初始深度，默认 1，取值限制在 0 到 1。例如，把初始深度设为 0，再使用 `pipeline-state.depth-compare: "greater"`，更远的片元就能通过本阶段的深度测试。需要给后续捕获提供统一阈值时，可以创建一个 `renderables: "none"` 且自定义清除值的捕获，再通过 `depth-source` 导入它的深度。导入仍按相机投影和阶段深度合成规则处理。

通过 `material_texture` 或 `scene_buffer_color` 读取命名深度时，空白像素的 R 通道是实际清除值。世界坐标重建用独立的空白标记判断有效性，即使自定义清除值与某个有效表面的深度数值恰好相同，也可以区分两者。自定义深度清除沿用每像素 4 字节的附件。

**在命名捕获间共享模板遮罩**

捕获记录可用 `stencil-clear: int` 指定模板初值，默认 0。`stencil-source: string` 引用同尺寸捕获的模板缓冲。遮罩按纹理像素对齐，因此两个捕获可以使用不同相机。模板导入沿用捕获依赖顺序，后续阶段用自己的深度状态绘制。尺寸不匹配或循环依赖时，当前阶段使用本地模板清除值。

`pipeline-state.stencil` 可声明以下字段。引用值、读掩码、写掩码和清除值的范围为 0 到 255。

| 字段 | 用途 |
| --- | --- |
| `enabled: bool` | 控制该状态记录，默认启用。 |
| `reference: int` | 动态模板引用值，默认 0。 |
| `read-mask: int`、`write-mask: int` | 参与比较和写入的位，默认 255。 |
| `compare: string` | 模板比较函数，接受与深度比较相同的比较名称，默认 `always`。 |
| `fail: string`、`depth-fail: string`、`pass: string` | 模板测试失败、深度测试失败、两项测试均通过时执行的操作，默认 `keep`。也可用 `zero`、`replace`、`invert`、`increment-clamp`、`decrement-clamp`、`increment-wrap`、`decrement-wrap`。 |

例如，一个写入遮罩的捕获可用 `color-write: false`、`pass: "replace"` 和 `reference: 1`，后续读遮罩的捕获可用 `compare: "equal"`、`reference: 1` 和 `write-mask: 0`。模板写入伴随捕获的几何测试，可以配合图层、视口、裁剪和加权透明。缓存命中可复用模板结果，消费方重绘时只在 GPU 上传递模板字节。

**查看捕获线框**

`polygon-mode: "line"` 可以在命名捕获中查看模型及粒子的三角形边缘。线框仍按材质着色，深度和模板状态只处理覆盖到的边缘。线框阶段跳过原材质的填充深度预处理。设置 `cull-mode` 可以选择保留哪一侧的边缘，也可与捕获视口和裁剪组合。

渲染器会在可用的设备上使用原生多边形线框并复用索引。其余设备使用 WGSL 计算边缘覆盖，第一次绘制索引网格时建立可复用的展开缓存。开发环境可用 `XRACK_FORCE_SHADER_WIREFRAME=1` 验证 WGSL 路径。

**分别控制 RGB 和 Alpha 混合**

命名捕获的 `pipeline-state.target-blend` 可以覆盖主颜色附件的混合方程与通道写入。RGB 和 Alpha 各自计算 `source × source-factor` 与 `destination × destination-factor`，再应用对应的运算。源颜色是材质输出的场景线性预乘颜色，目标颜色是该附件中已写入的值。

| 字段 | 用途 |
| --- | --- |
| `enabled: bool` | 是否使用此记录，默认启用。设为 `false` 时沿用 `blend-mode` 或材质状态。 |
| `blend-enabled: bool` | 是否执行混合，默认启用。关闭时直接写入选中的通道。 |
| `write-mask: int` | 允许写入的通道位，R 为 1、G 为 2、B 为 4、A 为 8。默认 15，0 保留全部通道。外层 `color-write: false` 会关闭全部写入。 |
| `source-color`、`source-alpha: string` | 源 RGB 与源 Alpha 的混合因子，默认 `"one"`。 |
| `destination-color`、`destination-alpha: string` | 目标 RGB 与目标 Alpha 的混合因子，默认 `"one-minus-src-alpha"`。 |
| `color-operation`、`alpha-operation: string` | 可用 `"add"`、`"subtract"`、`"reverse-subtract"`、`"min"`、`"max"`，默认 `"add"`。最小值和最大值比较缩放前的通道。 |
| `constant: color` | 自定义混合常量，默认透明黑。RGB 转为线性值，Alpha 直接归一化。改变常量会重绘捕获并复用原渲染管线。 |

混合因子可以使用 `"zero"`、`"one"`、`"src"`、`"dst"`、`"src-alpha"`、`"dst-alpha"`、`"constant"`，以及 `"one-minus-src"`、`"one-minus-dst"`、`"one-minus-src-alpha"`、`"one-minus-dst-alpha"`、`"one-minus-constant"` 和 `"src-alpha-saturated"`。后者的 RGB 因子为源 Alpha 与 `1 − 目标 Alpha` 的较小值，Alpha 因子为 1。

```xr
struct TargetBlend { source-color: string, destination-color: string, write-mask: int }
struct OverlayState { target-blend: TargetBlend }
struct Feed { name: string, color-source: string, layers: int, pipeline-state: OverlayState }

// 在 SceneView3D 中，只把发光图层的 RGB 加到现有底图上，保留底图的 Alpha。
scene-captures: [
    Feed { name: "background", layers: 1, pipeline-state: OverlayState {
        target-blend: TargetBlend { source-color: "one", destination-color: "zero", write-mask: 15 },
    } },
    Feed { name: "glow", layers: 2, color-source: "background", pipeline-state: OverlayState {
        target-blend: TargetBlend { source-color: "one", destination-color: "one", write-mask: 7 },
    } },
];
scene-color-capture: "glow";
```

自定义颜色混合按绘制顺序直接写入目标，适用于模型、精灵和粒子。运行 `bazel run //examples/puzzle3d:play_capture_composite`，打开 **Offscreen** 后点击 **Custom blend** 可以对比材质和自定义混合。

**跨帧效果纹理**

在有名字的输出通道上设置 `history: true` 可保存提交成功的结果。额外输入记录设置 `previous-frame: true` 后，同名绑定会读取上一帧的输出，可以引用当前通道自己。第一帧会收到缺失输入，着色器可用 `scene_has_buffer(0u)` 判断是否已有历史。读取与写入交替使用两张 GPU 纹理。

```xr
struct HistoryInput { name: string, previous-frame: bool }
struct HistoryPass { shader: string, output: string, history: bool, buffers: [HistoryInput] }

export component TrailView inherits SceneView3D {
    scene-effects: [
        HistoryPass {
            shader: @shader-url("effects/feedback.wgsl"), output: "trail", history: true,
            buffers: [HistoryInput { name: "trail", previous-frame: true }]
        }
    ];
}
```

历史按视口保存。视口尺寸、输出格式或着色器源码改变时从空白开始。应用也可以改变 `scene-effect-history-revision: int` 主动重置，同一个修订值下调整强度或参数会继续使用现有历史。启用 `cache-rendering` 时，普通重绘可复用静止视口的画面。`scene-effect-animate: true` 可请求连续帧，让拖影在物体停止后继续衰减，默认只在视口需要重绘时推进。每个渲染器的历史双缓冲预算为 128 MiB，按纹理格式的字节数计算。超出预算的历史输入按缺失处理。 [拖影示例](../examples/puzzle3d/effect_history.xr)

`bloom: true;` 为可见高亮启用泛光，默认关闭。`bloom-threshold` 是曝光前的线性亮度阈值，默认 1，阈值附近使用平滑过渡。`bloom-strength` 默认 0.45，设为 0 可跳过泛光计算。`bloom-levels` 可取 1 到 6，默认 5，更多级别会扩散到更远处。渲染器从合成后的 HDR 画面提取高亮，用长宽逐级减半的纹理过滤，并在色调映射时合入泛光。遮挡、透明和 MSAA 会先结算，泛光留在当前视口内。 [HDR 泛光示例](../examples/puzzle3d/bloom.xr)

`depth-of-field: true;` 启用景深，默认关闭。`dof-focus-distance` 按相机前向轴测量对焦距离，默认 10 个世界单位。`dof-focus-range` 是焦平面两侧清晰区的总宽度，默认 2。`dof-blur-radius` 设置最大模糊半径，接受逻辑像素长度，默认 `12px`，范围 `0px` 到 `32px`。`0px` 跳过景深通道。

景深读取当前视口的不透明和 Alpha Mask 深度，按半分辨率过滤前后景，再用全分辨率深度合成。透明颜色参与后处理，其对焦距离沿用该像素的不透明深度。MSAA 深度采用最近样本，支持透视和正交相机。LDR 视口保留原色彩空间，HDR 则在泛光和色调映射之前完成景深。拾取仍使用原三角面。 [景深示例](../examples/puzzle3d/focus.xr)

要为物体配置 LOD，先用带唯一 `id` 的场景行声明主网格，再为每个替代网格添加一行。替代行用 `lod-of` 指向主行，用正的 `lod-max-size` 长度设定启用该级别的最大投影尺寸。例如在同一个场景列表里写 `Actor { mesh: @mesh-url("models/rock-medium.glb"), lod-of: "rock", lod-max-size: 140px, ..root.defaults }`，再为远景网格写一个更小的阈值。渲染器选用仍能覆盖投影尺寸的最小阈值。超过所有阈值时使用主网格。阈值采用逻辑像素，正交相机下移动相机的距离不会改变屏幕尺寸。

替代行提供 `mesh`、`lod-of`、`lod-max-size` 和可选的 `visible`。选中后沿用主行的变换、颜色、拾取设置、动画时间、形变权重和场景行索引。替代 mesh 可以是内建形状或 glTF，AOT 会随主网格一起嵌入资源。`lod-bias: float` 可以调节单个物体，`camera-lod-bias: float` 可以调节具名相机。默认乘数为 1，设为 0 使用主网格。LOD 使用主网格的姿态包围盒来估算屏幕大小。

每个视口独立选择 LOD。同一相机的颜色、阴影和视口拾取共用选中的网格，显式世界射线使用主网格。已计算过的网格姿态和相同级别组合可复用。`Renderer::scene_draw_stats()` 还提供替代网格的选择次数和颜色通道提交的顶点数。 [LOD 示例](../examples/puzzle3d/lod.xr)

glTF 资源也可以用 `@mesh-url("models/rock.glb", auto-lod)` 自动生成 LOD。资源加载会在材质分区内减面，把法线、纹理坐标、顶点色和切线纳入误差估算，保留边界与顶点属性断开处。生成的级别根据实测误差设置屏幕阈值。手写替代行优先于自动级别，仍可用 `lod-bias` 和 `camera-lod-bias` 调整。

自动减面适用于静态、节点动画、蒙皮和形变网格。幸存顶点保留原关节权重、形变增量和目标名。蒙皮级别会抽样检查 glTF 动画姿态，形变级别会检查每个目标的位置偏差。形变权重绝对值超过 1 时，自动级别会按当前最大权重缩小启用范围。AOT 把生成结果和阈值嵌入产物，解释器在准备资源时用同一算法生成。无可用蒙皮姿态样本、未能通过误差预算或减面收益不足的网格会沿用主网格。

glTF 相机可通过场景行 ID 加选择器引用，例如 `camera: "rig/Hero";` 或 `camera: "rig/#1";`。井号后的数字是原文件的节点索引。名称优先取节点名，其次取相机名，重复名称用节点索引区分。只有相机和动画的 glTF 也可以直接引用。摄像机位置跟随节点和动画，视图方向忽略节点缩放。

透视镜头可省略远裁剪面。固定宽高比的透视和正交镜头会在控件内居中适配，留边使用 `scene-background`。绘制和拾取使用同一画幅，留边不会命中 3D 物体。投影、反投影、景深和阴影使用同一镜头。VS Code 悬停 `@mesh-url` 可查看可用的相机节点选择器。在同一组件中用字面量声明相机行时，输入 `camera: "rig/` 可补全该行的相机。 [导入镜头示例](../examples/puzzle3d/imported_cameras.xr)

结合 `Scene3D.pick-camera(model, camera, viewport, pointer)` 使用相同的记录模型、相机 ID 和视口尺寸即可拾取，匹配渲染的投影和裁剪面。解谜示例的「俯视地图」可以在正交和绕视镜头之间切换。`selected-object` 高亮给定 ID，`scene-background` 设置视口背景。

如果还需要命中点的信息，使用 `Scene3D.hit(objects, yaw, pitch, distance, viewport, pointer)` 或 `Scene3D.hit-camera(objects, camera, viewport, pointer)`。两个函数按视口当前裁剪面返回最近的可见表面。`Scene3D.ray-hit(objects, origin, direction, max-distance)` 可从任意世界位置发射射线，其中 `origin` 用 `Point3D.new` 构造，`direction` 用 `Vector3D.new` 构造。方向会归一化，最大距离和结果距离均用世界单位。

`hit3d` 的 `valid` 表示是否命中。命中匿名网格时 `valid` 仍为真，`object-id` 为空。`position` 和 `normal` 使用世界坐标，`local-position` 和 `local-normal` 使用网格坐标，形变和蒙皮会参与计算。法线按命中三角形的顶点法线插值，双面材质从背面命中时反转。`u` 和 `v` 是插值后的第一组纹理坐标，未提供 UV 时默认为零。`distance` 是从射线起点到命中点的距离。透视拾取从相机位置出发，正交拾取从相机平面上对应指针的位置出发。

无效射线、无效相机或未命中会返回 `valid: false` 的结果。遮挡、透明裁剪和背面判定与 ID 拾取共用同一套逻辑。多个实例共享刚性导入网格时，可复用网格的三角形包围盒树；正在变形的网格使用当前姿态的三角面。

穿透选择可用 `Scene3D.hit-all`、`Scene3D.hit-all-camera` 或 `Scene3D.ray-hit-all`。它们和对应的单命中查询接受相同参数，返回按距离从近到远排列的 `[hit3d]`，包括前方表面后面的物体。普通子网格取每行最近的命中。glTF 实例按 `row-index`、`mesh-node-index` 和 `instance-index` 保留各自最近的命中。距离相同时沿用单次拾取的绘制顺序规则，因此取列表第一项可得到同一命中。

`row-index` 是在查询发生时命中物体在原场景记录列表中的从零开始的索引。它可以区分匿名行和重名行。未命中或 Rust 直接构造的场景物体使用 `-1`。每个模型行可声明可选的 `pickable: bool`，默认开启。关闭后，该行仍参与绘制和阴影，射线会继续查找它后面的物体。

可用 `Model.count(hits)`、`Model.at(hits, index, miss)` 和 `for hit in hits` 消费结果。这里的 `miss` 可以用没有初值的 `hit3d` 属性提供。解谜示例开启「穿透选择」后，点击场景，再按「下一个命中」就能沿射线切换选择。 [解谜示例](../examples/puzzle3d/puzzle.xr)

要把 2D 标签放在世界中的一个点上，可以用 `Point3D.new(x, y, z)` 创建世界坐标，再调用 `Scene3D.project(yaw, pitch, distance, viewport, world)` 或 `Scene3D.project-camera(model, camera, viewport, world)`。将用于绘制和投影的视口宽高绑定到同一组属性，再用 `Point2D.new(root.view-width, root.view-height)` 得到尺寸。相机参数也使用画面绑定的同一组值。标签的 `x`、`y` 可直接绑定到投影坐标。 [解谜示例](../examples/puzzle3d/puzzle.xr) 用这套接口定位「出口」标签。

投影结果的类型是 `projectedpoint3d`。其中 `point.x`、`point.y` 是相对视口左上角的逻辑像素，`depth` 是从相机沿前向轴测量的世界单位。相机可用且世界点在相机前方时，`valid` 为真。点还要位于视口矩形内、通过近远裁剪，`inside` 才为真。画面外的有效点也有位置，可以据此放置指示箭头。`inside` 表示通过视锥检查，物体遮挡仍由拾取处理。

`Scene3D.unproject(yaw, pitch, distance, viewport, pointer, depth, fallback)` 和 `Scene3D.unproject-camera(model, camera, viewport, pointer, depth, fallback)` 将逻辑像素坐标与正的前向深度还原为 `point3d`。将一次投影的 `point` 和 `depth` 传回同一个相机即可还原世界点。无效相机、坐标或深度返回指定的 `fallback`。两种映射共享渲染与拾取使用的透视或正交镜头。

`Scene3D.map-position(model, from, to, position, fallback)` 在两个具名场景行之间转换 `point3d`，包含父级变换、缩放和枢轴。`Scene3D.map-direction(model, from, to, direction, fallback)` 转换 `vector3d`，使用节点及父级的旋转并保持向量长度。空 ID 表示世界空间。两个函数接受控制 `SceneView3D.model` 的同一记录列表，也支持作为挂点的空网格行。

查询使用声明的场景行变换，隐藏的节点仍可作为挂点。缺失或重名的节点、无效父级链和非有限坐标返回指定的 `fallback`。可以把挂点的位置和发射方向转换到世界空间后传给 `Scene3D.ray-hit`。 [节点坐标示例](../examples/puzzle3d/node_mapping.xr)

`Scene3D.sphere-cast(objects, origin, direction, max-distance, radius, layer-mask?)` 沿归一化方向连续移动球体，返回第一个 `hit3d` 接触。距离是球心移动量，`position` 是碰撞面上的接触点，`normal` 是用于分离的几何法线。球体可以接触物体的面、棱或顶点。起始位置已经穿入时返回距离 0。起点刚好相切时，沿表面或远离表面的运动可以继续。扫掠途中相切的接触仍会报告，可通过法线判断运动方向。

`Scene3D.sphere-overlap(objects, center, radius, layer-mask?)` 返回 `[hit3d]`，每条源模型行最多返回一个接触，按模型顺序排列。基础盒体和球体按封闭体检查内部，导入模型和高度图按变换后的三角面检查。可用记录字段 `collidable: bool` 独立控制这两种查询，默认开启。它们使用可见行和指定层掩码，默认第一层。三角面参与双面碰撞，材质透明度继续服务于绘制和射线拾取。

静态查询复用场景和网格的 BVH。蒙皮及形变网格使用当前姿态，碰撞查询始终使用主网格。半径必须为正，距离为非负世界单位。位置、方向或范围无效时返回未命中。想约束移动时，可以用返回距离限制球心的目标位置。 [扫掠与高度图碰撞示例](../examples/puzzle3d/sphere_cast.xr)

`Scene3D.sphere-slide(objects, origin, displacement, radius, layer-mask?)` 返回球体移动后的中心 `point3d`。传入本次请求的世界坐标位移。遇到墙时，先推进到接触前，再将剩余位移投影到可移动的方向。多个接触面会一起限制剩余位移，例如进到两面墙的拐角后，仍可沿两面墙的交线移动。

滑移只处理阻挡运动的接触，纯相切和分离方向可以继续移动。球心起始于封闭图元内部时停在原处。每次调用处理最多八次阻挡接触，并在接近表面时沿进入方向留出半径万分之一的间隙。这个函数使用同一场景 BVH、网格 BVH 和 `collidable` 字段。 [沿墙滑移示例](../examples/puzzle3d/sphere_slide.xr)

`Scene3D.sphere-walk(objects, origin, displacement, radius, step-height, snap-distance, max-slope, layer-mask?)` 为空间移动加入贴地规则，返回移动后的球心 `point3d`。Y 轴向上，`max-slope` 是允许向上走的坡度角，范围为 0 到 90 度。过陡的上坡接触会同时限制朝坡内的水平运动，仍可沿坡横向移动。

起点有地面支撑且位移朝水平或向下时，`step-height` 启用上台阶。系统检查头顶空间、抬高后的水平路线及可行走的落脚点，且台阶路线须比普通滑移推进更远。`snap-distance` 控制下坡或下台阶后的最大贴地距离。两个值均为非负世界单位，设为 0 可分别关闭。向上移动和从空中开始的移动继续使用本次传入的位移，跳过踏步与吸附。恒定零位移在编译时折叠为原位置。 [地面行走示例](../examples/puzzle3d/sphere_walk.xr)

`lights` 接受灯光记录列表，记录名可以自定。以下字段参与类型检查。

```xr
struct SceneLight3D {
    kind: string,
    x: float, y: float, z: float,
    yaw: float, elevation: float,
    intensity: float, color: color,
    range: float, inner-angle: float, outer-angle: float,
}
```

- `"directional"` 提供方向光。偏航和仰角都为 0 时，光沿世界的 −Z 方向传播。
- `"point"` 从 `x`、`y`、`z` 向各方向发光，亮度按距离平方衰减。
- `"spot"` 在点光基础上沿一个锥形区域发光，`inner-angle` 和 `outer-angle` 是以度为单位的半角。内锥保持最大亮度，外锥边缘平滑衰减到零。

`intensity` 是线性强度，`color` 是 sRGB 颜色。点光和聚光灯的 `range: 0` 表示无限范围，正值表示衰减到零的位置。每个视口读取前 16 条记录，零强度和无效记录会跳过。`ambient-color` 与 `ambient-intensity` 控制常量环境光，默认分别为白色和 1。

灯光记录的可选 `parent: string` 将方向光、点光或聚光灯挂到模型里的节点。点光和聚光灯的局部位置继承父变换。方向光和聚光灯的照射方向继承旋转，光照范围仍以世界单位计。隐藏父节点、无效父引用或灯光行上的 `visible: false` 会停用该光源。移动灯光时，在同一视口的阴影图布局和投影物体保持一致的情况下，可以保留未变化的深度层。

导入 glTF 的 `KHR_lights_punctual` 时，方向光、点光和聚光灯保留线性 RGB、强度、范围和聚光锥。灯光位置跟随节点和动画，方向取旋转，节点缩放只作用于灯光的位置。只包含灯光和动画的 glTF 也可以作为场景行引用。隐藏源行或父节点会隐藏它的灯光。 [导入布光示例](../examples/puzzle3d/punctual_lights.xr)

`imported-lights` 默认为 `true`。同时有手写灯光和导入灯光时，视口先使用 `lights` 中的灯，再使用 glTF 的灯，总数最多 16。启用 `shadows` 时导入灯光也可投影。切换镜头或几何 LOD 时沿用主场景灯光，`light_buffer_uploads` 可查看 GPU 灯光缓冲区上传次数。悬停 `@mesh-url` 可查看 glTF 灯名和类型。

省略 `lights` 且没有启用的导入灯组时，`light-yaw` 和 `light-elevation` 控制默认方向光。可绑定空灯光列表并关闭 `imported-lights`，只保留环境光。

在视口上设置 `shadows: true` 启用实时阴影，默认关闭。每个视口按灯光列表顺序为前四盏开启投影的灯光生成深度图。方向光默认使用一个深度图，聚光灯使用一个深度图。点光沿六个立方体朝向投影，只渲染与投影物体相交的面。`shadow-map-size` 指定深度图的边长，默认 1024 像素，可设为 128 到 2048。当多个灯和面超过单组 64 MiB 的显存预算时，渲染器会降低深度图分辨率。`shadow-bias` 控制深度比较的偏移，默认 0.001。出现自阴影时可以稍微提高偏移。

灯光记录可以声明 `casts-shadow: bool`，物体记录可以声明 `casts-shadow: bool` 和 `receives-shadow: bool`。字段省略时按 `true` 处理。阴影只减弱相应灯光的直接光照。环境光、自发光和 unlit 材质按原有规则显示。`MASK` 按材质的 alpha 阈值投影，`BLEND` 的半透明图元可以接收阴影。绕视和改变光强时复用深度图，移动投影物体、移动点光源或调整光照方向时更新。对于点光源，偏移量按阴影视锥的最远距离缩放，以保留远处遮挡的精度。

方向光可以用 `shadow-cascades` 将相机深度范围分成 1 到 4 级，默认 1。近处切片覆盖更小的世界范围，能用相同分辨率分辨更细的阴影。级联数量大于 1 时，`shadow-distance` 设置从相机开始的最远投影距离，0 表示按场景范围计算。`shadow-split-balance` 在均匀分割（0）和对数分割（1）之间插值，默认 0.5。`shadow-cascade-blend` 控制相邻切片的过渡比例，范围 0 到 1，默认 0.1。

级联使用与深度图像素网格对齐的投影。给定固定的距离和视口比例时，小于一个深度图像素的相机变化可复用已有深度图。相机移动后，点光、聚光灯和投影仍相同的级联层可以继续复用。相机画面外的物体也可以给画面内的表面投影。

使用 `environment: @environment-url("models/studio.hdr")` 加载 Radiance HDR 环境探针。资源路径相对声明所在的 `.xr`，Bazel 应用要把文件加入 `xr_ui_library(assets = [...])`。探针接收 2:1 的 RGBE 全景图，最多 8 Mi 像素，文件最多 16 MiB。编译器预计算漫反射和按粗糙度分层的立方体反射，AOT 嵌入结果，解释器按资源内容缓存。环境光与已有的方向光、聚光灯和点光叠加。

`environment-yaw` 以度旋转环境，`environment-intensity` 调整环境光与天空盒的强度，默认 1。`environment-skybox` 默认关闭，开启后用同一张探针绘制背景。加载探针时，视口在线性 HDR 颜色空间完成透明混合，再通过色调映射输出。`scene-exposure` 调整整个 3D 视口的曝光，默认 1。旋转环境、移动相机或调整曝光会复用 GPU 上已上传的环境纹理。

`reflection-probes` 为同一视口中的模型提供局部 HDR 镜面反射。探针记录需要 `position: point3d` 和 `box-size: vector3d`。使用 `source: @environment-url("models/room.hdr")` 加载 HDR 全景。省略 `source` 或将其设为空字符串会从场景捕获。`position` 是世界空间采样中心。把 `box-size` 设为世界轴对齐盒体的三条边长，三个分量均须大于零。可用 `box-offset: vector3d` 将影响盒相对采样中心平移，默认零。

视口读取前八个探针记录，从包含模型原点的盒中选采样中心最近的探针。距离相同时按记录顺序选取。模型行可用 `receives-reflections: false` 关闭局部反射，或通过移动模型进入另一个盒。盒外模型使用全局环境的镜面反射。全局环境还提供天空和漫反射。`parallax-correction: true` 根据反射射线在盒体上的出口方向查找局部纹理，默认关闭。探针的 `yaw` 以度旋转纹理，`intensity` 控制强度，`visible` 控制是否参与。默认值依次为 0、1 和 `true`。

局部探针沿用 HDR 预滤波和 GPU 纹理缓存。移动镜头时可以复用网格与实例缓冲。同一批次内相邻且使用同一探针的模型仍可合批。探针绑定属于绘制更新。 [局部反射示例](../examples/puzzle3d/reflection_probes.xr)

场景捕获为匹配接收模型的探针渲染六个 HDR 面，并在 GPU 上为不同粗糙度生成 mip。`capture-size: int` 指定每面像素尺寸，默认 128，范围为 32 到 256。模型记录可用 `casts-reflections: false` 退出捕获。捕获使用当前视口的场景灯光、全局环境和匹配的层。本地探针从全局环境获取反射输入，避免构成立方体捕获的递归依赖。

`refresh-mode: "on-change"` 在捕获输入变化时刷新，且为默认模式。`"first-frame"` 保留初次捕获的内容，适合静态布景。`"every-frame"` 在视口绘制时更新。`capture-revision: int` 可显式请求刷新。改变探针位置或面尺寸也会更新捕获。仅移动观察相机时，按变化刷新的探针可以复用现有立方体贴图。`cache-rendering` 也会跟踪立方体贴图版本。Rust 可查询 `reflection_cubemap_updates`、`reflection_capture_faces` 和 `reflection_prefilter_passes`。 [实时场景反射示例](../examples/puzzle3d/live_reflections.xr)

`time-slicing: "none"` 默认在同一帧完成捕获和预滤波。`"individual-faces"` 分六帧捕获六面，再分六帧逐面生成 mip。`"all-faces-at-once"` 在第一帧捕获六面，之后每帧为六面生成一级粗糙度 mip。两种分帧模式都按每个探针调度。

分帧捕获使用开始时的场景和探针位置。更新完成后才发布整张立方体贴图，并切换视口缓存使用的版本。期间继续合成上一张已完成的贴图。初次捕获期间使用全局环境。按变化刷新时，捕获期间的新输入会安排到下一轮。原生窗口和 VS Code 预览会按需请求帧，直到当前任务收尾。

结合 `TouchArea` 的本地指针坐标，可以调用 `Scene3D.pick` 取得最近可见物体的 ID。传入和视口一致的记录模型、相机属性和布局尺寸。空 ID 的物体仍会遮挡后方物体。Rust 宿主可以通过 `xrack_runtime::scene3d::raycast` 发出世界空间射线，获取命中的 ID、距离、位置和法线。查看 [3D 解谜示例](../examples/puzzle3d/README.md) 中的绕视、拾取和原生 Play。

#### 指定模型拾取

可以在拾取参数末尾、可选层掩码之前传入行索引或行索引列表。 `Scene3D.hit-row`、`Scene3D.hit-row-camera` 和 `Scene3D.ray-hit-row` 返回指定行的 `hit3d`。对应的 `Scene3D.hit-subset`、`Scene3D.hit-subset-camera` 和 `Scene3D.ray-hit-subset` 返回候选行的 `[hit3d]`，从近到远排列。 `hit-row` 和 `hit-subset` 使用绕视相机，带 `-camera` 的版本用指定相机，`ray-hit-` 系列用世界射线。重复及越界索引会被忽略，列表可以直接使用框选结果。

行索引从 0 开始。前方模型若不在候选中，仍可以命中后方候选。相机裁剪、`pickable`、层掩码及材质的背面与透明裁切规则继续生效。普通子网格按行合并，属于同一行的不同 glTF 实例可以各自返回命中。大型场景的点选和射线查询共用场景 BVH。 [候选拾取示例](../examples/puzzle3d/filtered_picking.xr)

#### 拾取单个 glTF 实例

`Scene3D.hit-instance`、`Scene3D.hit-instance-camera` 和 `Scene3D.ray-hit-instance` 在相应射线参数之后依次接收 `row-index`、`mesh-node-index`、`instance-index`，最后可以传入层掩码。三个索引都从零开始，可以直接使用一次 `hit3d` 的结果。即使实例在另一个表面之后，也可验证射线是否命中它。相机裁剪、透明度、背面和 `pickable` 继续生效。缺失或无效的地址返回 `valid: false`。 [实例拾取示例](../examples/puzzle3d/instance_picking.xr)

#### 视口框选

`Scene3D.pick-in-rect(objects, yaw, pitch, distance, viewport, start, end, layer-mask?)` 使用绕视相机，`Scene3D.pick-in-rect-camera(objects, camera, viewport, start, end, layer-mask?)` 使用指定相机。`viewport` 是视口的逻辑尺寸，`start` 和 `end` 是本地逻辑像素中的两个角，任意拖动方向均可。矩形会裁到相机有效画面之内，近远裁剪面和层掩码仍然生效。

返回的 `[int]` 是当前场景模型中从 0 开始的行索引，按行顺序排列。一行中的多个 glTF 子网格只计一次，匿名模型也可框选。只要可拾取模型的三角面与框相交就会入选，包括被其他模型遮挡的表面。查询使用当前相机选择的 LOD 和动画姿态，直接计算几何相交。零宽、零高或完全落在画面外的矩形返回空列表。 [框选示例](../examples/puzzle3d/marquee.xr)

#### 最近表面点

`Scene3D.closest-point(objects, origin, radius, layer-mask?)` 在世界空间半径内找最近的可拾取表面。`Scene3D.closest-point-on(objects, object-id, origin, radius, layer-mask?)` 将搜索限定到该 ID 对应的模型。两者返回 `hit3d`，包含命中对象、记录索引、世界与模型坐标、法线、UV 和从查询点到表面的距离。未命中时 `valid` 为 `false`。距离相同时按场景顺序选取。

查询使用动画后、经过父节点与实例变换的三角面。距离使用世界单位，非均匀缩放也会参与计算。它从三角面的任一侧计算几何距离，法线沿用模型朝向。隐藏或 `pickable: false` 的模型会跳过，`collidable` 由球体扫掠等碰撞查询使用。层掩码省略时搜索主层，0 表示任何层都不搜索。半径 0 可查询恰好位于表面上的点。 [吸附标记示例](../examples/puzzle3d/closest_surface.xr)

#### 3D 粒子

`particle-emitters` 接受使用自定义记录名的发射器列表。`particle-time` 是秒为单位的播放位置，默认 0，可以绑定 `Timer`、滑块或业务状态。发射器参数、种子和时间一致时会得到同一帧粒子，可以暂停、倒回和直接跳到目标时间。重设某个发射器的 `start-time` 会重启它的时间线。需要重叠的爆发时，为每次爆发保留一条独立记录。

| 记录字段 | 类型和含义 |
| --- | --- |
| `position`, `color`, `size`, `lifetime` | 必填。世界坐标 `point3d`、含透明度的 `color`、世界单位直径和秒为单位的寿命。后两项为 `float`。 |
| `rate`, `burst-count`, `start-time` | 连续发射的每秒速率 `float`、起始时一次发射的数量 `int`、起始秒数 `float`，默认都是 0。连续发射的首个粒子在起始后一个发射间隔出现。 |
| `velocity`, `velocity-spread`, `gravity` | 三个 `vector3d`，默认为零向量。分量为世界坐标方向的初速度、随机速度幅度和加速度。 |
| `end-size`, `fade-in`, `fade-out` | 三个 `float`，分别为末尾直径、淡入秒数、淡出秒数，默认是初始直径、0 和 0.25。 |
| `intensity`, `seed`, `layers`, `enabled` | 发光倍数 `float` 默认 1，限制在 0 到 64。随机种子和层掩码为 `int`，默认 0 和 1。启用状态为 `bool`，默认启用。 |

粒子使用面向相机的精灵，在 GPU 上按时间和种子算出位置、大小和淡出。CPU 每帧提交发射器参数。每个视口最多处理 16 个发射器、8192 个存活粒子，单个发射器最多 4096 个并优先保留较新的粒子。

精灵接受不透明几何的深度遮挡，并参与 HDR、泛光和 MSAA。开启 `order-independent-transparency` 时，精灵与半透明网格一起参与加权合成。普通模式下先绘制透明网格，再按发射器顺序混合精灵。粒子属于视口效果，拾取、碰撞、阴影和反射探针继续使用场景模型。 [粒子示例](../examples/puzzle3d/particles.xr)

发射器可以用 `sprite: image` 绑定 `@image-url("sprites.png")`。PNG 从左到右排列等宽的帧，单帧图片也可以直接用。精灵的高度由 `size` 控制，宽度按单帧比例计算，`color` 和 `intensity` 再乘上贴图颜色。无可用贴图时使用软圆精灵。

| 序列字段 | 类型和含义 |
| --- | --- |
| `sprite-frames` | `int`，默认 1，范围 1–256，应整除图片宽度。 |
| `sprite-frame` | `int`，起始帧从 0 编号。未声明时正放从首帧开始，倒放从末帧开始。 |
| `sprite-duration`, `sprite-duration-variation` | `float`，单位为秒，控制完整播放周期及每颗粒子的随机时长偏移。周期默认等于寿命，一个往返计作一周期。 |
| `sprite-playback` | `string`，可选 `"loop"`、`"reverse"`、`"ping-pong"`、`"ping-pong-reverse"`、`"once"`、`"single-frame"`，默认循环。 |
| `sprite-interpolate`, `sprite-random-start` | `bool`，默认开启相邻帧插值，默认关闭按粒子种子随机起帧。 |

上传时把贴图转换为线性预乘颜色，帧采样限制在单元格内。时间推进和换帧复用 GPU 上的贴图。加权透明下，同一贴图的发射器可以跨行合为一次实例化 draw。普通混合只合并相邻且共用贴图的发射器。Bazel 应用将 PNG 加入 `xr_ui_library(assets = [...])`。 [序列帧示例](../examples/puzzle3d/sprite_particles.xr)

`color-table: image` 接收静态 PNG。横轴从出生映射到寿命结束，RGBA 与发射器颜色和贴图相乘。多行颜色表会按种子为每颗粒子固定选一行，水平插值让颜色和透明度随年龄连续变化。改时间、速度或混合模式时复用驻留在 GPU 上的颜色表。

`particle-blend-mode: string` 默认是 `"source-over"`，使用常规透明合成。可选 `"additive"` 累加发光、`"screen"` 滤色提亮和 `"multiply"` 正片叠底。混合在线性颜色空间执行。HDR 保留加色后的高亮，滤色和正片叠底把粒子自身的直通 RGB 限制在 0–1。

所有模式都测试场景深度。打开加权透明时，加色、滤色和正片叠底按发射器顺序写入场景底色，常规透明粒子与透明网格随后参与加权合成。关闭加权时，四种模式都按发射器顺序绘制。共享混合模式、精灵贴图和颜色表的发射器可以合批。 [混合与颜色表示例](../examples/puzzle3d/particle_materials.xr)

`emitter-shape: string` 支持 `"point"`、`"box"`、`"sphere"` 和 `"cylinder"`，默认从 `position` 发射。`emitter-extents: vector3d` 是各轴的半尺寸，球体支持椭球，圆柱沿局部 Y 轴，X 和 Z 是截面半径。三个分量默认为 0。`emitter-fill: bool` 默认为 `true`，设为 `false` 可从面、外壳或端盖发射。表面采样考虑缩放后的面积。`emitter-yaw`、`emitter-pitch`、`emitter-roll` 以度旋转发射范围，顺序与场景物体一致。

| `velocity-mode` | 出生时的速度 |
| --- | --- |
| `"vector"` | 默认。以 `velocity` 为中心，按 `velocity-spread` 扰动各个分量。 |
| `"normalized-vector"` | 使用扰动后的方向，速度大小取 `velocity` 的长度。 |
| `"cone"` | 在以 `velocity` 为轴的锥内按立体角采样。`velocity-cone-angle: float` 是半角，范围 0–180 度。 |
| `"target"` | 从每颗粒子的出生点朝 `velocity-target: point3d` 发射。`velocity-target-spread: vector3d` 可以扰动目标的世界坐标。 |

后三种模式使用 `velocity` 的长度作为标称速度，可用 `velocity-speed-variation: float` 设置速度变化范围。目标用于计算出生时的速度方向，运动按初速度和 `gravity` 推进。发射范围和速度都按粒子身份与种子采样，参数固定时，向前或回拨时间得到相同轨迹。修改发射参数会按当前时间重算存活粒子。普通点发射的批次继续使用原来的顶点路径。 [空间发射示例](../examples/puzzle3d/particle_emitters.xr)

网格发射使用 `emitter-shape: "mesh"` 和 `emitter-mesh: @mesh-url("models/shape.gltf")`。也可以使用上表中的内置图元。发射范围取 glTF 初始姿态下的主网格表面，计入节点层级和初始变形。`emitter-mesh-scale: vector3d` 是三个轴各自的缩放，默认 `Vector3D.new(1, 1, 1)`。`position` 和发射器角度会把该范围摆放到场景中。资源键无法解析或缩放后没有有效三角形时，该发射器跳过。

网格模式始终从表面发射。GPU 先按缩放后的三角面面积选面，再在面内取均匀的点。设置 `velocity-mode: "surface-normal"` 可沿所选三角面的几何法线喷射，速度取 `velocity` 的长度，也可以叠加速度变化。移动、旋转和拖时间轴会复用采样表。共享网格、缩放与材质的发射器可以合批。单张采样表最多包含 300,000 个有效三角面。 [网格发射示例](../examples/puzzle3d/particle_mesh_emitters.xr)

`SceneView3D.particle-affectors` 接收自定义记录列表，必填字段为 `affector-kind: string`。GPU 先计算发射器的轨迹，再按列表顺序应用匹配的影响器。每个视口最多应用 16 个影响器。

| `affector-kind` | 作用 |
| --- | --- |
| `"attractor"` | 在 `duration: float` 秒内线性拉向 `position: point3d`。默认时长 −1 表示余下寿命。可用 `position-variation: vector3d` 和 `duration-variation: float` 随机扰动终点和时长，默认 0。`hide-at-end: bool` 默认关闭，开启后到达时隐藏。 |
| `"repeller"` | 以 `position` 为中心径向推开粒子。`radius: float` 内施加全量位移 `strength: float`，到 `outer-radius: float` 平滑衰减至零。默认内半径为 0，外半径和位移强度为 1，单位为世界单位。 |
| `"gravity"` | 沿单位 `direction: vector3d` 添加匀加速位移。`strength` 默认 1，单位为世界单位每秒平方，可为负数。默认方向为 −Y。 |
| `"point-rotator"` | 围绕经过 `position`、指向 `direction` 的轴旋转。`angular-speed: float` 默认 10 度每秒，可为负数。默认轴为 +Y。 |

`position` 默认在世界原点。`start-age: float` 是从粒子出生算起的启用秒数，默认 0。`layers: int` 默认为 −1，会与发射器和视口的图层取交集。`enabled: bool` 默认开启。`seed: int` 默认 0，用于固定吸引扰动，回拨时间可以得到同样的粒子位置。编辑影响器会用新参数重算存活粒子在当前时间的位置。只改变时间时复用影响器参数的 GPU 缓冲。 [粒子影响器示例](../examples/puzzle3d/particle_affectors.xr)

发射器可用 `trail-segments: int` 绘制面向视口的粒子拖尾。默认 0 使用普通精灵，1–16 指定条带段数。GPU 以 `trail-duration: float` 秒为历史窗口，默认 0.25。在窗口内按时间取样各个轨迹点，并在段间插值颜色和透明度。条带包含粒子受影响器作用后的轨迹，回拨时间可重建同一形状。尚未移动的粒子会随位移逐渐展开拖尾。

| 字段 | 用法 |
| --- | --- |
| `trail-end-scale: float` | 尾端宽度倍率，范围 0–2，默认 0.12。头部宽度由粒子尺寸决定。 |
| `trail-fade: float` | 尾端透明度损失，范围 0–1，默认 1。设为 0 保留粒子在对应年龄的透明度。 |
| `trail-eol-fade: float` | 寿命结束后冻结最后一条轨迹，再用指定秒数淡出，默认 0。淡出中的拖尾仍计入粒子预算。 |

拖尾沿用粒子深度测试和透明混合。贴图沿整条拖尾绷开，序列帧取头部年龄，颜色表按各轨迹点的年龄取色。相同段数并共用贴图和材质的粒子会合批。 [粒子拖尾示例](../examples/puzzle3d/particle_trails.xr)

发射器可用 `particle-model: string` 绘制模型粒子。接受 `@mesh-url("models/shape.glb")` 或上表中的任意内置图元。模型几何取 glTF 初始姿态，沿用模型材质、贴图、深度测试和光照，并接收场景阴影。粒子颜色与模型颜色相乘，`size` 和 `end-size` 是模型缩放倍率。粒子使用同一套发射位置、速度、影响器、颜色表和时间回拨。

`particle-rotation: vector3d` 分别指定 X、Y、Z 轴的初始角度，默认 0 度，按 X、Y、Z 顺序应用。`particle-rotation-variation: vector3d` 为各轴增加按种子固定的对称随机角度，`particle-angular-velocity: vector3d` 为每秒转过的角度。两者默认零向量。

同一模型及颜色表的粒子可以合批。GPU 批量写入常规 3D 实例变换，几何和贴图保留在 GPU 上。带透明材质或正在淡出的模型粒子会启用视口的加权透明。 [模型粒子示例](../examples/puzzle3d/particle_models.xr)

`particle-casts-shadow: bool` 将模型粒子加入方向光、聚光和点光的阴影贴图，需要在视口启用 `shadows`。默认关闭。投影使用不透明或 Alpha 裁切的模型材质，粒子自身的不透明度至少为 0.5 时写入阴影。可通过 `particle-receives-shadow: bool` 独立控制粒子接收阴影，默认开启。

`particle-casts-reflections: bool` 将模型粒子加入实时场景反射探针的捕获，默认关闭。探针的层遮罩、刷新模式和分帧设置同样生效，逐面捕获共用开始捕获时的粒子时间和发射器状态。 [粒子阴影与反射示例](../examples/puzzle3d/particle_lighting.xr)

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
