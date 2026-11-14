# .xr 语言参考

[返回项目首页](../README.md)

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

## 属性绑定与回调

属性 getter 首次读取时求值并缓存结果。运行时记录绑定本次实际读取的属性，依赖变化时沿组件关系标记失效，等下一次读取才重算。条件切换会替换订阅，未执行分支中的属性变化不会使缓存失效。

setter 和 DSL 赋值覆盖绑定并解除订阅，`reset_*()` 恢复默认绑定。已知值相同的写入不会使下游失效。`in` 和 `in-out` 属性提供公开 setter，`out` 提供公开 getter，`private` 仅供组件内部访问。

直接写在元素上的普通计算绑定也使用缓存，例如 `Text { text: "计数：" + root.count; }`。纯回调被 Rust 重新注册时，依赖它的绑定会失效。生成组件通过 `Component::render_shared()` 复用干净的 `Node` 快照。

`for` 按位置保存每行的场景和绑定缓存。替换字符串列表时会比较被使用的行，值相同的位置复用原有绑定。嵌套 `for` 各自跟踪依赖，`if` 隐藏的列表会等到再次显示才求值。普通重复项最多生成 4096 行。

Rust 可用 `model_<property>()` 取得列表属性的共享模型，用 `bind_model_<property>(model)` 接入已有的 `VecModel<T>`。`set_row`、`push`、`insert` 和 `remove` 分别通知行数据和结构变化。`get_<property>()`、`set_<property>(Vec<T>)` 和 Rust 回调使用 `Vec<T>` 快照。模型属于 UI 线程，后台任务可以通过组件弱句柄的 `upgrade_in_event_loop` 在 UI 线程更新指定行。winit 会复用没有受变化影响的视口场景与布局。

```rust
let model = xrack_runtime::VecModel::from(vec!["首行".into(), "第二行".into()]);
ui.bind_model_results(model.clone());
model.set_row(1, "第二行已更新".into());
```

回调可用 `callback score(value: int) -> int;` 声明结果类型，处理器的末尾表达式提供返回值。`.xr` 中可用 `root.score(4)` 或 `child.score(4)` 调用组件回调。属性绑定只能调用标有 `pure` 的回调，其处理器也不能写入属性或调用有副作用的回调。编译器会检查参数和返回类型，并拒绝递归调用以及经回调形成的属性依赖循环。

DSL 调用与 Rust 的 `invoke_*()` 共用同一处理逻辑。注册了 Rust `on_*()` 闭包时，有返回类型的调用返回闭包的结果。纯回调接收 `Fn` 闭包，调用方需保证闭包没有副作用。元素事件通过 `dispatch(id, event)` 分发。

`callback submitted <=> editor.accepted;` 可以把内部控件事件或子组件的回调作为组件 API 公开。别名沿用目标的参数、返回类型和纯度，调用别名与调用目标进入同一个回调。也可以在命名元素上声明自定义回调。组件实例可写 `entry := Editor { submitted => { root.save(); } }`，类型化的处理器可以返回结果。列表和网格的实例处理器可读取当前模型项和索引。

实例处理器和 Rust `on_*()` 设置目标回调的实现。声明上已有的普通 DSL 回调语句仍先执行，返回值以当前注册实现为准。别名上声明的处理器可作为默认实现。纯回调的注册变化会让依赖该回调的绑定失效。运行 `bazel run //examples/stepper:stepper -- --linked`，在共享编辑器或任务行按 Enter 体验转发。

## 容器组件与共享状态

组件体内可以用 `@children` 指定调用方子元素的插入位置。省略标记时，调用方子元素追加到组件根部。每个组件至多有一个标记，标记必须在静态的元素层级中。插入的元素按实际容器参与布局，属性引用和回调仍使用调用方的作用域。

```xr
component Panel inherits VerticalLayout {
    Label { text: "Settings"; }
    VerticalLayout {
        spacing: 8px;
        @children
    }
}

export global Session {
    in-out property <string> user-name: "Guest";
}

export component App inherits Window {
    Panel { Label { text: Session.user-name; } }
}
```

`global` 可声明属性、回调及其处理器。一个顶层组件实例及其子组件共享一份全局状态，每次独立创建顶层组件都会获得新状态。跨文件用普通 `import` 导入已导出的全局。Rust AOT 可用 `ui.global_Session()` 取得导出对象。解释器可用 `instance.global("Session")`，再调用属性和回调 API。全局不出现在窗口或预览组件列表中。

## 内建函数与值

普通内建函数通过 `Math`、`Strings`、`Model`、`Colors`、`Dates`、`I18n` 和 `Log` 命名空间调用。`@` 用于 `@tr`、`@image-url` 等专用语法。作用域中若已有同名控件 ID，限定调用会解析为该控件的成员。

- **Math** 的 `min`、`max`、`clamp`、`abs` 和 `mod` 保留数值单位，整数和浮点数可以混用。`clamp` 会交换颠倒的边界。`mod` 计算欧几里得余数，除数为零时整数结果为 0，其他数值结果为 NaN。`round`、`floor` 和 `ceil` 返回整数，超出整数范围时饱和。`round` 在中点向远离零的方向舍入。
- **Math** 还提供 `sign`、`sqrt`、`pow`、`exp`、`ln`、`log`、`hypot`、`is-finite`，以及 `pi`、`tau`、`e`。三角函数 `sin`、`cos`、`tan` 和反三角函数 `asin`、`acos`、`atan`、`atan2` 用弧度表示角度。用 `Math.to-radians(90)` 和 `Math.to-degrees(angle)` 换算。`Math.atan2(y, x)` 按坐标返回方向角。
- **Strings** 提供 `trim`、`is-empty`、`contains`、`starts-with`、`ends-with`、`replace-all`、`lowercase`、`uppercase` 和 `character-count`。字符计数按 Unicode 字素簇计算，例如 `"👩‍💻"` 算一个字符。`is-int`、`is-float` 检查修剪过的数字字符串。`Strings.to-float(input, 0.0)` 和 `Strings.to-int(input, 0)` 在解析失败时返回默认值。小数分隔符使用 `.`。
- **Strings.format** 的模板是字符串字面量。`{}` 按顺序消费字符串、整数、浮点数、布尔值、日期或时间参数。`{{` 和 `}}` 表示字面量大括号。`Strings.format("{:.2}", value)` 输出两位小数，精度可设为 0 到 20。
- **Model** 的 `count` 和 `is-empty` 跟踪字符串或记录模型的行数。修改已有行的内容会保留行数绑定的缓存。`Model.at(root.rows, index, fallback)` 按从零开始的索引取行，只有索引越界时才求值默认表达式。默认值类型必须和模型行一致。
- **Colors** 提供 `rgb`、`rgba`、`with-alpha` 和 `mix`，例如 `Colors.rgba(30, 120, 200, 75%)`。RGB 通道限制在 0 到 255，透明度与混合系数限制在 0 到 1。`mix` 对 RGBA 四个通道做线性插值。
- **I18n.locale()** 返回协商后的语言标识，例如 `"fr"`。调用会跟踪 UI 语言变化。有 Fluent 语言包时使用匹配语言或语言包的默认语言。没有语言包时返回应用设置的语言，尚未设置时返回 `"und"`。词条翻译使用 `@tr`。

### 公历日期运算

- `Dates.is-valid(date)` 检查公历日期是否存在。年份范围是 1–9999，1900 年没有 2 月 29 日，2000 年有。
- `Dates.add-days(date, days, fallback)` 加减自然日。偏移量可以是负数，跨月、跨年和闰日按公历计算。
- `Dates.add-months(date, months, fallback)` 加减整月并尽量保留原来的日。目标月没有对应的日时取目标月的最后一天，例如 2024 年 1 月 31 日加一个月得到 2 月 29 日。
- `Dates.weekday(date)` 返回周一为 1 到周日为 7 的编号。无效日期返回 0。

加减遇到无效输入或结果超出年份范围时才会求值并返回 `fallback`。

```xr
in property <Date> selected: Date { year: 2024, month: 1, day: 31 };
out property <Date> next-month: Dates.add-months(root.selected, 1, root.selected);
out property <int> weekday: Dates.weekday(root.next-month);
```

### 本地化数字

`I18n.format-number(value, decimals)` 将整数或浮点数格式化为当前语言的数字，按地区规则使用数字字形、小数分隔符和分组符。小数位数限制在 0 到 20，整数在格式化前保留完整精度。

`I18n.parse-number(text, fallback)` 返回浮点数，接受当前地区的小数和分组符号。分组位置错误、非有限数值和其他无效输入返回 `fallback`。例如法语界面输入 `"1,5"` 会得到 `1.5`。

```xr
in-out property <string> amount-input;
out property <float> amount: I18n.parse-number(root.amount-input, 0.0);
out property <string> amount-display: I18n.format-number(root.amount, 2);
```

两个函数都会随 UI 语言切换刷新。数字使用应用请求的地区规则，例如 `fr-CA`，即使翻译词条回退到 `fr`。未设置语言时使用 Fluent 目录的默认语言，没有目录时使用 ICU 根语言环境。面向机器的文本可继续用固定语法的 `Strings.format` 和 `Strings.to-float`。

## 调试 UI 逻辑

在事件处理器和有副作用的回调中，可以用 `Log.write(LogLevel.info, message)` 指定等级，也可以直接调用 `Log.trace(message)`、`Log.debug(message)`、`Log.info(message)`、`Log.warn(message)` 和 `Log.error(message)`。消息是字符串表达式。等级是 `LogLevel` 枚举值，可来自属性或回调参数。

```text
clicked => {
    Log.debug(Strings.format("before: {}", root.count));
    root.count += 1;
    Log.write(LogLevel.info, Strings.format("after: {}", root.count));
}
```

生成的 Rust 和解释器都向 `xrack::ui` 发出带组件名、文件和行列号的 `tracing` 事件。VS Code 实时预览把所有等级写到 **XRack Preview** 输出面板。独立 Rust 应用通过自己的 `tracing` subscriber 接收和过滤这些事件。

## 界面翻译

在入口 `.xr` 文件中用 `translations` 声明 Fluent 语言包清单，用 `@tr` 绑定整句文案。入口文件及其导入的组件共享该清单。参数可传字符串、整数和浮点数，Fluent 按当前语言处理占位符和复数。

```text
translations "i18n/catalog.toml";

export component Counter inherits Window {
    in-out property <int> count: 0;
    Text { text: @tr("counter-count", count: root.count); }
}
```

清单的 `default` 指定默认语言，`[locales]` 把语言标识映射到相对清单的 `.ftl` 路径。例如 `en-US = "en-US.ftl"` 和 `zh-CN = "zh-CN.ftl"`。中文语言包可以写 `counter-count = 计数：{ $count }`。编译器检查默认语言的词条和各译文引用的参数。其他语言缺少词条时，按条回退到默认语言。

Bazel 的 `xr_ui_library` 用 `assets` 声明清单和全部语言文件。Cargo 使用 `xrackc --cargo` 时会跟踪这些输入，语言包随生成代码嵌入应用。桌面宿主用 `Desktop::set_locale("zh-CN")` 更新已打开的窗口和托盘。无窗口渲染也可以通过生成组件的 `new_with_locale(context)` 共享 `LocaleContext`，随后调用 `context.set_locale("zh-CN")`。

运行 `bazel run //examples/tray:tray`，打开托盘菜单中的计数窗口，再用 **Language** 子菜单切换中英文。窗口标题、计数文案、托盘菜单和提示文字会更新，已有计数会保留。中文包有意省略窗口提示语，便于看到词条回退。
