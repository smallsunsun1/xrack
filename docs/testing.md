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

基准固定 640×360 逻辑像素、2 倍渲染比例和 10,000 行。每帧滚动并修改一条可见记录。每个后端预热 30 帧，再统计 240 帧。JSON 报告包含 GPU、后端、p50/p95/p99、绑定求值和布局计数，保存在测试的 `test.outputs/ui-frame-benchmark.json` 中，也会写入测试日志。

基准通过真实 GPU 绘制到离屏纹理，并串行等待提交完成。结果可对比场景准备、提交及 GPU 等待开销。桌面合成器呈现、掉帧率和输入到实际显示的延迟需要另外做原生窗口测量。比较时使用同一台机器、同一 GPU 后端和相同构建参数，先重复运行测出正常波动范围，再设性能回归门槛。
