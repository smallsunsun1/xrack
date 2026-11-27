# XRack Notes

一个独立的 Cargo 桌面应用，用于打开、编辑和保存本地 UTF-8 文本或 Markdown 源文件。界面用 `.xr` 写成，`xrack-build` 在构建时生成 Rust。打开、保存和另存为使用原生文件面板。打开另一份文档或关闭窗口前，如果有未保存的修改，应用会先请求确认。

## 构建与运行

Cargo 案例固定使用与仓库一致的 XRack 版本。从 crates.io 构建时，`xrack`、`xrack-build` 和开发依赖 `xrack-testing` 需要同一发布版本。构建脚本自动获得对应平台的预编译 `xrackc`。

```sh
cd examples/desktop-notes
export CARGO_TARGET_DIR=../../.tools/cargo-build
cargo test --release
cargo run --release
```

在 macOS 用 ⌘O、⌘S、⇧⌘S 打开、保存和另存为，Windows 和 Linux 使用对应的 Ctrl 组合。可把已有文档作为第一个命令行参数传入。

如果测试尚未发布的仓库改动，从仓库根目录使用已经生成的原生编译器归档。脚本将 SDK 和应用暂存到临时目录，独立运行 Cargo 测试并构建 release 可执行文件。

```sh
export CARGO_TARGET_DIR="$PWD/.tools/cargo-build"
python3 tools/test_cargo_consumer.py .tools/dist aarch64-apple-darwin --desktop-notes
```

将平台参数替换为归档对应的宿主三元组。框架仓库内的 DSL 生成也可单独验证。

```sh
bazel build //examples/desktop-notes:notes_ui
```

## 本机 macOS 应用包

从仓库根目录运行以下命令。脚本创建一个本地 ad-hoc 签名的 `.app` 供验证，输出路径需要是新目录。

```sh
sh tools/bundle-macos-app.sh \
  .tools/cargo-build/release/xrack-desktop-notes \
  ".tools/desktop-notes/XRack Notes.app" dev.xrack.notes 0.10.0
open ".tools/desktop-notes/XRack Notes.app"
```

## 验收范围

- `cargo test` 驱动实际组件，覆盖编辑、保存和重新打开、保存失败保留草稿、文件面板取消、关闭确认，以及保存期间收到较新内容时的状态更新。文件 I/O 使用测试临时目录。
- 桌面执行器用 Tokio 的阻塞线程执行文件读写，再由 `spawn_local` 把结果送回 UI 线程。保存先写入同目录的临时文件，再替换目标。
- 原生窗口验收可用一份临时 `.md` 文件测试打开、编辑、另存为、关闭确认和重新打开。另存为后核对磁盘上的文件内容。
