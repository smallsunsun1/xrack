# Cargo 应用内热重载与只读附加

这个独立项目沿用异步搜索示例的 Rust 回调、Tokio 任务和 `run(ui)` 入口。

```sh
cargo run --features live-ui
```

在 VS Code 打开这个 Cargo 项目的根目录，运行 **XRack: Attach to Running App**。**XRack Running App** 面板显示当前实例的公开属性。点搜索可以观察 `query`、`loading` 和 `results`。列表显示总行数和前 12 行。保存 `ui/async.xr` 后，应用会在同一个窗口换版。

运行 **XRack: Sync Editor to Running App**，可以把当前工作区未保存的 `.xr` 文本同步到原窗口。应用在内存里使用入口和已知导入的缓冲区。写入语法错误时保留上一个有效界面，面板显示编译诊断。关闭附加面板后，应用恢复使用磁盘源码。普通 **Attach to Running App** 只读取状态。

在面板的 **Component tree** 选中 `query-field` 可查看搜索框的 `text` 和候选项，选中 `results-view` 可查看列表的 `current-index`、`row-height` 和 `row-count`。选中根组件可继续查看 `query`、`loading` 和 `results`。点击 **Go to Definition** 跳到当前运行版本的元素声明。自定义组件实例还可以打开组件类型的声明。树包含已经渲染的元素，虚拟列表显示行数。

在编辑同步面板选中节点，原窗口会高亮对应的控件。点 **Pick in Window**，再点原窗口控件，就能在树中反向定位。也可以在原窗口按住 macOS 的 **Option** 键点击。选取点击专用于检查，普通点击仍然按应用原有逻辑运行。

改动公开属性类型或回调签名时，重新构建应用。使用普通 `cargo run` 或 `cargo build --release` 可走默认 AOT 构建。应用附加和编辑同步使用 Unix 本机套接字，当前示例在 macOS 上验证。

本地验证使用从当前仓库生成的 Cargo SDK 与编译器。发布后的 Cargo 用户使用提供这些 feature 的 XRack crate 和对应的 VS Code 扩展。
