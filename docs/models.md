# 模型与虚拟视图

[返回项目首页](../README.md) · [控件与布局参考](controls.md)

## 滚动、列表和网格

`ScrollView`、`ListView`、`TreeView` 和 `TableView` 在内容溢出时显示可拖动的纵向滚动条。`ScrollView` 和 `TableView` 也提供横向滚动条。拖动滑块定位，点击轨道按视口大小翻页。滚动条覆盖在内容边缘并共享滚轮的滚动位置，表头与表身继续同步。可通过 `vertical-scroll-bar-policy` 和 `horizontal-scroll-bar-policy` 设置各滚动轴的 `as-needed`、`always-on` 或 `always-off`。拖动滚动条保留当前焦点，虚拟视图继续只创建视口内的行。

`ScrollBar { target: canvas; orientation: horizontal; }` 和 `ScrollIndicator { target: canvas; }` 可以在视口外独立布局，`target` 引用同组件中的视口 ID。方向默认为纵向，`policy` 默认为 `as-needed`，也可设为 `always-on` 或 `always-off`。滚动条支持鼠标、触摸、Tab 焦点、方向键、PageUp/PageDown、Home/End 和原生无障碍操作。指示器只绘制滑块，指针事件可以穿过它。内建滚动条、外置滚动条和指示器共享视口偏移，用户移动外置滚动条时照常写回 `content-x/y` 绑定并触发 `scrolled()`。运行 `bazel run //examples/list:list -- --scroll-bars` 体验。

附着模式的 `position` 和 `size` 是只读的归一化视口数据，分别为实际偏移和可见长度占内容总长度的比例。没有溢出时返回 `0` 和 `1`，视口尚未布局或被隐藏时都返回 `0`。滚动条的显示策略和最小滑块尺寸独立于这两个值。可以用这些读数驱动文本、颜色或另一个滚动指示器。编译器会检查它们流入当前布局阶段的尺寸或位置时是否形成反馈。

省略 `target` 即可使用独立滚动控件。`size` 是可见部分占总内容的比例，取值为 `0..1`，`position` 的有效范围为 `0..1-size`。用 `position <=> root.offset` 接收滚动条输入，或者在表达式中读取 `bar.position` 和 `bar.size` 来移动自定义内容或驱动指示器。`size` 改变时，有效位置会重新夹紧，原绑定值会保留。用户移动滑块时会写回位置并触发 `changed(position)`。`step-size` 控制方向键和无障碍增减的步长，默认 `0.1`。两种模式都支持 `minimum-size` 设置最小滑块比例，滚动条可用 `interactive: false` 切换为被动显示。运行 `bazel run //examples/list:list -- --standalone-scroll` 体验。

两种滚动条都支持 `snap-mode: ScrollSnapMode.always` 和 `ScrollSnapMode.on-release`，默认值是 `none`。设置正的 `step-size` 后，滑块拖动按可移动行程的比例吸附，起点和终点始终可达。附着式滚动条的方向键和无障碍增减会把显式 `step-size` 换算为总内容长度的比例，省略时仍以 28 逻辑像素为步长。轨道点击保留翻页行为，滚轮和程序定位继续使用视口偏移。取消拖动时保留已经提交的位置。可在 `--standalone-scroll` 示例中勾选 **Snap on release** 体验。

`ScrollView` 根据直接子项的位置和布局尺寸计算内容范围。`content-width` 和 `content-height` 可以用长度或视口百分比指定最小内容尺寸。子项的百分比尺寸以显式内容尺寸或视口为基准。`viewport-height` 仍可用于既有的最小内容高度绑定。`Shift` 加滚轮可横向滚动，嵌套视图会将各轴剩余滚动量交给外层。运行 `bazel run //examples/layout:layout` 可体验二维画布。

`Flickable` 是可拖动并带惯性的底层视口。它默认裁剪内容，滚动条可通过 `ScrollBar { target: canvas; }` 独立放置。`content-width` 和 `content-height` 给出各轴的显式内容范围，省略时按子项推导。`flick-direction` 可设为 `both`、`horizontal` 或 `vertical`，`interactive: false` 暂停拖动和惯性输入，子控件、滚轮和 `scroll-to()` 仍可操作。`dragging`、`flicking` 和 `moving` 反映当前消费位移的视口。`flicked()` 在用户改变偏移时触发，`cancel-flick()` 停止该视口正在参与的惯性滚动。运行 `bazel run //examples/layout:layout -- --flickable` 体验。

`bounds-behavior` 默认是 `FlickBoundsBehavior.stop-at-bounds`。可分别启用拖动越界 `drag-over-bounds`、惯性越界 `overshoot-bounds`，或同时启用 `drag-and-overshoot-bounds`。嵌套视口先消费合法范围内的位移，剩余部分才交给最外层启用弹性的视口。`horizontal-overshoot` 和 `vertical-overshoot` 是带符号的逻辑像素，`content-x/y` 仍表示合法的内容偏移。设 `bounds-movement: FlickBoundsMovement.stop-at-bounds` 可让内容停在边界，用读数驱动自定义效果。`rebound-duration` 默认 180 毫秒，`rebounding` 和 `moving` 在回弹期间为真。也可调用 `return-to-bounds()` 主动开始回弹。示例中的 **Elastic content** 切换内容是否随越界移动。

`ScrollView` 和虚拟视图可以在回调中调用 `view.scroll-to(96px, 60px)`，参数为从内容原点量起的逻辑像素。列表、网格、树和表格也可用 `view.position-view-at-index(9000, ScrollAlignment.center)`。索引从零开始，树视图使用当前展开后的可见行序列。对齐方式有 `beginning`、`center`、`end`、`visible` 和 `contain`。`visible` 在目标已有一部分可见时保留位置，`contain` 尽量完整显示目标行。

虚拟视图也可用 `position-view-at-source-index(source, alignment)`，表格可用 `position-view-at-source-cell(source, column, horizontal, vertical)`。源索引从零开始，列索引按声明顺序。调用时捕获源记录标识，布局时在最新的显示顺序中找到它，因此排队期间的排序和插入会保留定位目标。处理请求时，已隐藏、折叠、删除或来自旧模型的记录会被跳过。运行 `bazel run //examples/table:table -- --row-visibility`，用「定位源索引 24」在两个不同的显示顺序中找到同一条记录。

`view-index-for-source(source)` 和 `source-index-for-view(index)` 在源模型顺序和当前显示顺序之间转换。两者都从零开始，可用于属性绑定。隐藏、折叠或越界的记录返回 `-1`，排序、筛选和模型变更会更新绑定结果。回调可用 `select-source-index(source)` 选择列表、网格、树或表格中的记录，也可用 `select-source-cell(source, column)` 选择表格单元格。选择遵循控件的选择模式，列索引按声明顺序。需要同时滚动时，可以接着调用对应的定位命令。

`TableView` 和 `StandardTableView` 还提供 `position-view-at-cell(row, column, horizontal, vertical)`，两个方向分别接受 `ScrollAlignment`。`position-view-at-row(row, alignment)` 只移动纵轴，`position-view-at-column(column, alignment)` 只移动横轴。行号按当前显示顺序，列号按声明顺序，列重排后仍定位同一列。定位使用当前列宽、行高、行标题和冻结列的布局，冻结列保留横向偏移。这些命令保留光标和选区，无效单元格坐标取消整次定位。运行 `bazel run //examples/table:table -- --position-cells`，可在五万行模型中跳到末行末列，再重排列并比较定位结果。

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

`ApplicationWindow` 是可直接运行的窗口根元素，沿用 `Window` 的主题、标题、尺寸和关闭回调。直接声明的 `MenuBar`、`PageHeader` 和 `PageFooter` 各最多一个，依次放在窗口顶部、菜单栏下方和底部。正文位于剩余区域，`padding` 及四边覆盖只作用于正文。`available-width/height` 发布实际正文尺寸，正文子项可用百分比填满。正文滚动时，菜单栏、页眉和页脚保持固定，弹层仍以窗口为范围。Tab 和无障碍按菜单栏、页眉、正文、页脚遍历。运行 `bazel run //examples/navigation:navigation -- --application-window` 可试用。

`SwipeView` 的直接子元素为静态 `SwipePage`。用 `current-index <=> root.page;` 双向绑定声明顺序中的页索引，通过 `orientation` 选择横向或纵向。可用触摸、中键拖动或聚焦后按方向键切页，Home 和 End 跳到首尾可用页。手势在松手时切页，页内同轴滚动优先，滚到边界后下一次滑动可以切页。`interactive: false;` 暂停手势和键盘切页，应用仍可写入页索引。

隐藏或禁用的页面在导航中跳过。索引不可用时显示首个可用页，用户切页后才写入新索引并触发 `changed(index)`。只有当前页的内容参与场景求值，页面里的组件状态会保留。运行 `bazel run //examples/navigation:navigation` 可体验。

`PageIndicator` 用 `count` 和 `current-index` 表示页数与当前页。它可以独立使用，也可以和 `SwipeView` 双向绑定到同一个页索引。设置 `interactive: true;` 后可点击点位，或聚焦后用方向键和 Home / End 切页，用户切页触发 `changed(index)`。`orientation` 控制横排或竖排，`dot-size`、`spacing`、`color` 和 `current-color` 控制外观。空间不足时，点位围绕当前页显示一段连续索引，最多绘制 64 个点，两端缩小的点提示还有页。点击仍使用点位对应的原始页索引。

## 列表、树和重复组件

`ListView` 接收一个 `for` 模板，支持整数计数、`[string]` 和命名记录列表，最多寻址 100 万行。固定的 `row-height` 决定滚动位置与行索引，只有与视口相交的行才会生成。`current-index` 可双向绑定，鼠标选择和键盘方向键、Home、End、PageUp、PageDown 会更新索引。选择按位置解释，插入或删除行后仍使用当前索引。

`ListView`、`GridView` 和 `StandardListView` 支持 `row-visible-field: "flags.shown";`。布尔字段为 `false` 的记录退出视图，网格用剩下的记录重新排列。视图的 `current-index` 和滚动命令使用显示索引，`for entry[source]` 中的 `source` 仍是源模型索引。编辑和行组件状态按记录标识定位，开启 `preserve-scroll-anchor` 时可在隐藏视口前的记录后保持锚点。运行 `bazel run //examples/list:list -- --visibility` 体验三个共享模型的视图。

`ItemDelegate`、`CheckDelegate`、`RadioDelegate` 和 `SwitchDelegate` 可作为 `ListView` 或 `GridView` 的直接行模板。整行可点击，默认高亮当前项，也可显式绑定 `highlighted`。聚焦列表后用方向键选行，按 Enter 或空格触发当前委托。`ItemDelegate.clicked()` 可读取重复项和索引，`RadioDelegate` 的勾选由列表的 `current-index` 派生。

直接重复的 `CheckDelegate` 和 `SwitchDelegate` 用 `checked <=> row.field` 写回布尔字段。`CheckDelegate` 也支持 `check-state <=> row.state` 和 `tristate: true;`。交互会核对行标识和字段原值，确认写入后才执行状态回调。运行 `bazel run //examples/list:list -- --item-delegates` 查看四种控件。

`TreeView` 接收一个重复的 `TreeRow`。在模板中用 `key: record.id; parent-key: record.parent-id;` 把两个整数记录字段声明为树的角色。非负键唯一，找不到父节点的记录显示为根，重复键的记录会被排除。循环在最小键处断开父链接。`current-key` 可双向绑定，展开状态按模型行标识保留，排序不会把展开状态转给别的记录。点击三角或按 Enter、Space 可切换展开，左右键可展开、折叠或移动到父子节点。只有视口内的可见节点会创建行内容。运行 `bazel run //examples/tree:tree` 查看示例。

`TreeView` 提供 `expand(key)`、`collapse(key)`、`toggle-expanded(key)` 和 `expand-to(key)`。参数使用记录里的业务键。`expand-to` 展开目标的祖先路径，可结合 `row-for-key(key)` 和 `position-view-at-index(...)` 滚动到深层节点。`depth(key)` 返回当前视图中的层级，未知键返回 `-1`。`is-expanded(key)` 可读取当前展开状态，`row-for-key(key)` 在节点折叠隐藏时返回 `-1`。

`expand-recursively(key, depth)` 展开一整支子树，`depth` 为 `1` 时的效果等同于单项展开，`-1` 展开到叶子。`collapse-recursively(key)` 同时清除隐藏后代的展开状态。省略键或传入 `-1` 时操作当前视图中的整棵树。批量操作一次更新状态，并触发一次 `expanded(key)` 或 `collapsed(key)`，全树操作的事件键为 `-1`。折叠后，当前项回退到最近可见的祖先，多选仍按记录标识保留。Rust 可用 `command_tree(id, TreeCommand)` 和 `tree_snapshot(id)` 调用与读取。运行 `bazel run //examples/tree:tree -- --commands` 体验。

`TreeView.root-key` 指定浏览分支，默认 `-1` 显示完整模型。设为有效业务键时，从该节点的子项开始显示，子项的父键视为视图边界，缩进深度从零计算。未知键显示空视图。`depth`、`row-for-key` 和展开命令都针对当前分支，键盘导航停留在该分支内。切换分支后滚动位置回到顶部，显式绑定的滚动偏移仍由绑定控制。展开状态和多选按记录标识保存，切回原分支可继续使用。运行 `bazel run //examples/tree:tree -- --branches` 查看共享模型的两个独立视图。

`TreeView` 可用 `filter-field: "label"; filter-text: root.query;` 搜索当前分支。字段名支持嵌套路径，字段类型为 `string`。搜索去除查询首尾空白并忽略大小写，保留命中节点及其祖先。搜索期间路径自动展开，可以用方向键在父子节点间移动。清空查询后恢复原来的展开状态。筛选和编辑共用同一模型，只有视口内的行会创建控件。双树示例右侧提供搜索框。

`TreeView` 可用 `row-visible-field: "visibility.shown";` 从常量布尔字段路径读取可见性。值为 `false` 时隐藏该节点及其子树，搜索和 `root-key` 都遵守这层可见性。展开和选区按原业务键保留，重新显示时恢复。隐藏分支会取消其中的编辑草稿，拒绝基于旧场景发起的提交。开启 `preserve-scroll-anchor` 可在隐藏视口前方的子树时保持当前记录的位置。运行 `bazel run //examples/tree:tree -- --visibility` 切换分支并试用搜索。

树拓扑单独订阅模型的结构变化和当前视图使用的键字段。修改名称或数值时，只比较被修改行的键和父键，二者相同就复用拓扑和导航顺序。行内容按视口跟踪，视口外的字段变化可以保留当前帧缓存。插入、删除、排序和父子关系变化会在下次读取时更新拓扑。

`multi-select: true;` 启用多选，选区与 `current-key` 独立。Shift+点击或 Shift+方向键选择当前树顺序中的一段，Ctrl/Cmd+点击或 Ctrl/Cmd+Space 增减一行，Ctrl/Cmd+Shift 增加一段，Ctrl/Cmd+方向键只移动光标。Ctrl/Cmd+A 选择展开后可见的所有行，Escape 清空选区。折叠保留后代的选中状态，排序保留记录标识，删除或替换模型会清除失效的选项。`selection-changed(int)` 报告用户操作改变选区后的选中数量。Rust 宿主可通过场景节点的 `tree_selection` 读取当前业务键及其行标识。

`TreeRow` 可在布局中组合类型化编辑器。`TreeCell { text <=> record.label; }` 编辑字符串，`SpinBox { value <=> record.quota; }` 编辑整数，`Slider { value <=> record.load; }` 编辑浮点数，`CheckBox { checked <=> record.active; }` 或 `Switch` 编辑布尔值。绑定目标是可写列表中当前记录的字段。布局可用 `if` 条件显示，编辑器的路径需保持稳定。

双击文本单元格或按 F2 开始重命名，Enter 提交，Escape 放弃。聚焦树并选定当前行后，按 Tab 可进入该行可用的复选框、数字框或滑块。重排后仍在视口内的记录会保留草稿，折叠隐藏记录或外部修改被编辑字段会丢弃草稿。树和表格的编辑器共用字段提交逻辑，核对行标识和字段原值后再合并到最新记录。

`ListView` 的 `for` 也可直接实例化匿名行组件。组件可接收模型项和索引作为输入，包含输入框、按钮或其他控件。行状态按模型标识保留，滚出视口或因插入而移动时会跟随原行。已访问过的行组件保留到模型删除该行或替换模型。

虚拟行组件可以包含自己的 `ListView`、`GridView` 或 `StandardListView`。给内层视图设定高度后，两层视图分别按自己的可见区域生成行。内层滚动偏移随外层行实例保留。多个内层视图共享同一模型时，点击、焦点和无障碍操作仍能定位到各自的行。运行 `bazel run //examples/list:list -- --nested` 体验 1 万个分组共用 5 万条记录。

行组件也可嵌套 `TableView`、`StandardTableView` 和 `TreeView`。表格的横纵滚动、选区和编辑历史，以及树的展开状态，都随所属行实例保留。Tab 编辑、复制、粘贴和撤销沿同一条组件路径找到内层表格。异步粘贴完成时会再次校验外层行和目标模型。运行 `bazel run //examples/list:list -- --nested-data` 查看共享记录的表格和目录树。

## 表格

`TableView` 使用静态 `TableColumn` 和一个 `for … : TableRow`。每行按列顺序声明一个展示单元格，支持文本、图像及嵌套布局。列宽支持 `width`、`min-width`、`preferred-width`、`max-width` 和 `horizontal-stretch`。超宽时可用触控板或 Shift+滚轮同步滚动表头和单元格。表头固定在垂直滚动区域上方，只创建可见的行。`current-row` 支持双向绑定，鼠标选择或键盘导航触发 `current-row-changed(int)`。可排序表头交替发出 `sort-ascending(int)` 和 `sort-descending(int)`，应用可用 `VecModel::sort_by` 稳定排序并保留行标识。

`TableView` 和 `StandardTableView` 的只读属性 `current-sort-column`、`current-sort-descending` 提供当前排序标记。无排序标记时列索引为 `-1`。在回调中调用 `sort-by(column, descending)` 会设置标记，状态变化时触发对应的排序回调。列参数和读取结果均使用声明索引。`clear-sort()` 清除标记。Rust 可通过 `table_sort(id)` 和 `set_table_sort(id, sort)` 读取或设置同一状态。默认由应用的排序回调处理模型顺序。运行 `bazel run //examples/table:table -- --sort-state` 体验。

表格可用 `movable-columns: true;` 开启表头拖动，默认关闭。拖到目标列的前半或后半时会显示插入位置，松手后提交。靠近表头可见边缘时会自动横向滚动。列宽拖柄和短按排序仍按原操作处理。回调可调用 `table.move-column(0, 2)` 移动显示位置，两个参数都是从零开始的显示索引。`column-moved(column, position)` 给出被移动的声明列索引和新的显示索引。排序、单元格编辑和既有选区保留声明列，键盘导航、Shift 选区和无障碍阅读使用显示顺序。运行 `bazel run //examples/table:table -- --standard` 可试用表头拖动和程序化移动。

表格支持 `frozen-columns: 1;`。它冻结当前显示顺序中的前 N 个可见列，列重排或可见性变化后重新计算。冻结列随整表纵向滚动，横向滚动条和翻页只覆盖后面的滚动区。两区共享行标识、选区和编辑状态。窗口较窄且内容溢出时，布局减少实际冻结列数，为滚动区保留空间。默认值为 0。运行 `bazel run //examples/table:table -- --frozen` 可试用，Shift 加滚轮可横向滚动。

`TableColumn.visible` 可绑定应用状态，默认显示。隐藏列后，声明索引、重排顺序、手动列宽和选区仍保留。表头拖动按可见列命中，程序化 `move-column()` 按包含隐藏列的排列计数。键盘移动、剪贴板和无障碍按可见列处理。正在编辑的列隐藏时取消草稿，定位到隐藏列的请求会被拒绝。宽表示例的「编号列」和「二月列」可切换冻结列和普通列。

`TableView` 和 `StandardTableView` 同时按行和列的可见区域生成单元格。冻结区和滚动区分别计算裁剪，可见行共享一份列索引映射，场景和布局只保存已生成的单元格。事件路径沿用声明索引。Rust 可用 `Node::child_at(column)` 或 `Node::at_path(path)` 查找已生成的逻辑列，用 `materialized_children()` 遍历已生成的单元格及其声明索引。自定义单元格绑定在首次进入视口时求值，已经访问过的组件单元格随行保留状态。复制选区仍从模型读取，包括视口外的记录和列。运行 `bazel run //examples/table:table -- --wide` 查看五万行、十二列的示例。

表格可用 `row-header-width: 64px;` 显示左侧行标题，默认显示从 1 开始的显示行号。自定义表格在 `TableRow` 里绑定 `row-header-text`。标准表格用 `row-header-field: "name";` 投影记录字段，也接受点分路径。行标题复用可见行和行高，横向滚动时固定在左侧，启用行高调整后可在标题分隔线上拖动。点击标题可选中整行，单元格多选时选择该行所有单元格，列选择模式下保留已选列并移动当前行。辅助技术可读取可见的行标题及其行号。运行 `bazel run //examples/table:table -- --row-headers` 查看记录名称和冻结列，或用 `--row-numbers` 查看行号。

开启 `movable-rows: true;` 后可用鼠标拖动行标题。移动期间显示插入线，靠近上下边缘会滚动，松手提交。可用 Escape 取消。回调可调用 `table.move-row(from, to)`，两个参数为零基显示行号。提交后触发 `row-moved(model-row, from, to)`，第一个参数是源模型索引。行顺序属于表格实例，共享模型的其他视图可保留自己的顺序。当前行、选区和手动行高随记录移动，编辑和粘贴按显示位置写回对应记录。源模型插入、删除、排序或替换时恢复源顺序。运行 `bazel run //examples/table:table -- --move-rows`，拖动左侧设备名称体验。

触屏按住行标题 500 毫秒后可拖动该行，短按仍选择该行，提前滑动仍滚动表格。聚焦表格后按 Alt+↑ 或 Alt+↓ 可移动当前行，包括视口外的当前行。macOS 上用 Option 键。AccessKit 行标题公开上移、下移自定义动作，边界处只公开有效方向。

表格可用 `resizable-rows: true;` 开启拖动行缝调整行高，默认关闭。回调也可调用 `table.set-row-height(20, 64px)`，按当前模型索引指定行高。行高限制在 12–512 个逻辑像素，传入负长度可恢复该行的默认 `row-height`。调整后的尺寸跟随记录标识，插删和排序后仍属于同一记录，记录移除时清理。列表虚拟化、键盘翻页、行锚定和按索引定位共用这份几何数据。运行 `bazel run //examples/table:table -- --rows` 可试用。

基于记录模型的 `ListView`、`StandardListView`、`TreeView`、`TableView` 和 `StandardTableView` 可以用 `row-height-field: "height";` 指定行高字段。字段名须为常量，字段类型须为 `length`。非正或非有限值使用默认 `row-height`，正值裁剪到 12–512 个逻辑像素。用户拖动或命令设置的表格行高优先，清除覆盖后恢复记录中的最新高度。字段修改只更新对应记录的投影，树按展开后的顺序计算几何。运行 `bazel run //examples/table:table -- --row-field` 可查看共享模型的表格与列表，并对比记录修改和手动覆盖。

`StandardTableView` 可直接绑定记录列表，例如 `model: root.inventory; TableColumn { title: "设备"; field: "name"; }`。`field` 是编译期校验的字段名，支持字符串、整数、浮点数、布尔值、`Date` 和 `Time`。控件为可见行生成文本单元格，复用固定表头、列宽调整、选择和排序回调。默认将程序选中的行滚入视口，可通过 `follow-current: false;` 关闭。辅助技术可读取可见的表头、行列位置和单元格值。运行 `bazel run //examples/table:table -- --standard` 体验五万行库存表。

标准表格的列可用 `editable: true;` 或布尔绑定启用内置文本编辑器。声明可编辑列时，`model` 须直接引用可写的记录列表属性。编辑器按 `field` 的类型解析文本，也支持嵌套字段。整数和有限浮点数接受十进制文本，布尔值接受 `true`、`false`、`1` 和 `0`，日期和时间使用 `YYYY-MM-DD` 与 `HH:MM:SS`。无效草稿显示错误边框，按 Enter 或 Tab 后仍可修正。提交核对行标识与原字段值，只修改目标字段。辅助技术设置单元格值时使用同一写回路径。运行 `bazel run //examples/table:table -- --editable` 体验。

`StandardTableView` 可设置 `auto-sort: true;`，由控件按排序列的 `field` 对视图排序，排序回调仍会收到通知。多个视图可共享模型并使用不同顺序。排序保留记录标识、行高和多选，显式排序与清除排序时会按记录标识重定位当前行。排序字段更新时重新计算顺序，同值项保持源顺序，`current-row` 此时按新的显示顺序解释。清除排序后恢复源顺序，手动移动行可在此时使用。运行 `bazel run //examples/table:table -- --sorted-views` 体验。

`StandardTableView` 可用 `filter-field: "name"; filter-text: root.query;` 按字符串字段筛选。字段支持嵌套路径，匹配会忽略大小写和查询首尾空白。筛选和自动排序可同时使用，编辑仍按记录标识写回共享模型。`current-row` 按当前显示位置解释，显式选区和手动行高按记录标识保留，复制和粘贴使用显示行。筛选期间暂停拖动行，清空查询后恢复基础顺序。若连续编辑的提交使记录退出筛选，Tab 从它原先的显示位置继续查找。运行 `bazel run //examples/table:table -- --editable`，用搜索框筛选五万条库存记录。

`TableView` 和 `StandardTableView` 可用 `row-visible-field: "flags.shown";` 指定控制可见性的布尔字段。字段路径须为常量，值为 `false` 的记录退出视图，源模型、记录标识、选区和手动行高均保留。有隐藏行时暂停拖动排行，已有的手动顺序随记录保留。标准表格可同时按文本筛选和排序。行定位、复制和粘贴使用当前显示行号。开启 `preserve-scroll-anchor` 时，隐藏锚点前的记录会保留锚点记录在视口中的位置。运行 `bazel run //examples/table:table -- --row-visibility`，可在共享记录的两个表格里切换首行可见性。

设置 `resizable-columns: true;` 后，可拖动表头分隔线调整列宽。可在单列设置 `resizable: false;` 锁定分隔线。手动宽度保存在当前表格实例，受声明的最小、最大宽度限制，排序和替换行模型时仍然有效。

`multi-select: true;` 启用行多选。Shift+点击或 Shift+方向键选取范围，Ctrl/Cmd+点击或 Space 增减一行，Ctrl/Cmd+Shift 增加范围。Ctrl/Cmd+A 全选，Escape 清空，Ctrl/Cmd+方向键只移动 `current-row`。排序和插入保留已选行的标识，删除会移除对应的选择，替换模型会清空选择。`selection-changed(int)` 报告用户操作后的选中数量。Rust 宿主可通过场景节点的 `table_selection` 查询当前选择，使用 `keys()` 获取行标识或 `indices(&model)` 按模型顺序取得索引。

`TableView` 和 `StandardTableView` 可通过 `current-column`、`selection-count` 和 `has-selection` 读取当前列与选区。`current-cell-changed(row, column)` 在光标行列变化时通知应用，列使用声明索引。回调可调用 `select-cell(row, column)`、`toggle-cell(row, column)` 和 `set-current-cell(row, column)` 操作单元格，或用 `select-range(first-row, first-column, last-row, last-column)` 选择矩形。行模式可用 `select-row(row)`，列模式可用 `select-column(column)`。`select-all()` 选择当前目标的全部项，`clear-selection()` 清空选区。`set-current-cell` 单独移动光标。Rust 可通过 `table_selection(id)` 和 `select_table(id, action)` 访问同一选择状态。运行 `bazel run //examples/table:table -- --selection-api` 体验。

`selection-target` 接受 `rows`、`cells` 和 `columns`，默认按行选择。单元格模式下，Shift 扩展矩形选区，方向键移动焦点并滚动到当前单元格。列模式可点击表头或表身选择整列，Alt+点击表头排序。`multi-select` 在三种目标下都可启用。`selection-changed(int)` 按当前目标报告行、单元格或列数。Rust 可读取 `current_column()`、`columns()` 或 `cell_keys(column)`。单元格选择跟随记录标识，整列选择在替换行模型后保留。

用 `bazel run //examples/table:table -- --cells` 或 `bazel run //examples/table:table -- --columns` 体验两种选择方式。

表格同时设置 `multi-select: true;` 和 `drag-select: true;` 后，可从表身用鼠标拖出选区。预览随 `selection-target` 显示行、单元格或列范围，靠近边缘时会继续滚动到离屏内容。松手后一次提交，按住 Ctrl 或 Command 可保留原选区。Escape、移出窗口、失去焦点或拖动期间改变行结构和列顺序会取消本次选择。已进入编辑状态的单元格和独立的交互控件仍处理自己的拖动。

触屏上按住单元格 500 毫秒即可开始框选，随后拖动扩展范围，抬手提交。短按仍执行普通点击，长按生效前滑动仍滚动表格。系统取消触摸时会撤掉选区预览。

提交连续选区后，可用鼠标或触屏拖动左上、右下角点来缩小或扩大选区，另一角固定。角点滚出视口时隐藏，行结构或列顺序变化后需重新框选。

表格设置 `copyable: true;` 后，可用 Ctrl 或 Command 加 C 复制选区到系统剪贴板。复制按当前显示列顺序读取模型，包括离屏的已选行和列。单元格选区输出最小矩形，选区内的空洞保留为空字段。文本中的制表符、换行和引号会转义。自定义表格默认复制直接文本和双向绑定的模型值，也可在直接单元格上用 `clipboard-text` 指定导出文本。编辑中的文本选区优先。一次复制最多 100 万个字段或 8 MiB。

`TableCell { text <=> item; }` 可编辑 `for item in root.records` 中可写 `[string]` 模型的一行。双击单元格，或在聚焦后按 Enter / F2 开始编辑。Enter 提交，Tab 提交并移动焦点，Escape 或切换到其他控件会丢弃草稿。应用在编辑期间重排模型时，仍可见的记录会按行标识保留草稿。滚出视口、删除记录或外部改值会结束编辑。文本输入、IME、剪贴板和撤销在草稿上工作，提交时才修改模型。

`TableView` 和 `StandardTableView` 可组合布尔属性 `edit-on-click`、`edit-on-selected-click`、`edit-on-double-click`、`edit-on-key` 和 `edit-on-typing`。默认开启双击和 Enter / F2。选中后单击以本次点击之前的选区为准。输入可打印文字会打开编辑器，用第一段文字替换选中的原值，并将结果保存在草稿中。

设置 `edit-on-tab: true;` 后，Tab 提交草稿并打开下一处可编辑文本单元格，Shift+Tab 向前查找。查找按当前显示行列顺序，跳过只读、隐藏或禁用的单元格，并在必要时滚动到离屏目标。若提交改变排序，起点按原记录标识重新定位。到达首尾时回到普通焦点顺序，无效草稿留在当前编辑器。连续编辑保留表格选区，可用 `bazel run //examples/table:table -- --editable` 体验。

回调可调用 `table.edit-cell(row, column)`，行号按显示顺序，列号按声明顺序。命令将焦点移到可见、可写的文本单元格，保留表格选区。定位离屏单元格时，可在同一回调先调用 `position-view-at-cell`。切换到另一个编辑目标或调用 `table.cancel-edit()` 会丢弃该表的草稿。运行 `bazel run //examples/table:records -- --edit-triggers` 体验。

结构化行可用 `export struct StockRow { name: string, units: int }` 声明，`property <[StockRow]>` 接收 `StockRow { name: "交换机", units: 12 }` 等记录字面量。在 `TableRow` 的直接子元素中，`TableCell { text <=> row.name; }` 编辑字符串，`SpinBox { value <=> row.units; }` 编辑整数，`Slider { value <=> row.load; }` 编辑浮点数，`CheckBox { checked <=> row.available; }` 或 `Switch` 编辑布尔值。提交核对行标识和被编辑字段的原值，再把改动合并进最新记录。Rust 使用生成的 `StockRow` 和 `VecModel<StockRow>`。运行 `bazel run //examples/table:records` 可体验独立字段编辑和按类型排序。

记录字段可使用其他命名记录类型，类型之间不能形成循环。表格和树编辑器可以双向绑定 `row.stock.units` 这样的字段。`TableColumn.field`、`text-role`、`value-role` 和 `row-height-field` 接受点分路径，例如 `"stock.units"`。树的 `key` 和 `parent-key` 可引用嵌套的整数。粘贴解析器返回整段子记录时，撤销历史仍按实际变更的字段保存。运行 `bazel run //examples/table:records -- --nested` 可查看同一库存模型在表格、列表、下拉框和树中的投影。

`TableRow` 与 `TreeRow` 可以在静态子路径使用匿名组件，例如 `NameEditor { value <=> row.name; submitted => { root.save(); } }`。组件只在记录进入视口时创建，滚动、排序和树的折叠保留同一记录的局部状态，删除记录或替换模型会释放对应实例。点击表格单元格内的控件时，表格先更新选区，控件继续处理本次点击。无障碍树公开行列坐标以及编辑器和按钮自己的角色。

直接作为表格单元格的组件可设置 `clipboard-text: row.name;`，从模型复制可见和离屏记录。表格启用 `pasteable` 后，自定义编辑器可用 `clipboard-target: "value";` 指定绑定到当前模型行的 `in-out` 输入，再用 `clipboard-paste(text) => { … }` 返回包含 `accepted: bool` 和同类型 `value` 的记录。每列可在静态子路径配置一个解析器。表达式可读取外层组件属性、当前记录和索引，也可调用纯回调。所有字段通过校验后才一起提交，解析覆盖离屏行。未配置解析器的自定义单元格会使整次粘贴取消。

运行 `bazel run //examples/table:records -- --delegates`，在表格焦点下可粘贴名称、数量和可用状态。数量解析示例同时接受 `12` 与 `12件`。给一行加星后按表头排序，或编辑名称并按 Enter 提交，可体验自定义编辑器的交互。

`TableView` 可用 `pasteable: true;` 开启 Ctrl/Cmd+V 批量粘贴。选中单元格后，从该格沿显示列顺序写入 TSV，行选区从当前行的第一显示列开始。数据通过单元格的双向绑定写入，支持离屏行和包含制表符、换行、双引号的文本字段。数字使用十进制文本，布尔值接受 `true`、`false`、`1` 或 `0`。整块数据校验成功后才写入模型，保留行标识。只读、禁用、隐藏、类型不匹配、越界和过期剪贴板回执会取消整次粘贴。正在编辑文字时，快捷键交给文本编辑器。运行 `bazel run //examples/table:records -- --cells`，选中名称或负责人单元格后可粘贴多行数据。

`StandardTableView` 也可用 `pasteable: true;` 将 TSV 批量写入声明为 `editable` 的列。它沿当前显示顺序定位行列，使用单元格编辑器的类型校验，包括布尔值、日期和时间。排序后的离屏行可参与同一次提交，任一目标只读或字段值无效时整块拒绝。剪贴板返回时会再次核对列顺序、模型版本和写入权限。运行 `bazel run //examples/table:table -- --editable`，选中数量列后可粘贴 `数量、可用、刷新时间` 三列，再用 Ctrl/Cmd+Z 撤销。

表格可用 `undoable: true;` 开启编辑历史，`pasteable: true;` 也会启用。内置编辑器和自定义单元格组件通过模型绑定进行的每次有效写回各记一笔，整块粘贴也记一笔。滑块持续写入时，每次生效的值变化各记一笔。表格聚焦时用 Ctrl/Cmd+Z 撤销，用 Ctrl/Cmd+Shift+Z 或 Ctrl+Y 重做。具有草稿的单元格编辑器打开时，快捷键作用于当前草稿。

历史按行标识和实际变更字段保存，重排、滚动和列移动后仍可重放。每次重放先核对全部目标行和字段值。若目标行已删除或目标字段已改写，整笔历史失效，其余字段保留后续编辑。每个表格实例最多保留 64 笔、约 8 MiB 的历史数据。超出单笔容量的修改仍会提交，并清空旧历史。关闭历史后提交修改也会清空旧历史。运行 `bazel run //examples/table:table -- --editable`，依次提交数量编辑、粘贴多行，再按快捷键逐笔撤销。

工具栏和菜单可绑定表格的只读布尔属性 `can-undo` 和 `can-redo`，并在回调中调用 `table.undo()` 或 `table.redo()`。两个状态分别跟踪当前模型的待回放记录，执行时才核对字段值。冲突的记录会被丢弃，并更新按钮状态。`--editable` 示例也提供了绑定这两个属性的撤销与重做按钮。

`background` 和 `border-color` 使用 `brush` 类型，纯色可直接绑定。线性渐变使用 `@linear-gradient(90deg, #536dfe 0%, root.accent 100%)`，径向渐变使用 `@radial-gradient(circle, #fff, #536dfe)`，中心和半径由元素尺寸决定。色标颜色可以引用属性，省略的位置会均匀插值，显式位置须在 0% 到 100% 之间且按升序排列。`Rectangle` 和 `Button` 可以用 `border-top-left-radius` 等属性分别覆盖四角的 `border-radius`。

视觉节点可以用 `z` 控制同一父级下的叠放顺序，默认是 0。同值按声明顺序绘制，指针从最上层开始命中。Tab 遍历、无障碍阅读顺序和组件事件路径保留各自的结构顺序。模态和弹出层由窗口管理打开顺序。

`opacity` 把节点和子节点作为一组淡出，取值限制在 0 到 1。透明度为 0 时跳过绘制，控件仍参与布局并可接收输入。纯透明度更新可复用已准备的布局。运行 `bazel run //examples/layout:layout -- --compositing` 查看重叠淡出与点击顺序。

`Rectangle` 支持 `drop-shadow-*` 与 `inner-shadow-*`，两组都提供 `color`、`offset-x`、`offset-y`、`blur` 和 `spread`。外阴影由背景和边框的透明度生成，沿父容器边界裁剪。内阴影沿矩形的圆角裁切。正扩张增大外阴影并加宽内阴影，负值收缩阴影。外阴影扩张按设备像素取整，范围为 ±192 个设备像素。两种阴影都随节点的 `opacity` 合成，可通过绑定、状态或动画更新。运行 `bazel run //examples/layout:layout -- --shadows`。

`Rectangle { clip: true; }` 按边框内侧的圆角裁剪子树。显式设置 `clip: false;` 允许子控件溢出矩形，仍受更外层的裁剪和窗口范围约束。省略 `clip` 时沿用原有矩形裁剪。绘制和指针命中共用圆角几何，无障碍按其矩形投影报告可见边界。运行 `bazel run //examples/layout:layout -- --clipping`。

普通视觉节点支持 `transform-rotation`、`transform-scale`、`transform-scale-x` 和 `transform-scale-y`。旋转值用顺时针角度，逐轴值可覆盖统一缩放，负值可镜像。`transform-origin-x` 和 `transform-origin-y` 接受长度或百分比，省略时以节点中心为原点。变换作用于已完成布局的子树，绘制、指针定位和裁剪共用变换矩阵。窗口级弹出层继续以窗口坐标定位。运行 `bazel run //examples/layout:layout -- --transforms`。

`ScaleRotateGestureHandler` 把触屏双指手势和 macOS 触控板手势统一成 `started()`、`updated()`、`ended()` 和 `cancelled()`。在 `started()` 保存初始状态，在 `updated()` 读取累计 `scale`、顺时针角度 `rotation` 和局部逻辑像素中心 `center-x/y`，再更新内容变换。手势结束或取消时，`active` 变回 `false`，读数回到默认值。两指须从同一个可用区域开始。第二指接管待提交的单指输入时，会取消原来的点击候选。关闭 `enabled` 会取消该区域的手势，子控件仍可接收单指操作。运行 `bazel run //examples/layout:layout -- --scale-rotate`。

`SwipeGestureHandler` 用 `handle-swipe-left/right/up/down` 选择接收的方向，默认全部关闭。鼠标左键或单指触屏移动达到 8 逻辑像素后，最内层接受主方向的区域接管输入。其他方向可继续由滚动视图处理。`moved()` 中可读 `swiping` 与 `pressed-x/y`、`current-x/y`。坐标以按下时的局部坐标系计算。松手触发 `swiped()`，此时可以读取最终坐标。滑动期间调用 `cancel()`、禁用区域或失去窗口焦点会触发 `cancelled()`。轻触仍可点击内部按钮。运行 `bazel run //examples/layout:layout -- --swipe-gesture`，横拖移动卡片，竖拖滚动画布。

`WheelHandler` 的 `wheel(event)` 接收类型化的 `WheelEvent`。其中 `x/y` 是处理器内的局部坐标，`delta-x/y` 是逻辑像素增量，向右和向下为正。行滚轮输入还提供 `is-line` 和 `lines-x/y`，`modifiers` 可读取修饰键。设置 `orientation` 可只接收含指定轴增量的事件，省略时接收双轴。处理器沿命中路径由内向外执行，返回 `accept` 即消费，返回 `reject` 则继续交给外层或默认滚动。裁剪与模态区域约束照常生效。运行 `bazel run //examples/layout:layout -- --wheel`，按住 Ctrl 滚动可围绕光标缩放画布。

`TouchArea` 的 `pointer-event(event)` 接收 `down`、`move`、`up` 和 `cancel`。事件包含按钮、局部逻辑像素坐标 `x/y`、命中状态 `inside`、触摸标记 `is-touch` 和 `modifiers`。鼠标移出控件后仍由起始控件接收事件。滚动等手势接管时，起始控件收到 `cancel`。有效点击先收到 `up`，再执行 `clicked()`。运行 `bazel run //examples/layout:layout -- --pointer` 可试用。

触摸事件的 `event.touch-finger-id` 是窗口分配的正整数，同一触点从按下到释放或取消都使用同一个 ID。鼠标为 `0`。设置 `multi-touch: true` 后，区域可以同时捕获多个触点，`active-touch-count` 可直接绑定。`pressed` 表示还有指针按住，`contains-press` 表示至少一个活动指针仍在区域内。每个窗口仍由一个触点驱动常规点击、长按和滚动。附加触点发送各自的指针事件，同一区域并发触摸会取消单指点击和长按候选。双指缩放接管时只取消它所用的触点。`--pointer` 示例显示当前触点数和最近事件的 ID，并在两个触点同时按住时改变底色。

`area.touch-points` 是只读的 `[TouchPoint]` 模型，按按下顺序列出仍活动的触点。每行提供 `id`、局部坐标 `x/y`、按下时的坐标 `start-x/start-y` 和 `inside`。移动更新对应行，释放或取消移除该行，其余触点保留行身份。可用 `for point in area.touch-points : Rectangle { x: point.x; y: point.y; }` 绘制跟随标记。重复元素声明 `x` 或 `y` 时会按这些坐标放置。`--pointer` 示例为每根手指绘制一个标记。

`TouchArea` 的只读属性可直接用于绑定。`pressed` 表示指针仍按住，`contains-press` 表示按住且当前位置在区域内，`has-hover` 表示鼠标命中该区域。`mouse-x/y` 保留最近一次局部坐标，`pressed-x/y` 保留按下时的坐标。状态先于回调更新，按住移动时会触发 `moved()`。同一实例的两次鼠标主键点击或同一触摸设备的两次轻触，在 450 毫秒内且释放位置相距不超过 6 个逻辑像素时，第二次 `clicked()` 后触发 `double-clicked()`。

`TouchArea.scroll-event(event)` 使用与 `WheelHandler.wheel(event)` 相同的 `WheelEvent`，包含局部坐标、像素和行增量，以及修饰键。返回 `accept` 会停止传递。返回 `reject` 会继续交给外层处理器，若沿途都拒绝，则执行默认滚动。在 `--pointer` 示例中，Ctrl 加滚轮切换底色，Shift 加滚轮由外层接收，普通滚轮滚动页面。

设置 `TouchArea.enabled: false` 会让点击、触摸、悬停和滚轮穿透到下层控件。区域内的子控件仍可交互。若禁用时指针仍按住，原区域收到一次 `cancel`，下层控件从下一次按下开始接收点击。运行 `bazel run //examples/layout:layout -- --pointer-pass-through`，切换上层触控区后，可在同一位置验证穿透。

`TouchArea.accepted-buttons` 使用类型化的按钮集合，例如 `[primary, secondary]`。省略时接收主键、右键和中键，`[]` 让这些按键都穿透。单指触摸按主键过滤，`clicked()` 仍由主键或轻触触发。右键和中键可通过 `pointer-event(event)` 的 `button` 读取。过滤与悬停、光标样式和滚轮独立。若绑定变化排除了正在按住的按钮，当前序列收到 `cancel`。在 `--pointer-pass-through` 示例中勾选「上层仅接收右键」，同一区域的主键点击会落到底层按钮。

`TouchArea.press-and-hold()` 支持鼠标主键和单指触摸。每次按下时读取 `press-and-hold-interval`，默认 800 毫秒，非正值关闭长按。移出区域、移动超过 6 个逻辑像素或被滚动接管会取消尚未触发的长按。长按每次按下只触发一次，可在回调中读取 `pressed-x/y`。松手仍会发送 `pointer-event` 的 `up`，本次长按会消费对应的点击。运行 `bazel run //examples/layout:layout -- --pointer`，按住画布可看到底色变化。

`HoverHandler` 包住要观察的区域，子控件照常接收点击。可绑定只读 `hovered` 和局部逻辑像素坐标 `hover-x/y`，离开后坐标归零。`entered()`、`moved()` 和 `exited()` 在同一帧的悬停状态发布后执行。命中路径上的嵌套处理器可同时观察，设置 `blocking: true;` 会停止向外层传递。布局移动、禁用、裁剪和模态弹窗会重新计算悬停，即使光标静止。运行 `bazel run //examples/layout:layout -- --hover` 体验悬停样式和子按钮点击。

`HoverHandler` 和 `TouchArea` 可绑定 `cursor-shape`，类型为 `CursorShape`。可用值包括 `pointer`、`crosshair`、`text`、`grab` 和 `zoom-in`。默认 `auto` 会沿当前命中路径查找显式形状，再使用控件默认光标。`default` 则明确使用系统箭头。实际命中的 `TouchArea` 优先于悬停处理器，嵌套悬停优先用内层的形状。拖放和分栏、表格、窗口调整大小仍使用原生操作光标，编辑区默认使用 I 形光标。`--hover` 示例中的外层区域使用十字，内层按钮区域使用手形。
