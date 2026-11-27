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
        text <- "计数：" + root.count;
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

同一文件内可以实例化用户定义的组件，例如 `left := Stepper { step <- root.step; }`。实例内的绑定可设置 `in` 和 `in-out` 输入，父组件可以读取非私有属性。组件内部可通过事件写自己的 `out`、`in-out` 和私有属性。每个实例有独立的属性状态和事件路径，嵌套实例的按钮可通过 `dispatch("left/add", "clicked")` 调用。双计数器示例演示了父组件读数、惰性输入绑定与实例隔离。

内建元素包括 `Window`、`Rectangle`、`Text`、`Button`、`TouchArea`、`TextInput`、`LineEdit`、`TextEdit`（也可写 `TextArea`）、`CheckBox`、`Switch`、`Slider`、`SpinBox`、`ComboBox`、`RadioGroup`、`TabWidget`、`ListView`、`Dialog`、`DialogButton`、`ProgressIndicator`、`ScrollView`、`VerticalLayout`、`HorizontalLayout`、`VerticalBox`、`HorizontalBox`、`GridLayout`、`Image` 和 `Path`。声明事件处理器的子元素需要显式 ID。

## 属性绑定与回调

属性 getter 首次读取时求值并缓存结果。运行时记录绑定本次实际读取的属性，依赖变化时沿组件关系标记失效，等下一次读取才重算。条件切换会替换订阅，未执行分支中的属性变化不会使缓存失效。

属性赋值只改变可写存储里的值。已知值相同的写入不会使下游失效。可写的 `in` 和 `in-out` 属性提供公开 Rust setter，`out` 提供公开 getter，`private` 仅供组件内部访问。

结构体更新表达式用 `..` 从同类型的结构体取出未列出的字段。它先按书写顺序计算显式字段，再求值一次 `..` 后的表达式，得到一个新的结构体值。可以用它在回调中修改几个字段，再把整个值写回属性。

```xr
rename(new-name) => {
    root.user = User { name: new-name, ..root.user };
}
```

这里的赋值遵循整属性写入规则。`user` 须声明为可写存储。直接给 `root.user.name` 赋值仍不支持。

直接写在元素上的普通计算绑定也使用缓存，例如 `Text { text <- "计数：" + root.count; }`。纯回调被 Rust 重新注册时，依赖它的绑定会失效。生成组件通过 `Component::render_shared()` 复用干净的 `Node` 快照。

`for` 按位置保存每行的场景和绑定缓存。替换字符串列表时会比较被使用的行，值相同的位置复用原有绑定。嵌套 `for` 各自跟踪依赖，`if` 隐藏的列表会等到再次显示才求值。普通重复项最多生成 4096 行。

Rust 可用 `model_<property>()` 取得列表属性的共享模型，用 `bind_model_<property>(model)` 接入已有的 `VecModel<T>`。`set_row`、`push`、`insert` 和 `remove` 分别通知行数据和结构变化。`get_<property>()`、`set_<property>(Vec<T>)` 和 Rust 回调使用 `Vec<T>` 快照。模型属于 UI 线程，后台任务可以通过组件弱句柄的 `upgrade_in_event_loop` 在 UI 线程更新指定行。winit 会复用没有受变化影响的视口场景与布局。

```rust
let model = xrack_runtime::VecModel::from(vec!["首行".into(), "第二行".into()]);
ui.bind_model_results(model.clone());
model.set_row(1, "第二行已更新".into());
```

回调可用 `callback score(value: int) -> int;` 声明结果类型，处理器的末尾表达式提供返回值。`.xr` 中可用 `root.score(4)` 或 `child.score(4)` 调用组件回调。属性绑定只能调用标有 `pure` 的回调，其处理器也不能写入属性或调用有副作用的回调。编译器会检查参数和返回类型，并拒绝递归调用以及经回调形成的属性依赖循环。

回调体可以用 `if`、`else if` 和 `else` 执行语句。条件必须是 `bool`，每个条件最多求值一次，只有选中的分支会执行。条件取值仍可用 `条件 ? 真值 : 假值`，有返回类型的回调仍以末尾表达式返回结果。

```xr
clicked => {
    if root.can-save {
        root.status = "保存中";
        root.save();
    } else {
        root.status = "请先填写必填项";
    }
}

refresh => {
    for entry[index] in root.entries {
        root.refresh-row(index, entry);
    }
}
```

`for entry in model { ... }` 遍历进入循环时取得的模型快照。循环体内的模型增删会立即更新模型，当前循环会遍历原来取得的条目。`[index]` 可省略，索引从零开始。循环源也可以是整数计数，例如 `for step in 3 { ... }` 依次绑定 `0`、`1`、`2`，非正数执行零次。同步循环会遍历完整的计数区间或模型快照。循环变量在循环体内可读，语句可嵌套，`pure` 回调的限制适用于每个分支和循环体。

DSL 调用与 Rust 的 `invoke_*()` 共用同一处理逻辑。注册了 Rust `on_*()` 闭包时，有返回类型的调用返回闭包的结果。纯回调接收 `Fn` 闭包，调用方需保证闭包没有副作用。元素事件通过 `dispatch(id, event)` 分发。

`callback submitted <-> editor.accepted;` 可以把内部控件事件或子组件的回调作为组件 API 公开。别名沿用目标的参数、返回类型和纯度，调用别名与调用目标进入同一个回调。也可以在命名元素上声明自定义回调。组件实例可写 `entry := Editor { submitted => { root.save(); } }`，类型化的处理器可以返回结果。列表和网格的实例处理器可读取当前模型项和索引。

实例处理器和 Rust `on_*()` 设置目标回调的实现。声明上已有的普通 DSL 回调语句仍先执行，返回值以当前注册实现为准。别名上声明的处理器可作为默认实现。纯回调的注册变化会让依赖该回调的绑定失效。运行 `bazel run //examples/stepper:stepper -- --linked`，在共享编辑器或任务行按 Enter 体验转发。

### 属性初始化与绑定

| 写法 | 行为 |
| --- | --- |
| `property <T> name: expression;` | 在构造时求值一次，把结果存入属性。可写权限由 `in`、`out` 等可见性限定。 |
| `property <T> name <- expression;` | 建立持续绑定。读取时求值，依赖变化后重新求值。公式属性只读。 |
| `property <T> name <-> target;` | 建立可写别名，沿连接读写目标。目标需要是可写存储。 |
| `property <T> name;` | 分配存储，初值为该类型的默认值。 |

实例体内也使用同一套含义。`Text { text: root.title; }` 在初始化时取标题，`Text { text <- root.title; }` 持续跟随。重复项的视觉初值在各行首次创建时分别取值。结构体字段、回调参数类型和状态声明各自使用冒号作为分隔符。

例如，编辑器同时保留上游文档名和自己的草稿。

```xr
enum TitleMode { Follow, Custom(string) }

component DocumentEditor inherits Rectangle {
    in property <string> document-title;
    out property <string> live-title <- root.document-title;
    private property <string> draft: root.document-title;

    private property <TitleMode> title-mode: TitleMode::Follow;
    out property <string> display-title <- match root.title-mode {
        Follow => root.document-title,
        Custom(text) => text,
    };

    TextInput { text <-> root.draft; }

    callback rename(string);
    rename(text) => { root.title-mode = TitleMode::Custom(text); }
    callback restore-default();
    restore-default => { root.title-mode = TitleMode::Follow; }
}

export component App inherits Window {
    in-out property <string> title-source: "A";
    DocumentEditor { document-title <- root.title-source; }
}
```

父组件把 `"A"` 接入编辑器。Rust 调用 `app.set_title_source("B".into())` 后，`live-title` 变成 `"B"`，草稿保留 `"A"`。如果用户显式使用 `rename("A")` 接管标题，`display-title` 就保留 `"A"`，直到调用 `restore-default()` 后重新显示最新的上游名称。公式属性没有 Rust setter，一次性初始化属性没有重新执行初始化表达式的 reset 方法。

实例上的 `document-title <- root.title-source` 由提供输入的组件拥有。即使接收端声明为 `in-out`，从接收端写入也无法替换这条绑定。双方需要共享可写存储时，使用 `<->`。给实例接上单向绑定后，经由同一存储的其他别名写入也会报错。如果显式调用的 DSL 回调会写入这个输入，编译器会指出调用、写入和绑定的位置。条件未知时，可能执行的写入也会导致编译错误。明确不可达的写入分支（例如 `if false`）不影响回调能否调用。

`in-out` 属性需有可写存储或可写别名。用 `out property <T> value <- expression;` 公开计算结果，用 `in-out property <T> value: expression;` 声明可写的初值。

`TextEdit { text <- root.title-source; }` 持续显示上游值，`edited(value)` 仍会报告输入，应用可以在事件处理器里更新上游。需要独立草稿时，可以把文本双向绑定到一次性初始化的属性。类似地，`ScrollView` 的单向 `content-y <- expression;` 保留计算绑定，`content-y <->` 会把滚动偏移写回可写属性。

## 局部变量、纯函数和枚举

回调和纯函数中可用 `let name = expression;` 声明不可写的局部变量。初始化表达式执行一次，后续语句和末尾表达式均可读取它。内层块可以声明同名变量，离开该块后外层变量恢复可见。`pure function` 在同一处声明返回类型和实现，调用方式与纯度检查沿用 `pure callback`。

```xr
export enum RequestState {
    Idle,
    Ready(string),
    Failed { message: string, retryable: bool },
}

component StatusLine inherits VerticalLayout {
    in property <RequestState> state: RequestState::Idle;
    pure function status-text(value: RequestState) -> string {
        let caption = match value {
            Idle => "等待",
            Ready(document) => document,
            Failed { message: reason, retryable: _ } => reason,
        };
        "状态 " + caption
    }

    Label { text <- root.status-text(root.state); }
    match root.state: {
        Idle => Label { text <- "可以启动"; },
        Ready(document) => Label { text <- document; },
        Failed { message, retryable: _ } => Label { text <- message; },
    }
}
```

有名枚举可导入，可作为属性、回调参数或记录字段。默认值是第一项，载荷字段使用各自的默认值。用 `RequestState::Idle`、`RequestState::Ready("结果")` 或 `RequestState::Failed { message: "失败", retryable: true }` 构造值。AOT 为宿主生成原生 Rust `enum`，属性和回调直接使用生成的类型。

两种 `match` 都要求列出全部变体且每项只出现一次。按声明的形状解构每个载荷字段，可用 `_` 忽略字段，也可给命名字段起局部别名。绑定仅在该分支中可见，且为只读。表达式形式只求值选中的结果，树内形式显示选中分支的元素。

动态宿主通过同名 record 传递枚举，`discriminant` 是从零开始的变体序号。只传入所选变体的载荷字段，位置字段使用 `_0`、`_1` 等键。解释器会拒绝无效序号、缺失或多余字段以及类型错误的值。

## 容器组件与共享状态

组件体内可以用 `@children` 指定调用方子元素的插入位置。省略标记时，调用方子元素追加到组件根部。每个组件至多有一个标记，标记必须在静态的元素层级中。插入的元素按实际容器参与布局，属性引用和回调仍使用调用方的作用域。

```xr
component Panel inherits VerticalLayout {
    Label { text <- "Settings"; }
    VerticalLayout {
        spacing <- 8px;
        @children
    }
}

export global Session {
    in-out property <string> user-name: "Guest";
}

export component App inherits Window {
    Panel { Label { text <- Session.user-name; } }
}
```

`global` 可声明属性、回调及其处理器。一个顶层组件实例及其子组件共享一份全局状态，每次独立创建顶层组件都会获得新状态。跨文件用普通 `import` 导入已导出的全局。Rust AOT 可用 `ui.global_Session()` 取得导出对象。解释器可用 `instance.global("Session")`，再调用属性和回调 API。全局不出现在窗口或预览组件列表中。

用 `@slot(toolbar)` 声明命名插槽，在组件实例中用 `slot toolbar { ... }` 提供内容。命名插槽可与 `@children` 同时使用。每个命名插槽可填充一次，省略时它为空。重复模板可以将周围的行和索引作为参数传给插槽。

```xr
component Results inherits VerticalLayout {
    in property <[string]> rows;
    @slot(toolbar);
    for row[index] in root.rows : VerticalLayout {
        @slot(row, row, index);
    }
}

export component ResultsDemo inherits Window {
    Results {
        rows <- ["Alpha", "Beta"];
        slot toolbar { Text { text <- "结果"; } }
        slot row(item, position) {
            Text { text <- position + " " + item; }
        }
    }
}
```

填充内容沿用调用方的 `root`、元素 ID 和回调作用域。插槽参数根据声明处的重复模型推断类型，参数个数须与标记匹配。

## 内建函数与值

普通内建函数通过 `Math`、`Strings`、`Model`、`Colors`、`Dates`、`Times`、`I18n` 和 `Log` 命名空间调用。`@` 用于 `@tr`、`@image-url` 等专用语法。作用域中若已有同名控件 ID，限定调用会解析为该控件的成员。

- **Math** 的 `min`、`max`、`clamp`、`abs` 和 `mod` 保留数值单位，整数和浮点数可以混用。`clamp` 会交换颠倒的边界。`mod` 计算欧几里得余数，除数为零时整数结果为 0，其他数值结果为 NaN。`round`、`floor` 和 `ceil` 返回整数，超出整数范围时饱和。`round` 在中点向远离零的方向舍入。
- **Math** 还提供 `sign`、`sqrt`、`pow`、`exp`、`ln`、`log`、`hypot`、`is-finite`，以及 `pi`、`tau`、`e`。三角函数 `sin`、`cos`、`tan` 和反三角函数 `asin`、`acos`、`atan`、`atan2` 用弧度表示角度。用 `Math.to-radians(90)` 和 `Math.to-degrees(angle)` 换算。`Math.atan2(y, x)` 按坐标返回方向角。
- **Strings** 提供 `trim`、`is-empty`、`contains`、`starts-with`、`ends-with`、`replace-all`、`lowercase`、`uppercase` 和 `character-count`。字符计数按 Unicode 字素簇计算，例如 `"👩‍💻"` 算一个字符。`is-int`、`is-float` 检查修剪过的数字字符串。`Strings.to-float(input, 0.0)` 和 `Strings.to-int(input, 0)` 在解析失败时返回默认值。小数分隔符使用 `.`。
- **Strings.format** 的模板是字符串字面量。`{}` 按顺序消费字符串、整数、浮点数、布尔值、日期或时间参数。`{{` 和 `}}` 表示字面量大括号。`Strings.format("{:.2}", value)` 输出两位小数，精度可设为 0 到 20。
- **Model** 的 `count` 和 `is-empty` 跟踪模型的行数。修改已有行的内容会保留行数绑定的缓存。`Model.at(root.rows, index, fallback)` 按从零开始的索引取行，只有索引越界时才求值默认表达式。默认值类型必须和模型行一致。
- **Model.filter(model, "field", query)** 按常量字符串字段路径筛选记录，忽略大小写，去除查询两端空白。**Model.sort-by(model, "field", descending)** 按常量整数或字符串字段稳定排序。两者返回可绑定、可组合的只读模型，保留源行的键。源数据或查询变化时重新计算投影，编辑应用于源模型。**Model.lookup(model, "field", key)** 按常量字符串字段路径精确匹配键，区分大小写并保持源行顺序。按键索引在同一模型上共享。只修改其他键组时，当前查找的绑定可继续复用已有投影。
- **Model** 的 `append(model, value)`、`insert(model, index, value)`、`set(model, index, value)`、`remove(model, index)`、`move(model, from, to)` 和 `clear(model)` 可在有副作用的回调中编辑模型。第一个参数必须是可写的列表属性，行值必须匹配列表的元素类型。索引从零开始，`insert` 接受当前行数作为追加位置，`move` 的目标是行移动后的索引。无效索引会忽略。
- **Colors** 提供 `rgb`、`rgba`、`with-alpha` 和 `mix`，例如 `Colors.rgba(30, 120, 200, 75%)`。RGB 通道限制在 0 到 255，透明度与混合系数限制在 0 到 1。`mix` 对 RGBA 四个通道做线性插值。
- **I18n.locale()** 返回协商后的语言标识，例如 `"fr"`。调用会跟踪 UI 语言变化。有 Fluent 语言包时使用匹配语言或语言包的默认语言。没有语言包时返回应用设置的语言，尚未设置时返回 `"und"`。词条翻译使用 `@tr`。

### 二维点、向量和变换

`point2d` 表示位置，`vector2d` 表示位移。用 `Point2D.new(x, y)` 和 `Vector2D.new(x, y)` 构造，坐标是逻辑像素长度，读取用 `.x` 和 `.y`。两种类型的默认值都是零。整数缩放因子和角度可按现有数值规则提升为 `float`。

| 方法 | 行为 |
|---|---|
| `Transform2D.identity()` | 单位变换，也是 `transform2d` 的默认值 |
| `Transform2D.translation(x, y)` | 按两个长度平移 |
| `Transform2D.rotation(degrees)` | 按顺时针角度旋转 |
| `Transform2D.scale(x, y)` | 分别缩放两轴，负值可以镜像 |
| `Transform2D.compose(first, second)` | 先应用第一个变换，再应用第二个 |
| `Transform2D.around(transform, point)` | 绕给定点应用变换 |
| `Transform2D.map-point(transform, point)` | 映射位置，包含平移 |
| `Transform2D.map-vector(transform, vector)` | 映射位移，使用线性部分 |

```xr
export component TransformExample inherits Window {
    in-out property <float> angle: 30;
    property <transform2d> tilt <- Transform2D.around(
        Transform2D.rotation(root.angle),
        Point2D.new(40px, 20px),
    );

    Rectangle {
        x <- 12px; y <- 12px;
        width <- 80px; height <- 40px;
        background <- #378ac8;
        transform <- root.tilt;
    }
}
```

`transform` 使用节点左上角为原点的局部坐标，并在现有 `transform-rotation`、`transform-scale`、逐轴缩放与原点属性确定的变换之后应用。变换发生在布局完成后，绘制、裁剪和命中测试使用同一结果。MLIR 会折叠有限的常量几何运算和显式组合在一起的常量变换，并消去组合中的单位变换。动态组合保持源码的运算顺序。

### 图层缓存

`cache-rendering-hint` 是视觉元素的布尔属性，默认关闭。启用后，渲染器可将内容稳定的子树绘制到纹理中。元素的 `x`、`y` 和 `opacity` 变化时可以直接合成这张纹理。

```xr
Rectangle {
    x <- root.card-x;
    width <- 160px;
    height <- 80px;
    background <- #263854;
    cache-rendering-hint <- true;
    Text { text <- "状态概览"; color <- #ffffff; }
}
```

缓存使用窗口的图层预算。内容或尺寸改变后，渲染器会更新图层或直接绘制。完整缓存的图层经过设备像素对齐的矩形视口边界时，可以复用纹理并合成可见部分。轴对齐的圆角视口可缓存自身的裁剪覆盖率，在合成时应用到可见图层。亚像素平移使用双线性采样，文字和边缘的像素覆盖可能与重新栅格化不同。布局、命中测试和无障碍继续使用原来的元素树。

### 公历日期运算

- `Dates.is-valid(date)` 检查公历日期是否存在。年份范围是 1–9999，1900 年没有 2 月 29 日，2000 年有。
- `Dates.add-days(date, days, fallback)` 加减自然日。偏移量可以是负数，跨月、跨年和闰日按公历计算。
- `Dates.add-months(date, months, fallback)` 加减整月并尽量保留原来的日。目标月没有对应的日时取目标月的最后一天，例如 2024 年 1 月 31 日加一个月得到 2 月 29 日。
- `Dates.weekday(date)` 返回周一为 1 到周日为 7 的编号。无效日期返回 0。

- `Dates.parse-iso(text, fallback)` 解析 ASCII `YYYY-MM-DD` 日期。只有输入不完整、格式错误或日期不存在时才求值 `fallback`。
- `Dates.is-selectable(date, minimum, maximum, disabled-weekdays, disabled-dates)` 检查与日历控件相同的日期上下限、禁用星期和停约日期。
- `Dates.months(from, to)` 返回覆盖给定闭区间的 `[Date]`，每一项为月份的第一天。无效日期或起点晚于终点时返回空模型。
- `Dates.month-index(from, to, date)` 查找该日期所属月份在上述模型中的零基索引，日期无效或月份不在范围内时返回 `-1`。

加减遇到无效输入或结果超出年份范围时才会求值并返回 `fallback`。

`Times.parse-iso(text, fallback)` 和 `Times.is-valid-iso(text)` 接受 ASCII `HH:mm` 或 `HH:mm:ss`，省略秒时解析为零。解析失败时才求值 `fallback`。`Times.format-iso(time, show-seconds)` 返回补零后的 24 小时制文本。`Times.in-range(time, minimum, maximum)` 检查闭区间，起点晚于终点时没有可选时间。

```xr
in property <Date> selected: Date { year: 2024, month: 1, day: 31 };
out property <Date> next-month <- Dates.add-months(root.selected, 1, root.selected);
out property <int> weekday <- Dates.weekday(root.next-month);
```

### 本地化数字

`I18n.format-number(value, decimals)` 将整数或浮点数格式化为当前语言的数字，按地区规则使用数字字形、小数分隔符和分组符。小数位数限制在 0 到 20，整数在格式化前保留完整精度。

`I18n.parse-number(text, fallback)` 返回浮点数，接受当前地区的小数和分组符号。分组位置错误、非有限数值和其他无效输入返回 `fallback`。例如法语界面输入 `"1,5"` 会得到 `1.5`。

```xr
in-out property <string> amount-input;
out property <float> amount <- I18n.parse-number(root.amount-input, 0.0);
out property <string> amount-display <- I18n.format-number(root.amount, 2);
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
    Text { text <- @tr("counter-count", count: root.count); }
}
```

清单的 `default` 指定默认语言，`[locales]` 把语言标识映射到相对清单的 `.ftl` 路径。例如 `en-US = "en-US.ftl"` 和 `zh-CN = "zh-CN.ftl"`。中文语言包可以写 `counter-count = 计数：{ $count }`。编译器检查默认语言的词条和各译文引用的参数。其他语言缺少词条时，按条回退到默认语言。

Bazel 的 `xr_ui_library` 用 `assets` 声明清单和全部语言文件。Cargo 使用 `xrackc --cargo` 时会跟踪这些输入，语言包随生成代码嵌入应用。桌面宿主用 `Desktop::set_locale("zh-CN")` 更新已打开的窗口和托盘。无窗口渲染也可以通过生成组件的 `new_with_locale(context)` 共享 `LocaleContext`，随后调用 `context.set_locale("zh-CN")`。

运行 `bazel run //examples/tray:tray`，打开托盘菜单中的计数窗口，再用 **Language** 子菜单切换中英文。窗口标题、计数文案、托盘菜单和提示文字会更新，已有计数会保留。中文包有意省略窗口提示语，便于看到词条回退。
