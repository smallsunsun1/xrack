# 应用 UI 测试

[返回项目首页](../README.md)

`xrack-testing` 是业务项目的 Cargo 开发依赖，随 XRack 一起发布。它接受实现 `Component` 的实例，同一套操作可用于由 `xrack-build` 生成的 Rust 组件和从优化后 MLIR 创建的解释器实例。默认无窗口运行，使用与桌面共享的布局、命中测试和输入状态，无需显卡和系统字体。

## 在 Cargo 项目使用

在已有的 [Cargo 应用](cargo.md) 中加入与所用 XRack 版本一致的 `xrack-testing`。下面是仓库当前版本的配置，使用 crates.io 时以实际发布版本为准。

```toml
[dev-dependencies]
xrack-testing = "=0.8.0"
```

将下面的代码放在 `tests/ui_test.rs`。例子使用 [Counter](../examples/cargo-consumer/ui/counter.xr) 的控件 ID `increment`，由 `build.rs` 生成组件。运行 `cargo test`。

```rust
use xrack_testing::{ActivationKey, Size, UiDriver};

xrack::include_modules!();

#[test]
fn increments_from_mouse_and_keyboard() {
    let mut ui = UiDriver::headless(Counter::new(), Size::new(240.0, 120.0));

    ui.click("increment");
    assert_eq!(ui.component().get_count(), 1);
    assert_eq!(ui.focused(), Some("increment"));

    assert!(ui.activate(ActivationKey::Enter));
    assert_eq!(ui.component().get_doubled(), 4);
}
```

- `find_path("save")` 用来检查目标是否存在，返回缺失或重名错误。点击会经过命中测试，目标被遮挡或位于视口之外时测试会失败。
- `scroll`、`tab`、`fill_focused` 和 `activate` 可组合成用户操作。断言可以读取 `ui.node(id)` 的可见属性、`ui.focused()` 或 `ui.component()` 的业务状态。
- 虚拟列表和表格只查找当前已实例化的行。先滚到目标，再用模型的 `row_key(index)` 和 `find_row_path` 查找行。表格可用 `find_table_cell_path(view_id, key, column)`。路径反映当前布局，滚动、排序或数据更新后应重新解析。
- 测试可通过 `ui.update(|component| { … })` 注入业务结果并刷新场景。异步请求用测试桩控制完成顺序，DSL Timer 用 `advance_time(Duration)` 推进。该时钟用于 DSL Timer、编辑和测试驱动产生的交互时间，应用的 Tokio runtime 由应用测试自行控制。
- `UiDriver::headless` 使用近似字宽。需验证字体、像素输出、IME 或系统窗口行为时，应使用对应的原生测试。GPU 用例可复用驱动的 `scene()`、`layout()` 和 `input()`。

## 运行仓库基准

`//tests:ui_scenario_test` 验证万行表格滚动、修改一条可见记录、键盘编辑和插入后保持行身份。同一用例运行 AOT 和解释器两次，还验证键盘激活和确定性的 DSL Timer。

```sh
bazel test //tests:ui_scenario_test
bazel test //tests:ui_frame_benchmark -c opt --nocache_test_results \
  --test_arg=--nocapture --test_output=all
```

基准固定 640×360 逻辑像素、2 倍渲染比例和 10,000 行。每帧滚动并修改一条可见记录。每个后端预热 30 帧，再统计 240 帧。JSON 报告包含 GPU、后端、p50/p95/p99、绑定求值、布局和渲染计数，保存在测试的 `test.outputs/ui-frame-benchmark.json` 中，也会写入测试日志。

两条后端共享 GPU 设备，各自创建 renderer 和离屏目标。加 `--test_env=XRACK_BENCH_INTERPRETER_FIRST=1` 可反转执行顺序，报告会记录实际顺序。比较 DSL 执行开销时先看 `prepare_ms`。它包括滚动、更新与布局。`gpu_wait_ms` 是提交后等待设备完成的墙钟时间，包含主机等待开销。

基准通过真实 GPU 绘制到离屏纹理，并串行等待提交完成。结果可对比场景准备、提交及 GPU 等待开销。桌面合成器呈现、掉帧率和输入到实际显示的延迟需要另外做原生窗口测量。比较时使用同一台机器、同一 GPU 后端和相同构建参数，先重复运行测出正常波动范围，再设性能回归门槛。

矢量绘制基准在 640×480 逻辑像素的窗口内绘制 192 个带描边的三次曲线路径，像素比例为 2。路径位置和尺寸固定，填充色逐帧切换。分别测一种和十二种轮廓，每种场景对照直接绘制、轮廓缓存和 GPU 覆盖纹理缓存。预热 10 帧后，统计 60 帧的绘制提交和 GPU 完成耗时。

```sh
bazel test //tests:gpu_test -c opt --nocache_test_results --test_output=all \
  --test_arg=--ignored --test_arg=--nocapture --test_arg=benchmark_vector_geometry
```

在同一命令上加 `--test_env=XRACK_VECTOR_BENCH_GRADIENT=1`，将填充改成逐帧更新色标的线性渐变。路径、位置、描边和采样数与纯色场景一致。

矩形动画基准使用相同的视口和像素比例，更新 600 个不同颜色的矩形。分别测量移动和同时改变透明度的帧，记录布局、提交和等待 GPU 完成的整帧耗时。

```sh
bazel test //tests:gpu_test -c opt --nocache_test_results --test_output=all \
  --test_arg=--ignored --test_arg=--nocapture --test_arg=benchmark_rectangle_animation
```

加 `--test_env=XRACK_RECTANGLE_BENCH_ROUNDED=1` 可测 2 逻辑像素圆角的同一场景，保留抗锯齿与半透明混合。

卡片动画基准在同一视口中绘制 24 张含文字和圆角色块的卡片。分别测量只改变整组透明度和同时移动卡片，记录纹理重新栅格化次数、复用次数及整帧耗时。

```sh
bazel test //tests:gpu_test -c opt --nocache_test_results --test_output=all \
  --test_arg=--ignored --test_arg=--nocapture --test_arg=benchmark_translucent_card_animation
```

在同一命令上加 `--test_env=XRACK_CARD_BENCH_SHADOW=opaque` 测试不透明卡片的模糊阴影，加 `--test_env=XRACK_CARD_BENCH_SHADOW=fade` 测试带同样阴影的整组透明度动画。阴影基准还会保留已预热的 GPU 管线，每帧清空已保留的纹理，测量缓存全部失效时的帧开销。

再加 `--test_env=XRACK_CARD_BENCH_DYNAMIC=1` 可以逐帧更新正文，观察独立阴影纹理的复用次数、整组捕获次数，以及临时透明图集记录和提交的次数。

用 `--test_env=XRACK_CARD_BENCH_COUNT=1` 或 `--test_env=XRACK_CARD_BENCH_COUNT=4` 可缩小卡片数量。加 `--test_env=XRACK_CARD_BENCH_NO_OPACITY_ATLAS=1` 可在同一二进制中对照常规透明层。

用 `--test_env=XRACK_CARD_BENCH_SUBPIXEL=1` 测连续的小数平移，或用 `--test_env=XRACK_CARD_BENCH_SUBPIXEL=independent` 测四组不同的采样位置。

长富文本滚动基准在 640×480 逻辑像素的视口中，以 2 倍像素比例滚动 1,200 段带链接、混合文字方向和 Emoji 的 Markdown。分别记录完整绘制和按字体墨迹裁剪的布局、提交与整帧耗时。

```sh
bazel test //tests:gpu_test -c opt --nocache_test_results --test_output=all \
  --test_arg=--ignored --test_arg=--nocapture --test_arg=benchmark_long_styled_text_paint
```

富文本追加基准使用同一文档和视口。每帧追加一段并滚动，完整解析 Markdown，再分别测量全量排版和增量排版的耗时，统计范围包含 GPU 完成等待。

```sh
bazel test //tests:gpu_test -c opt --nocache_test_results --test_output=all \
  --test_arg=--ignored --test_arg=--nocapture --test_arg=benchmark_styled_text_append
```

加上 `--test_env=XRACK_STYLED_TEXT_PARAGRAPHS=6000` 可测更长文档。`--test_env=XRACK_STYLED_TEXT_LATEST=1` 使滚动位置跟随末尾，`--test_env=XRACK_STYLED_TEXT_TRANSFORM=rotate` 或 `zoom` 让视口在追加时旋转或缩放。`--test_env=XRACK_STYLED_TEXT_OPTIMIZED_ONLY=1` 只测默认的增量路径。输出包含复用和裁掉的片段数。

富文本交互基准预先布局 10,000 段文档并滚到末尾，分别测链接矩形枚举、鼠标命中、Tab 导航、选区命中和水平光标移动。输出包含普通和旋转视口的 CPU 耗时。

```sh
bazel test //tests:gpu_test -c opt --nocache_test_results --test_output=all \
  --test_arg=--ignored --test_arg=--nocapture --test_arg=benchmark_long_styled_text_interaction
```

宽表格基准用 50,000 行和 512 或 8,192 列测试横向滚动。表格包含冻结列和隐藏列，以 2 倍像素比例绘制，分别报告准备、提交和 GPU 完成耗时。

```sh
bazel test //tests:gpu_test -c opt --nocache_test_results --test_output=all \
  --test_arg=--ignored --test_arg=--nocapture --test_arg=benchmark_wide_table_scroll
```

设 `XRACK_TABLE_BENCH_PIXELS` 为已有的输出目录，可以在计时结束后保存包含混合文字和居中单元格的 RGBA 帧。加 `XRACK_TABLE_BENCH_RTL=1` 可保存右向左布局的对照帧。


帧报告的渲染计数包含图元分发、四边形和字形批次、渲染器队列提交、已观测的图像上传字节与保留几何的上传和命中。四边形顶点字节按实际提交的 XYUV 顶点计算。FemtoVG 内部展开的全部 GPU draw call 和上传需要使用后端追踪。CPU 准备计数另含模型扫描行和投影创建时的行克隆。实时附加快照的 `renderSamples` 还报告输入批次到调用 `present` 的耗时。真正显示到屏幕的时间需要平台呈现时间戳。离屏基准报告准备、提交和等待完成的耗时。


### GPU 命令与屏幕呈现

令 `XRACK_GPU_TRACE_DIR` 指向本地目录，设备会记录提交给 wgpu 的完整命令。正常退出进程后，用分析器读取设备子目录中的 `trace.ron`。

```sh
bazel run //crates/xrack-renderer-femtovg:xrack-gpu-trace -- /path/to/capture/trace.ron
```

报告按提交统计绘制命令，并展开 render bundle 中实际执行的命令。标准交换链的 `Present` 和 Metal 的呈现标记会划分统计区间。GPU 决定数量的间接多绘制单独计数，避免把上限当作实际数量。驱动内部的命令拆分由平台 GPU 捕获验证。追踪文件可能包含上传的资源，仅应在需要诊断时开启。

macOS Metal 窗口使用 `XRACK_PROFILE_UI=1 XRACK_PROFILE_PRESENT=1` 可在 Live Attach 的 `presentedFrames` 中读取同一 drawable 的真实呈现时间，也可通过 MCP 的 `get_update_plan` 获取。它与 `renderSamples` 以帧 ID 对应。AOT 应用设置 `XRACK_PROFILE_PRESENT=1` 后可通过 `xrack::diagnostics` 的 `frame_presented` 事件读取反馈，异步搜索示例可同时设置 `XRACK_TRACE_UI=1` 打印事件。一帧未收到呈现回调时，Live Attach 保留提交采样。未暴露等价平台反馈的后端仍提供 `inputToPresentSubmitMicros`。
