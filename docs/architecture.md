# 编译器与运行时架构

[返回项目首页](../README.md)

## Crate 分层

| Crate | 责任 |
| --- | --- |
| `xrack-syntax` | AST、类型、源码位置与诊断 |
| `xrack-parser` | 将 ANTLR4 生成的 Rust 规则上下文转换成拥有所有权的 AST |
| `xrack-ir` | 前端符号解析、类型检查、循环绑定检测与 HIR 调试视图 |
| `xrack-mlir` | 将语义分析后的 AST 成员降到 `xr`，运行 MLIR pass 并提供借用原生模块的只读视图 |
| `xrack-codegen` | 读取优化后的 MLIR 操作与 SSA use-def，用 `quote!` 生成 Rust token stream |
| `xrack-compiler` | 编译阶段编排与 `xrackc` CLI |
| `xrack-resources` | 编译器和解释器共用的资源解析、路径和内容校验 |
| `xrack-interpreter` | 从 xr MLIR 加载共享组件定义，按实例执行绑定和事件 |
| `xrack-preview` | 常驻编辑器预览宿主，动态加载界面并离屏绘制 |
| `xrack-runtime` | 惰性属性绑定、重复项缓存和 `Node` 场景快照，无 native 编译器依赖 |
| `xrack-assets` | PNG 解码、SVG 栅格化和 SVG 路径命令解析 |
| `xrack-layout` | 逻辑像素布局、容器排布、裁剪、命中测试、鼠标捕获与键盘焦点；文字测量由后端实现 |
| `xrack-renderer-femtovg` | 字体发现与回退、文字测量、FemtoVG 绘制、wgpu 纹理与 PNG 读回 |
| `xrack-platform-winit` | 原生窗口、输入事件、DPI、交换链和按需重绘 |

MLIR 的 native 实现在 `mlir/`。模块及其 context 由 `xrack-mlir` 管理，操作和 SSA 值引用只在模块存活期间有效。

## MLIR 阶段

```text
.xr → ANTLR4 Rust 解析树 → Rust AST
    → 语义分析 → TableGen builders → xr dialect（UI IR）
    → MLIR verify + XR schema checks
    → xr-collapse-private-aliases → xr-fold-constants → xr-simplify-expressions
    → xr-fold-record-projections → xr-simplify-visuals → canonicalize → CSE
    → xr-prune-constant-states → xr-simplify-state-transitions
    → xr-merge-state-cases → xr-materialize-static-states
    → xr-prune-expressions → xr-remove-unused-properties
    → xr-specialize-hidden-visuals → xr-specialize-empty-repeats
    → xr-precompile-static-paths → xr-plan-static-grid
    → xr-cache-static-gradients
    → xr-plan-update-effects → xr-plan-fixed-text-updates → xr-plan-fixed-image-updates
    → xr-plan-static-z-order → xr-plan-repeat-rows
    → xr-cache-static-subtrees → xr-specialize-static-repeats → xr-share-static-subtrees
    → xr-hoist-repeat-invariants → xr-hoist-repeat-subexpressions
    → xr-share-visual-bindings → verify
    → 按 operation 和 SSA use-def 生成 proc_macro2::TokenStream
    → xrackc 输出 Rust 源码
```

`xrack-parser` 调用生成的 `document()`，从类型化的规则上下文直接构造 `xrack-syntax` 定义的 `ast::Document`。它收集词法和语法诊断，并保留一基行列号。`--emit ast` 可以查看单文件的 AST，编译入口还会解析导入并合并文档。

`xr` 方言是编译主链路的 UI IR。语义分析验证 AST 成员并交给原生 MLIR builder。`xr.instance` 以符号引用指定组件，`xr.read` 和 `xr.assign` 携带解析后的属性所属元素或实例。MLIR 会核对引用目标、属性类型和访问权限。表达式使用 SSA operand/result 和 `!xr.int` 等类型，源码位置保留为 MLIR location。Rust 后端通过 MLIR C API 查找 operand 的定义，在 getter、绑定或赋值的使用位置生成表达式，以保留惰性求值、短路和回调中的读写顺序。`--emit hir` 提供前端调试视图。

`mlir/Xrack.td` 使用 TableGen ODS 定义 `xr` 方言。Bazel 通过 `mlir-tblgen` 生成操作和类型的 C++ builder 与校验器。Rust 通过 C ABI 传入 SSA 句柄、类型、属性和位置，C++ 调用生成的 `Op::create` 和 `Type::get`。MLIR printer 负责生成 `--emit mlir-raw` 和 `--emit mlir` 的文本。

`int` 的加、减、乘和取负按有符号 64 位回绕，解释器和生成的 Rust 使用相同规则。整数除法与取余在除数为零，或最小整数遇到除数 `-1` 时发生求值错误。解释器返回错误，生成的 Rust 触发带相同错误文本的 panic。

加载文本 MLIR 时，方言校验、常量折叠、解释器和 Rust 代码生成共用浮点字面量解码。有限的十六进制写法（例如 `0x1p-10`）和负零都按原值读取。

`xr-fold-constants` 按同一规则折叠整数算术，也折叠可表示的浮点和长度算术、比较、布尔表达式、整数转浮点、百分比转比率、整数和布尔值转字符串及常量字符串拼接。会出错的整数除法和取余，以及产生非有限浮点结果的表达式保留原操作。`xr-simplify-expressions` 去掉 `x + 0`、`x && true`、`!!x` 和空串拼接等恒等操作，同时保留动态操作数的求值。`xr-simplify-visuals` 去掉恒真的元素守卫，并保留元素及其事件路径。

`canonicalize` 简化常量条件选择，`CSE` 合并同类型的相同字面量。`xr-prune-expressions` 清理优化后没有使用者的表达式，例如常量短路跳过的 `xr.call`。独立调用用 `xr.invoke` 表示，动态条件即使两臂相同也会求值。`--emit mlir-raw` 输出优化前的 lowering 结果，`--emit mlir` 输出 Rust 后端消费的优化结果。编译库返回 token stream，`xrackc` 在写入 `.rs` 文件或标准输出时才将其转成文本。

`xr-simplify-expressions` 也会将带布尔字面量分支的选择表达式改写成短路运算，例如 `condition ? value : false` 改为 `condition && value`。被跳过的分支仍保持惰性求值。

`xr-fold-record-projections` 将完全由字面量构造的记录字段读取替换为字面量，也支持通过私有不可写属性读取嵌套字段。替换后继续折叠依赖该字段的算术与绑定，清理阶段移除已无引用的私有常量记录。可写属性、双向链接和含动态字段的记录保留原有求值路径。

`xr-collapse-private-aliases` 把同组件内、指向根属性存储的私有别名链折叠成直接读写，公开别名仍可通过原名访问。折叠后只经私有别名读取的常量属性也能继续参与常量传播。

`xr-remove-unused-properties` 删除无人读取且未被写入或链接的私有属性。删除绑定后沿初始化表达式追踪依赖，可一并删除整条无用的私有绑定链。公开属性和可写端点继续保留原有存储。

`xr-plan-fixed-text-updates` 对具有固定绝对宽高的 `Text` 和 `Label` 标记文本内容的更新只需要重绘。符合条件的窗口帧可复用布局，无障碍树仍会读取最新的完整文本。布局测量也直接使用显式宽高，字形排版留给绘制阶段。依赖自然尺寸或尺寸受状态、动画影响的绑定保留常规失效路径。

`xr-materialize-static-states` 把编译期无条件选中的字面量视觉状态改写成普通绑定。网格列号、元素尺寸等值可继续参与静态场景规划，被覆盖的旧绑定也可清理。含动态值、组件输入覆盖或状态转场的选择器仍按原路径执行。

`xr-simplify-state-transitions` 删除恒禁用的转场，并省去恒启用转场的检查。`xr-merge-state-cases` 合并同一选择器中相邻且覆盖目标与 SSA 值相同的状态。合并后的条件按声明顺序短路求值，多个状态只保留一份覆盖操作。仍有转场的状态组保留每个状态的索引。

`xr-specialize-empty-repeats` 对可证明为空、且子树没有可寻址节点或状态注册的普通重复项生成稳定占位节点，省去 Rust 后端的重复器存储和求值。虚拟视图保留模板。

`xr-specialize-static-repeats` 将最多 128 行、内容为常量且无交互状态的匿名整数重复项编译为惰性场景快照。生成 Rust 和解释器只构造一次模板，复用快照及子节点存储，并保留每行的键和普通布局或定位容器。虚拟视图按可见范围创建行。

`xr-plan-static-z-order` 也记录已经按声明顺序排列的静态子项，让布局直接沿用该顺序。修改场景节点会使静态顺序标记失效。

`xr-fold-constants` 可以折叠由字面量构造的 `Date`、`Time` 和 `Font` 的字段读取，含私有不可写属性上的读取。`Time` 按运行时的时、分、秒范围归一化。视觉绑定缓存也识别记录及这些类型的标量字段，可共享等价绑定或提升与行无关的字段读取。

`xr-share-visual-bindings` 在同一组件渲染作用域或同一重复项模板中，为结构相同且可共享的视觉表达式标记 `xr.visual_cache_group`。例如两个 `Text` 的 `text: "Count " + root.count;` 会共用一个惰性 `Property<String>` 缓存。可共享的表达式可以引用常量、根属性和当前行参数。依赖当前行的缓存按行保存，含回调调用或导航读取的表达式仍独立求值。

视觉绑定共享和循环不变量提升也识别响应式渐变和 Fluent 翻译。多个使用点可以共用一份色标数组或翻译结果，主题色、翻译参数或语言变化时通过依赖追踪失效。独立于行参数的表达式只在首次实际使用时求值。

`xr-hoist-repeat-invariants` 把完全独立于行参数的视觉绑定放到组件级惰性缓存，同一组件中重复项内外的等价绑定可以共用该缓存。空列表没有行级求值，读取 `item`、`index`、子元素状态或调用回调的表达式保留原有求值位置。`xr-specialize-hidden-visuals` 将恒假条件的渲染降为 `Node::hidden()`，省去生成不可达的渲染子树，并保留节点槽位和事件路径。

`xr-hoist-repeat-subexpressions` 也能提升混合绑定中的公共前缀，例如 `("Caption " + root.caption) + item` 的左侧。生成的缓存仍在原来的分支使用点读取。`xr-plan-update-effects` 标记 `Rectangle.background` 和 `Text.color` 只影响绘制。若帧输入只有已标记的纯绘制属性变化，且上次布局直接使用原始场景，窗口宿主用新场景绘制并复用原有布局。`xr-cache-static-subtrees` 标记仅包含静态视觉元素的子树，生成代码按线程复用场景节点。静态节点克隆共享底层数据，修改副本时再复制受影响的节点。普通动态节点仍直接持有数据。

`xr-precompile-static-paths` 标记常量 `Path.commands`，资源阶段复用已有解析器生成线段和源坐标边界，并嵌入 Rust。绘制阶段仍根据实际尺寸、`fit`、描边和填色变换路径。动态命令继续按运行时值解析。`xr-plan-static-grid` 对子项结构、可见性和跨度固定的 `GridLayout` 预计算单元格占位。布局仍测量文字、分配轨道大小并保留原场景索引。手写节点、动态网格及被编辑的网格快照继续使用同一套运行时占位算法。

`xr-cache-static-gradients` 标记色标和角度都为常量的渐变。相同的常量渐变在生成的 Rust 模块里共享一份惰性初始化的色标数组。依赖响应式属性的渐变仍随属性变化更新。`xr-share-static-subtrees` 让相同的匿名静态子树共享写时复制快照。被布局几何或焦点绑定观察的节点保留实例独立的句柄。

`xr-plan-fixed-image-updates` 在普通 `Image` 的绝对宽高可证明固定时，让换图复用布局，并跳过固有图片尺寸测量。新图的适配、裁剪和绘制仍读取当前资源。依赖图片固有尺寸的节点继续随资源变化更新布局。

Bazel 使用 `rules_rust_bindgen` 和同一份 LLVM 23 的 clang/libclang 生成 `c_bindings.rs`。`xrack_mlir_bindings` 将它作为 crate 根文件，供 `xrack-mlir` 和 Rust Analyzer 的项目图使用。Rust 持有原生模块，构建和优化都在这个模块上完成。C++ 接口负责注册方言、调用生成的 builder、配置 pass 和跨操作校验。

MLIR 的 AnalysisManager 缓存属性引用与场景分析。私有标量属性的初值为常量且没有赋值或双向链接时，读取可继续折叠。无引用的常量私有属性会省去存储。状态裁剪保留首个可达的恒真分支，重映射覆盖和转场使用的索引。

固定的兄弟节点可以带预计算的 `z` 顺序，布局使用该顺序并保留声明索引。合格的组件型虚拟行按 `RowKey` 缓存可见行场景，读取位置索引的模板在行移动时重新求值。

滚动时，布局缓存复用符合条件的列表行和带子树的表格单元格的局部布局。行身份和表格逻辑列决定缓存位置，每帧按当前偏移重新放置并裁剪。视口边缘新出现的项、尺寸变化以及变换或复杂裁剪仍走正常布局。生成 Rust 和解释器都通过共享的渲染器接入这条路径。

`StackView` 滑动转场可以把离场页栅格化到 GPU 纹理，后续帧复用纹理并更新位移和裁剪。动画打断会延续已呈现的帧，纹理在离场快照释放后回收。只含静态绘制节点的透明度子树也可以复用 GPU 纹理，每帧更新整组透明度。内容、布局、字体或主题变化会更新纹理。两种缓存共享显存预算。以下压测报告滚动帧的场景准备、绘制提交及等待 GPU 完成时间，以及页面转场的合成开销。

```sh
bazel build //tests:gpu_test
"$(bazel info bazel-bin)/tests/gpu_test" --ignored --nocapture --test-threads=1 benchmark_
```

用 `bazel test //tests:mlir_filecheck_test` 运行 LLVM FileCheck。用例放在 `tests/pass_cases/`，支持 `.mlir` 和 `.xr` 文件。用 `CHECK` 验证优化结果，可用 `RAW` 同时验证优化前的 IR。测试通过同一条优化流水线处理原生 IR 和从 DSL 降低的 IR。
