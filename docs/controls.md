# 控件与布局参考

[返回项目首页](../README.md)

列表、树和表格见 [模型与虚拟视图](models.md)。控件的完整可运行用例见 [examples](../examples/)。

## 主题、布局和动画

窗口可设置 `color-scheme: system;` 跟随宿主系统主题，或用 `light`、`dark` 固定应用内容的配色。`accent-color` 可绑定到应用状态，用于按钮、滑条、焦点和选中态。主题同时应用到窗口内的菜单和弹层。控件显式指定的背景和前景会覆盖调色板默认值。省略主题属性时沿用原有配色。运行 `bazel run //examples/controls:controls -- --theme` 可实时切换三种模式。

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

## 替换控件部件

在 `Button` 下声明一个 `ButtonContent`，可用任意视觉子树替代默认文字与图标。在 `Slider` 下声明一个 `SliderHandle`，可自定义手柄的颜色、边框和尺寸。按键、焦点、指针和无障碍操作由父控件处理，部件内部不会形成额外的焦点目标。自定义按钮仍可通过 `text` 或 `accessible-name` 提供辅助技术使用的名称。

`bazel run //examples/slots:slots` 展示有内容插槽的设置面板、带计数的按钮内容和自定义音量手柄。用 Tab 切换焦点，Space 激活按钮，方向键调整音量。

## 文字和输入

`StyledText` 接收 `styled-text` 值并复用标签的布局属性。``@markdown("**粗体**、*斜体*和 `xrackc`")`` 在编译时生成文字和字素对齐的样式区间。支持段落、标题、粗体、斜体、删除线、代码和链接。图片和 HTML 会产生诊断。Rust 可用 `StyledText::from_markdown` 解析动态内容，也可用 `StyledText::from_parts` 提供有样式区间和链接的文本。
Markdown 链接用 `[文案](目标)` 声明，默认显示下划线并可配置 `link-color`。鼠标和 Tab / Enter 均可触发 `link-activated(target)`，应用收到字符串后决定如何处理。布局截断和祖先裁剪会同步限制链接的命中范围和焦点。运行 `bazel run //examples/layout:layout -- --styled-text` 体验换行和截断。

窗口可绑定 `default-font-family`、`default-font-size`、`default-font-weight` 和 `default-font-italic`。文字、按钮和输入控件可分别用 `font-family`、`font-size`、`font-weight` 和 `font-italic` 覆盖窗口设置。字体可用系统字体族名或 `monospace`。缺少所选字形时继续用系统回退字体。字重使用 100 到 900，400 为常规字重，0 沿用默认值。可变字体使用支持的字重和斜体轴。布局、GPU 绘制、编辑器光标和无障碍文字范围共用字体选择。运行 `bazel run //examples/layout:layout -- --fonts`，切换等宽字体和字号，观察单行输入和多行换行。

在 `.xr` 文件顶部写 `import "fonts/example.ttf";` 可内嵌本地 TTF、OTF 或 TTC 字体。路径相对于声明所在的 `.xr` 文件，Bazel 目标通过 `xr_ui_library` 的 `assets` 列出字体文件。通过字体内部的字体族名设置 `font-family` 或 `default-font-family`。内嵌字体优先于系统同名字体，TTC 可按字重和斜体选择文件中的字体变体。

编译器校验字体并把字节嵌入生成的 Rust。同一文件在组件导入链中出现多次时只嵌入一次。窗口使用各自的字体上下文，Rust 的离屏渲染也可以选择 `TextSystem::bundled_only()`。运行 `bazel run //examples/layout:layout -- --bundled-fonts` 可查看内嵌的两款字重和一个使用内嵌字体的输入框。

`TextInput` 和 `LineEdit` 提供单行键盘编辑、选区、IME 预编辑和提交。`text <=> root.query;` 与 `checked <=> root.flag;` 分别把文本输入和复选框与可写属性相连。条件子元素用 `if condition : Element { ... }` 表达，隐藏时保留组件的内部状态。重复元素用 `for item[index] in root.items : Text { ... }` 表达，接受整数计数、`[string]` 和命名记录列表，以匿名、无状态的显示模板生成纵向列表。

在单行输入中设置 `input-type: password;` 后，绘制和光标按字素簇使用圆点，IME 预编辑也会遮罩。应用通过 `text` 绑定读写真值。设置 `password-visible: true;` 可显式显示原文，剪贴板复制、剪切仍被禁用，无障碍树仍只发布遮罩后的值和选区。可粘贴到可编辑的密码字段。运行 `bazel run //examples/form:form -- --password` 体验。

`TextInput` 和 `LineEdit` 的 `input-type: number;` 接受 ASCII 数字，`decimal` 允许前置正负号和一个小数分隔符。`decimal-separator` 默认为 `"."`，也可设为 `","`。空文本、单独的符号或分隔符可作为编辑中间态，按 Enter 提交需至少有一位数字。`input-type: search;` 将单行输入向辅助技术标记为搜索框。

单行和多行编辑器可用 `maximum-length` 限制 Unicode 字素簇数，负值表示不限。键盘、粘贴、IME 提交和无障碍替换都校验整次编辑。无效输入会保留原值、选区和撤销记录。应用写入的值保持原样，超长文本可逐步删到限制以内。运行 `bazel run //examples/form:form -- --validation` 体验。

## 导航

`StackView` 的直接子元素为带 ID 的静态 `StackPage`。例如 `home := StackPage { ... }` 的路由是 `"home"`。`initial-page` 设定初始路由，默认使用首个页面。事件处理器可调用 `navigation.push("details")`、`pop()`、`replace("summary")`、`pop-to("home")` 或 `reset("home")`。导航经 `xr.navigate` 校验，成功时触发 `changed(page, depth)`。同一路由在栈中占一个位置，根页可用 `reset` 更换。Rust 可通过 `navigate_stack(id, action)` 和 `stack_snapshot(id)` 操作和读取导航栈。

`navigation.current-page`、`navigation.depth` 和 `navigation.can-go-back` 是可在绑定及纯回调中读取的实时状态。读取经 `xr.stack_read` 保留在 MLIR 中，源码语义检查会拒绝 `initial-page` 经属性或回调形成的初始化环。一次导航前后的读取各自获取当时的栈状态。

`StackPage.active` 表示页面是否位于栈顶，`stack-index` 返回页面在栈中的零基索引，离栈时返回 `-1`。离开栈顶或移除栈中页面前，`leave-requested(destination, removing)` 可返回 `false` 取消整个导航，并可在处理器里显示确认对话框。栈顶先检查，再按栈顶到栈底的顺序检查其他将被移除的页面。也可声明 `callback can-leave-preview <=> preview.leave-requested;`，由 Rust 的 `on_can_leave_preview` 提供守卫。

导航提交后依次触发旧栈顶的 `deactivated()`、各离栈页面的 `removed()`、新栈顶的 `activated()`，最后通知 `StackView.changed(page, depth)`。这些处理器读取到已提交的栈状态。初始路由通过 `active` 和 `stack-index` 可读，生命周期事件在导航提交时触发。运行 `bazel run //examples/navigation:navigation -- --stack-guard` 体验带未保存草稿的页面。

`navigation.reset-to(["home", "details", "preview"])` 用完整路径恢复导航栈，`push-pages([...])` 则把一段路径追加到现有栈。路径可以来自 `[string]` 属性。整段路径都有效且路由唯一时才提交一次并触发一次 `changed`，当前只求值最终的栈顶页面。

只生成栈顶页面的场景，已声明页面的组件状态在返回时可继续使用。焦点位于导航栈内时，切页会记住原页面的焦点。Alt+左方向键或系统返回键可返回上一层。返回请求携带场景中的栈版本，提交时会核对实时版本。运行 `bazel run //examples/navigation:navigation -- --stack` 体验。

在 `StackView` 设置 `transition-duration: 220;` 可启用 220 毫秒的横向切页过渡。返回到当前路径上的祖先页面时从左侧进入，其他切页从右侧进入。离场页沿用已布局的绘制内容，栈顶状态和键盘焦点立即切换。过渡期间暂停该区域的鼠标与触摸操作，连续导航会从正在显示的页面片段开始下一次过渡。默认立即切页，时长为零或负数时也是如此。过渡期间视口几何变化会直接显示当前页。

## 表单与选择控件

`Slider` 的 `value` 可双向绑定到 `float` 属性。鼠标拖动连续更新数值，方向键按 `step` 调整，Home 和 End 移到端点。`changed(value)` 只在交互写入新值时调用，`released(value)` 在鼠标或调节键松开后调用。`orientation: vertical;` 将最小值放在下方。`Switch` 支持 `checked` 双向绑定与 `toggled()`。`ProgressIndicator` 接收 `0..1` 或百分比进度，`indeterminate: true;` 显示运动指示条。

`Spinner` 使用圆形轨道显示同样的 `progress` 和 `indeterminate` 状态。`BusyIndicator` 在 `running: true;` 时显示旋转的弧线，停止时隐藏并保留布局占位。三种指示器都支持 `color`、`track-color` 和 `animated: false;`，关闭动画会保留可见状态和无障碍进度。圆形指示器还支持 `stroke-width`。

运行 `bazel run //examples/controls:controls -- --indicators` 比较三种指示器，或运行 `bazel run //examples/async_demo:async_demo` 查看与后台任务绑定的加载状态。离屏验证可用 `Renderer::snapshot_at` 指定指示器的采样时间。

`CheckBox` 支持布尔 `checked`，也可使用类型化的 `check-state`。三种状态是 `CheckState.unchecked`、`CheckState.partially-checked` 和 `CheckState.checked`。设置 `tristate: true;` 后，用户交互会循环三种状态。默认交互会把部分选中切到已选中。三态模式选择 `check-state` 或 `tristate`，布尔模式使用 `checked`。

`check-state: expression;` 保留单向绑定，交互通过 `check-state-changed(state)` 报告请求的状态，适合汇总子项选择。`check-state <=> root.state;` 直接写入可写属性，也可绑定表格或树行的 `CheckState` 字段。运行 `bazel run //examples/controls:controls -- --check-state` 体验全选和独立三态循环。

`RangeSlider` 使用 `lower-value` 和 `upper-value` 选择一个区间，两端可分别双向绑定到不同的 `float` 属性。Tab 在两个手柄之间移动焦点，方向键按 `step` 调整，Home 和 End 移到当前手柄可达的边界。点击轨道移动最近的手柄。手柄相遇时，向较小值拖动下限，向较大值拖动上限。横向和纵向都使用 `changed(lower, upper)` 和 `released(lower, upper)` 报告有效区间。外部写入交叉值时，显示的下限收拢到上限，下一次有效交互会写回有序区间。运行 `bazel run //examples/controls:controls` 体验。

`Slider` 和 `RangeSlider` 支持 `snap-mode`。用 `ScrollSnapMode.always` 在拖动时吸附到以最小值为起点的步进刻度，或用 `on-release` 在松手时吸附。最大值也可作为终点。设置 `live: false;` 后，鼠标和触摸拖动先预览手柄位置，松手后提交，数值变化时触发 `changed`，随后触发 `released`。Escape 可取消待提交的预览，键盘和辅助技术可以直接调整数值。运行 `bazel run //examples/controls:controls -- --slider-snap` 体验。

`Slider`、`RangeSlider`、`Dial`、`SpinBox` 和 `DoubleSpinBox` 可设置 `wheel-enabled: true`。滚轮向上或向右增加数值，`RangeSlider` 调整靠近指针的手柄。滚轮按系统报告的行增量计步，触控板每 28 个逻辑像素累计为一步。一次输入的多步合并提交，数值变化时触发 `changed` 或 `edited`。上层 `WheelHandler` 仍可先消费输入。非循环控件到达边界后，同方向的后续滚轮会交回外层视口。带修饰键的滚轮沿用原路由。运行 `bazel run //examples/controls:controls -- --wheel` 可试用。

`Pane` 提供主题背景，`ControlFrame` 另有一像素的主题边框。两者默认内边距为 12 个逻辑像素，可分别设置四边。子元素的坐标相对于内边距内的内容区，也可以放入布局容器。未指定外部尺寸时会测量可见内容，`content-width/height` 可覆盖隐式内容尺寸。`available-width/height` 是布局完成后发布的只读内容区尺寸，子元素可用百分比尺寸填满内容区。空白区域拦截点击，`enabled: false` 会禁用其中的交互，`clip: true` 会按圆角裁剪子元素。运行 `bazel run //examples/controls:controls -- --panes` 可试用。

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

`accepted(string)` 报告显式提交的查询，`selected(string)` 报告选中的候选。两种操作都触发 `search-triggered(string)`。`live: true;` 也会在输入变化时触发它。`bazel run //examples/async_demo:async_demo` 演示用 Tokio 查询并在 UI 线程写回候选，示例单独维护一批候选和虚拟化的结果列表。

`RadioGroup` 接收静态 `RadioButton`，也可以用一个 `for` 生成所有选项。通过组的 `current-index` 或 `current-value` 双向绑定选择，按钮的选中状态由组推导。组支持横竖排列、方向键导航和 `selected(string)`。

组件根部也可声明非可视的 `ButtonGroup`，各个 `RadioButton` 用 `group: group-id;` 加入同一组，可分别放在不同的行、列或条件容器中。`current-index` 按声明顺序分配槽位，隐藏项保留槽位。`RadioButton.value` 可提供稳定的业务值，`current-value` 和 `selected(string)` 使用该值，省略时沿用 `text`。每组在 Tab 顺序中只占一站，方向键在可用选项间循环。设置组的 `enabled: false;` 停用整组，`accessible-name` 会用于无障碍分组。运行 `bazel run //examples/selection:selection -- --radio-groups` 体验。

`ButtonGroup { exclusive: false; }` 可组合 `CheckBox` 和 `Switch`。全选框用 `check-state <=> options.check-state;` 绑定汇总结果，普通成员各自保留 Tab 停靠点。汇总使用声明中的所有成员，包括隐藏或禁用的选项。`options.set-check-state(CheckState.checked)` 和 `options.set-check-state(CheckState.unchecked)` 分别批量选中和清除，传入部分选中值会保留当前成员状态。批量操作通过各成员的绑定更新属性，成员的用户操作回调由各自的交互触发。运行 `bazel run //examples/controls:controls -- --check-group` 体验。

`TabWidget` 接收静态 `Tab` 子元素，可双向绑定 `current-index`，并通过 `orientation` 切换顶部或左侧标签栏。非活动页面保留组件状态，只有活动页构建内容场景。方向键跳过禁用项，Tab 键把每组选项视为一个焦点停靠点。

`TabBar` 独立排列 `TabButton`，可与 `SwipeView` 双向绑定同一个 `current-index`。标签可逐项声明，也可用一个 `for` 从模型生成。横排默认按文字测量宽度，可用 `width` 固定某项。竖排使用 `height`。超出视口时可用滚动条、触摸或沿排列方向滚动浏览。方向键和 Home / End 跳过隐藏或禁用项，绑定变化后会把新的选中项滚入视口。隐藏的标签保留声明索引。运行 `bazel run //examples/navigation:navigation -- --tab-bar` 体验。

## 日期和时间

`Calendar` 的 `date` 和 `display-month` 使用 `Date { year: 2024, month: 2, day: 29 }`，可分别双向绑定。点击日期或按方向键选择，Home / End 跳到该月可用的首尾日期。翻月按钮和 Page Up / Page Down 更新显示月份并触发 `month-changed(month)`。选中日期变化时触发 `changed(date)`。

日历使用公历，年份范围为 1–9999，`minimum-date` 和 `maximum-date` 可进一步限制选择。默认日期是 `1970-01-01`，应用可传入当天或业务日期。`week-start` 按周一为 1 到周日为 7 编号，`weekday-labels` 接收从周一起排列的自定义星期名。

`DatePickerPopup` 通过 `open` 打开，复用 `Calendar` 的日期范围、星期设置和键盘导航。打开时从 `date` 读取独立草稿，翻月只改变视图。确认时按当前范围调整草稿，写回双向绑定并触发 `accepted(date)`，取消或按 Escape 触发 `canceled()`。`title`、`accept-text` 和 `cancel-text` 可自定义。运行 `bazel run //examples/controls:controls -- --calendar` 体验内联日历和日期弹层。

`MonthGrid` 提供六行日期网格，可与 `DayOfWeekRow` 和 `WeekNumberColumn` 组合。让星期行和月份网格同宽、周数列和月份网格同高，并为三个控件绑定同一 `week-start`。周数列和月份网格共享 `display-month`。周数采用 ISO 8601，每行按该行的星期四归属，跨年时可显示上一年最后一周或下一年第一周。

月份网格中的 `date` 和 `display-month` 可分别双向绑定。点击可用的相邻月份日期只更新选择，Page Up / Page Down 翻月。方向键改变日期，超出可见网格时随日期翻月。点击、Enter 或空格触发 `activated(date)`，包括再次触发同一日期。应用可传入 `today` 标记当天。运行 `bazel run //examples/controls:controls -- --month-grid` 体验自定义标题和周数列。

在 `MonthGrid` 内声明一个匿名 `DayCell`，即可自定义每个日期的视觉内容。模板可读取 `cell-date`、零基 `cell-index`、`cell-weekday`、`cell-week`、`cell-week-year`、`cell-selected`、`cell-today`、`cell-in-month` 和 `cell-enabled`。`cell-week` 和 `cell-week-year` 表示该日期的 ISO 周数和周所属年份。网格为每个单元分配并裁剪内容区域，同时处理选择、键盘导航和无障碍日期动作。模板的 `accessible-description` 可补充日程信息。每个日期独立缓存模板绑定。运行 `bazel run //examples/controls:controls -- --calendar-agenda` 查看带日程标签的月份网格。

`TimePicker` 和 `TimePickerPopup` 使用 `Time { hour: 23, minute: 58, second: 41 }`。字段省略时默认为零。构造时小时限制到 0–23，分秒限制到 0–59。`time` 可双向绑定，内联滚轮改值后触发 `changed(time)`。默认显示 24 小时制，设置 `use-24-hour-format: false;` 可显示 12 小时制和 AM/PM。`show-seconds: true;` 显示秒，隐藏时保留原值。滚轮支持鼠标、键盘和辅助技术操作。

`TimePickerPopup` 通过 `open` 打开，打开时读取一次 `time` 作为草稿。确认后写回并触发 `accepted(time)`，取消或按 Escape 触发 `canceled()`。可用 `title`、`accept-text` 和 `cancel-text` 自定义文字。运行 `bazel run //examples/controls:controls -- --time` 体验。

`ColorPickerPopup` 支持 RGBA 滑条、0–255 数值输入和十六进制编辑。`show-color-plane: true;` 打开 HSV 色板和色相滑条，鼠标或触摸可拖动色板，方向键分别调整饱和度和明度。所有输入共享草稿并实时更新棋盘格上的预览。十六进制输入接受六位 RGB，也可以在 `show-alpha: true;` 时输入八位 RGBA。六位输入和隐藏透明度控件都会保留原有 alpha。格式无效时保留上一次有效预览，修正后可确认。

`open` 和 `color` 支持双向绑定。确认时写回颜色并触发 `accepted(color)`，取消或按 Escape 丢弃草稿并触发 `canceled()`。运行 `bazel run //examples/controls:controls -- --color` 体验带色板的版本。

`FontPickerPopup` 通过 `open` 打开，使用当前窗口渲染器的系统字体和内嵌字体。可选择字体族、逻辑像素字号、1–1000 的字重以及斜体，`preview-text` 自定义预览内容。`font` 支持双向绑定，确认时一次写回整个 `Font` 并触发 `accepted(font)`，取消或按 Escape 丢弃草稿并触发 `canceled()`。用 `Text { font: root.selected; }` 应用选择，或将窗口的 `default-font` 绑定到同一值。单独设置的 `font-family`、`font-size`、`font-weight` 和 `font-italic` 覆盖对应字段。构造示例为 `Font { family: "sans-serif", size: 16px, weight: 400, italic: false }`。运行 `bazel run //examples/controls:controls -- --font-picker` 体验。

在组件根部声明命名的 `FileDialog`，从回调调用 `picker.open()`。`mode` 可取 `open-file`、`open-files`、`save-file`、`select-folder` 或 `select-folders`。可设置 `directory`、`file-name` 和 `name-filters`，扩展名过滤用 `"Text files (*.txt *.md)"` 这样的形式。宿主异步显示系统面板，并在 UI 线程触发 `accepted([string])`、`canceled()` 或 `failed(string)`。应用收到 UTF-8 本地路径后自行读写文件。同一窗口串行显示面板，同一个 `FileDialog` 的请求未结束时会合并重复调用。发起组件离开活动场景后，宿主丢弃该请求的结果。运行 `bazel run //examples/form:form -- --files` 体验。

`FileDialog.selected-files` 和 `selected-file` 可绑定到界面，分别提供上次确认的全部路径和第一条路径。确认结果在 `accepted` 执行前提交。取消、失败和弃用回执保留上一次结果。`FileDialog` 与 `MessageDialog` 都提供只读的 `pending`，排队和显示面板期间为真，结果回调执行前恢复为假。

`MessageDialog` 也通过 `open()` 打开系统对话框，可设置 `title`、`text`、`informative-text` 和 `level`。`buttons` 可取 `ok`、`ok-cancel`、`yes-no` 或 `yes-no-cancel`。确认和拒绝分别触发 `accepted()` 与 `rejected()`，随后 `finished(StandardButtonKind)` 报告具体结果。系统关闭或取消返回 `cancel`，宿主检测到的打开错误通过 `failed(string)` 返回。文件面板与消息对话框按请求顺序共用窗口的异步队列。运行 `bazel run //examples/form:form -- --message` 体验。

`SplitView` 把直接子元素排成横向或纵向面板，支持嵌套。面板沿分栏方向使用 `min-width`、`preferred-width`、`max-width` 或对应的高度属性。拖动分隔线只调整相邻的可见面板，并在当前实例中保存比例。用 Tab 聚焦分隔线后，可用方向键步进，Home 和 End 到达两个边界。`handle-size` 和 `handle-color` 设置分隔线，`resized()` 在比例更新后触发。隐藏面板不占空间，再显示时会恢复其比例。

`SplitView.save-state()` 返回可持久化的布局字符串，`restore-state(state)` 在回调中恢复比例。命名面板按 ID 匹配，匿名面板按声明位置匹配。恢复会校验版本、面板集合和比例，再按当前尺寸约束布局。Rust 可用 `save_split_state(id)` 和 `restore_split_state(id, state)` 读写状态，后者返回是否接受了字符串。保存数据由应用负责。运行 `bazel run //examples/split:split`，用「保存布局」和「恢复布局」体验。

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

单行、多行、搜索框、可编辑下拉框、数字输入框和正在编辑的表格或树单元格提供默认编辑菜单。右键、Shift+F10 或辅助技术均可打开。菜单与键盘共用选区、剪贴板和撤销记录。只读框可复制，密码框禁用复制和剪切。数字框的撤销和重做作用于草稿，提交时再更新绑定的数值。`ContextMenuArea` 优先，`context-menu-enabled: false` 可关闭默认菜单。菜单打开后，外部修改当前文本或选区会使旧操作失效。运行 `bazel run //examples/form:form -- --editor-menus` 体验。

`MenuItem` 的 `shortcut: "Primary+Shift+E";` 会在菜单中显示组合键，并与点击共用 `activated()`。`Primary` 在 macOS 上表示 Command，在其他平台表示 Ctrl。也可以组合 `Ctrl`、`Alt`、`Shift` 和 `Super`，主键支持英文字母、数字和 F1 到 F12。字母和数字需要 Ctrl、Alt、Super 或 Primary 中的一个修饰键。无效字符串不会注册快捷键，可将绑定设为空字符串清除快捷键。

菜单栏快捷键在当前窗口或最上层模态区域内生效。打开右键菜单时，只处理该菜单内的快捷键。隐藏和禁用项不参与匹配，同一作用域内的重复组合键不会触发命令。文本编辑保留选择、剪贴板和撤销重做快捷键。运行 `bazel run //examples/menu:menu` 可体验。

`Shortcut` 可声明在布局或对话框内，不占空间。用 `sequence: "Primary+K, Primary+P";` 声明最多四段组合键，相邻两段须在一秒内输入。`sequences` 可提供多个别名，`focus-within: true;` 将范围缩到父元素的焦点子树，`auto-repeat: true;` 允许单段快捷键随按住重复触发。不同命令匹配同一序列时，独立 `Shortcut` 会收到 `activated-ambiguously()`。短序列同时是长序列的前缀时，短序列报告冲突，长序列可以继续输入。用 `action` 复用命令时，未指定序列会采用命令的 `shortcut`，同一命令的多个入口只执行一次。运行 `bazel run //examples/menu:menu -- --shortcuts` 体验。

在组件根部声明命名的 `Action` 后，`Button`、`ToolButton`、`MenuItem` 和虚拟列表中的 `ItemDelegate` 可以通过 `action: command-id;` 共享命令。命令集中声明 `text`、`icon`、`shortcut`、`enabled`、`checkable`、`checked` 和 `activated()`。可勾选命令先切换 `checked` 再执行回调，也可用 `checked <=> root.some-flag;` 关联组件属性。控件可以覆盖显示文本和图标，也可额外限制自身的 `enabled`。同一命令的多个控件共用一个快捷键，按钮单独展示时也可以触发命令。每个组件实例独立持有命令状态。

`ActionGroup` 可以包含命名的 `Action`，也可以让组件根部的命令用 `group: group-id;` 加入。互斥组默认只勾选 `current-value` 对应的命令。命令的 `value` 在组内唯一，省略时用命令 ID。用 `current-value <=> root.mode;` 和业务状态双向绑定，也可在一个成员上用 `checked: true;` 初始化。再次触发已勾选的命令会继续执行回调并保持选中。组内的普通命令可设 `checkable: false;`。

设置 `exclusive: false;` 后，每个成员独立维护 `checked`。两种组都支持 `enabled`，禁用后菜单、工具栏和快捷键统一停止触发。命令自己的 `activated()` 在状态更新后先执行，随后调用组的 `activated(value)`。运行 `bazel run //examples/menu:menu -- --action-groups` 体验。

`ToolBar` 默认横向排列子项，可用 `orientation: vertical;` 改为纵排。设置 `wrap: true;` 后可随可用空间换行或换列。工具按钮支持独立的 `clicked()`，也可绑定命令以同步勾选状态。`Button` 和 `ToolButton` 可用 `icon-size` 和 `icon-only` 控制图标显示。纯图标模式在没有图标时回退到文字。`icon` 使用已声明的 PNG 或 SVG 资源。运行 `bazel run //examples/menu:menu` 可体验共享图标和工具栏换行。

`RoundButton` 沿用按钮的文字、图标、键盘激活和 `Action` 绑定。默认半径为宽高中较短边的一半，正方形尺寸会绘制为圆形，也可用 `radius` 指定半径。绘制和指针命中共用圆角几何，透明角落会把点击交给下层。`ToolSeparator` 默认绘制竖线，可在纵向工具栏里设 `orientation: horizontal`。`stroke-width`、`padding` 和 `color` 分别控制线宽、端部留白和颜色。分隔线参与布局，点击可以穿过它。运行 `bazel run //examples/menu:menu -- --round-tools` 可试用。

在控件或视觉元素内声明 `Tooltip { text: "操作说明"; }`，可在悬停或通过 Tab 聚焦后显示提示。每个元素可附加一个 `Tooltip`，内容可用纯文本或单个自定义布局。`delay` 默认 600 毫秒，`timeout` 默认持续到离开、失焦或发生输入。提示浮在窗口内，靠近边缘时会避让，点击会穿过浮层。菜单和模态作用域会限制提示显示。

## 图像和路径

`Image` 的 `source: @image-url("assets/icon.svg");` 引用相对于 `.xr` 的本地 PNG 或 SVG。图片以 `!xr.image` 通过 MLIR，编译器检查资源后把编码字节嵌入 Rust。支持 `image-fit: fill | contain | cover | preserve`。布局内默认 `contain`，其他位置默认 `fill`。SVG 支持独立的矢量图形，由 resvg 按显示尺寸和 DPI 栅格化。SVG 文本和外链资源暂未支持。

`source-clip-x`、`source-clip-y`、`source-clip-width` 和 `source-clip-height` 以源像素选择图块。未指定的宽高延伸到源图边缘，超出源图的部分裁掉。裁剪后的宽高参与隐式尺寸和比例计算，`image` 值的 `.width`、`.height` 仍返回完整资源尺寸。`horizontal-alignment` 和 `vertical-alignment` 决定图像在未平铺轴上的位置，`image-rendering: pixelated;` 使用最近邻采样。普通图像和 GIF 帧也可按轴使用 `horizontal-tile-mode` 和 `vertical-tile-mode`。SVG 直接栅格化裁剪区域，PNG 图块使用独立纹理区域以隔离相邻像素。运行 `bazel run //examples/media:media -- --atlas`。

`Image` 和 `AnimatedImage` 的 `colorize` 接受纯色、线性或径向渐变画刷，并以图像透明度作为遮罩。完全透明的画刷恢复原图。渐变以完整源图坐标计算，裁剪后保留对应的颜色区间，平铺重复该区间，九宫格的各块沿用同一画刷。纯色使用缓存的单通道纹理，渐变在 GPU 上合成。运行 `bazel run //examples/media:media -- --colorize`。

用 `@image-url("assets/frame.svg", nine-slice(4 6 8 10))` 创建九宫格图像。边距按上、右、下、左顺序指定源像素数，也可以给一个统一值，或给纵向和横向两个值。编译器会检查图像是否保留了正尺寸的中心。九宫格填满 `Image`，四角保留逻辑尺寸，空间不足时同轴边距按比例收缩。`horizontal-tile-mode` 和 `vertical-tile-mode` 独立支持 `stretch`、`repeat` 和 `round`，分别拉伸、按原尺寸平铺并裁剪末片、缩放为整数个完整的平铺单元。Rust 可以用 `Image::with_nine_slice` 为已有资源设置边距。运行时图片可用 `Image::Loaded` 提供，图片值通过 `clone()` 共享底层数据。运行 `bazel run //examples/media:media -- --nine-slice` 查看对照。

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

### MediaPlayer

`MediaPlayer` 接收本地文件或 HTTP(S) URL。相对路径以声明控件的 `.xr` 文件所在目录为准。`autoplay` 默认为 `false`，`volume` 范围是 0 到 1，`muted` 可静音。`playback-rate` 支持 0.25 到 4 倍速，当前倍速会改变音高。`loops` 默认为 1，设为 -1 可循环播放可定位的媒体。

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

## 首版边界

当前的状态转场作用于视觉属性及组件实例的布局属性。标准控件仍在扩充。文件扩展名约定为 `.xr`，语法取 Slint 风格子集。

桌面宿主通过 AccessKit 发布可见控件、当前焦点和活动模态窗口的无障碍树。`accessible-name` 和 `accessible-description` 可补充控件语义，例如纯图标按钮的名称。当前支持聚焦、点击、文本值替换和数值步进。`ComboBox` 和菜单公开弹层内的可见选项，子菜单会跟随当前展开层级。滚动容器公开偏移和范围，可按项、按页或指定偏移滚动。

单行和多行文本框及可编辑 `ComboBox` 公开已提交的文本、光标和选区。辅助技术可读取文本范围并设置选区，操作沿用编辑器的 Unicode 字素边界与视觉换行。可见文本按方向拆分范围并发布字符坐标。IME 预编辑期间保留已提交的文本，选区操作在预编辑结束后恢复。

`xrack-layout` 计算线性布局、绝对坐标和祖先裁剪。隐藏元素不占布局空间。鼠标点击需要在同一目标内按下和释放，禁用或被裁剪的控件不会触发点击。winit 将鼠标、Tab / Shift+Tab 和 Enter / Space 事件交给该状态机。绑定变化、窗口缩放和 DPI 变化会触发重绘。

原生 GPU 路径目前在 macOS / Metal 验证。离屏 GPU 测试需要适用的图形适配器和支持中文的系统字体，需显式运行。单行文本输入已接入 IME 预编辑和提交。
