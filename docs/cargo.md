# Cargo 接入

[返回项目首页](../README.md)

## 在 Cargo 项目中使用 XRack

以下接入方式适用于 XRack `0.5.1`。`xrack` 提供组件、模型、桌面宿主及布局、渲染和资源模块，`xrack-build` 在 Cargo 的构建阶段编译 `.xr`。

使用 Rust 1.99.1 或更新版本新建应用。

```sh
cargo new --bin --edition 2024 hello-xrack
cd hello-xrack
mkdir ui
```

把下面两段加入 `Cargo.toml`。`[dependencies]` 下只需声明 `xrack`。

```toml
[dependencies]
xrack = "=0.5.1"

[build-dependencies]
xrack-build = "=0.5.1"
```

将界面保存为 `ui/app.xr`。

```xr
export component App inherits Window {
    title: "Hello XRack";
    width: 320px;
    height: 160px;
    in-out property <int> count: 0;

    VerticalLayout {
        padding: 16px;
        spacing: 12px;
        Label { text: "Count: " + root.count; }
        increment := Button {
            text: "Add one";
            clicked() => { root.count += 1; }
        }
    }
}
```

在 `build.rs` 中编译一个入口文件。入口可以导入其他 `.xr` 文件。

```rust
fn main() -> std::io::Result<()> {
    xrack_build::compile("ui/app.xr")
}
```

在 `src/main.rs` 中引入生成的组件并启动原生窗口。

```rust
xrack::include_modules!();

fn main() -> Result<(), String> {
    xrack::run(App::new())
}
```

运行 `cargo run`。点击 **Add one** 后，绑定到 `count` 的文字会更新。修改 `.xr` 文件后重新运行 Cargo 即可看到变更。

发布的 `xrack-build` 通过 Cargo 选择宿主平台的 `xrackc-bin-*` 包。编译器由 Bazel 构建，Cargo 下载并校验归档，构建脚本把可执行文件解压到 `OUT_DIR`。编译器输出 Cargo 的文件变更指令，覆盖入口、导入文件和内嵌资源。交叉编译时编译器仍在宿主机运行。

[Cargo 示例](../examples/cargo-consumer) 使用同一入口。框架开发者可以用 `XRACKC` 指向同版本的自定义编译器，也可以在仓库根目录通过暂存的宿主包验证默认接入。

```sh
CARGO_BAZEL_ISOLATED=false python3 tools/package_xrackc.py
python3 tools/test_cargo_consumer.py .tools/dist aarch64-apple-darwin
```

将最后一个参数换成 `rustc -vV` 输出的宿主三元组。该命令在独立的临时目录构建示例。

### 从 Cargo 加载 MLIR

启用 `xrack` 的 `interpreter` feature 后，可以在应用运行时加载 `xrackc --emit mlir` 生成的 `.mlir` 文件。

```toml
[dependencies]
xrack = { version = "<发布版本>", features = ["interpreter"] }
```

```rust
use xrack::interpreter::load_mlir_file;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let program = load_mlir_file("counter.mlir")?;
    let ui = program.create("Counter")?;
    xrack::run(ui)?;
    Ok(())
}
```

用 `ui.on` 注册 `Fn` 回调。对于非 `pure` 回调，也可以用 `ui.on_mut` 注册直接修改捕获状态的 `FnMut` 闭包。

用 `ui.get_snapshot("rows")` 获取与生成 Rust 的 `get_rows()` 对应的独立快照。需要原地更新模型或保留行标识时，用 `record_model` 或 `string_model` 取得共享模型。

发布包按目标平台选择 Bazel 预编译的 MLIR 静态库。应用构建需要 Rust 和平台常规链接工具，不需要安装 LLVM、Bazel 或 ANTLR。资源路径相对于 `.mlir` 文件解析。可通过 `python3 tools/test_cargo_consumer.py .tools/dist <宿主三元组> --interpreter` 验证发布产物。

### 播放本地视频和 HLS

启用 `xrack` 的 `media` feature 后，`xrack::run` 和 `xrack::run_desktop` 会自动注册 `MediaPlayer` 后端。可与 `interpreter` 同时启用。Cargo 按目标平台选择包含 Bazel 预编译媒体库的 `xrack-media-bin-*` 包，安装依赖时无需安装 FFmpeg 或 Bazel。

```toml
[dependencies]
xrack = { version = "=0.5.1", features = ["media"] }
```

媒体库随应用可执行文件携带。平台为 macOS arm64、Linux GNU arm64/x64 和 Windows MSVC arm64/x64。自建宿主调用 `xrack::install_media()` 注册后端。需要替换媒体库时，可将 `XRACK_MEDIA_LIBRARY` 指向兼容库的绝对路径。发布页附有对应的 FFmpeg 源码和构建配置。 [控件用法](controls.md#mediaplayer)

用 `MediaPlayer { source: "https://example.org/live.m3u8"; autoplay: true; }` 播放 HLS 流。独立 Cargo 验证运行 `python3 tools/test_cargo_consumer.py <宿主三元组> --media --published`，会从 crates.io 解析依赖并用仓库内的小型分片启动本地 HTTP 流。
