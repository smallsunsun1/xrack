# XRack

XRack 是受 Slint 启发的声明式 UI 框架。用 `.xr` 定义组件、属性绑定和回调，用 Rust 编写应用逻辑。ANTLR4 前端将界面降低到 MLIR 的 `xr` 方言，同一份优化后的 IR 可交给解释器运行或生成 Rust。桌面宿主使用 winit，FemtoVG 通过 wgpu 绘制。

[项目网站](https://smallsunsun1.github.io/xrack/) · [控件与语法参考](https://smallsunsun1.github.io/xrack/reference.html)

## 快速开始

在已有 Rust 项目中，用 `xrack-build` 在构建时编译 `.xr`。需要在运行时加载 `.mlir` 时，启用 `xrack` 的 `interpreter` feature。 [Cargo 完整用例](docs/cargo.md) · [可运行示例](examples/cargo-consumer)

开发框架自身需要 Bazel 9.2.0 或 Bazelisk，以及用于生成 ANTLR 源码的 JDK 17+。Bazel 会构建锁定版本的 LLVM/MLIR。

```sh
bazel run //examples/counter:counter
bazel run //examples/counter:counter_headless
bazel test //...
```

在 VS Code 安装 [XRack 扩展](https://marketplace.visualstudio.com/items?itemName=SunJiahe.xrack)，打开 `.xr` 后运行 **XRack: Open Live Preview**。预览会读取当前编辑缓冲区，并随修改更新。 [编辑器配置](docs/editor.md)

## 文档

| 主题 | 内容 |
| --- | --- |
| [Cargo 接入](docs/cargo.md) | 构建时生成 Rust、运行时加载 MLIR |
| [.xr 语言参考](docs/language.md) | 语法、属性、回调、内建函数和翻译 |
| [控件与布局参考](docs/controls.md) | 布局、表单、图像与交互 |
| [3D 场景](docs/scene3d.md) | 视口、材质、模型加载、物理与粒子示例 |
| [模型与虚拟视图](docs/models.md) | 滚动、列表、树和表格 |
| [桌面宿主与异步](docs/desktop.md) | UI 线程、多窗口、托盘 |
| [VS Code 与预览](docs/editor.md) | .xr LSP、MLIR 调试、项目图和编译数据库 |
| [编译器与运行时架构](docs/architecture.md) | Crate 分层、MLIR 降低和优化 |
| [项目构建与验证](docs/development.md) | 工具链、Bazel 目标、缓存和测试 |
| [iOS 模拟器示例](examples/ios/README.md) | 用 Bazel 构建并运行触屏待办应用 |
| [Android 模拟器示例](examples/android/README.md) | 用 Bazel 打包同一份触屏待办应用的 arm64 APK |
| [巡检工作台](examples/inspection-workbench/README.md) | 万行虚拟表格、详情表单与后台项目读写 |
| [桌面应用示例](examples/desktop-notes/README.md) | 笔记编辑、原生文件面板、Cargo 测试与 macOS 应用包 |
| [应用 UI 测试](docs/testing.md) | Cargo 无窗口测试、双后端回归与 GPU 基准 |
| [发布与 CI](docs/release.md) | crates.io、编译器归档与平台矩阵 |
| [仓库开发约定](AGENTS.md) | 双后端一致性和语法同步 LSP 的工作流程 |

项目采用 [MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE) 双许可。
