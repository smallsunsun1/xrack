# 发布与 CI

[返回项目首页](../README.md)

## 0.10.0 发布说明

XRack 0.10.0 继续降低复杂界面的绘制开销，扩展可交互的 3D 视口，并在编译期明确属性初始化、绑定和写入的行为。

- **渲染性能**。改进字形塑形缓存、字形图集上传与批量采样，减少表格滚动和富文本更新时的重复工作。改进图层合成与局部重绘相关路径，保留宽表格、流式 Markdown 和动画基准用于比较帧耗时。
- **3D 场景**。`SceneView3D` 扩展材质、阴影、粒子、实例化与渲染捕获，支持更多几何查询。可以用 `MeshLoader3D` 在后台打开本地模型，用拾取结果联动普通 2D 控件。3D 物理加入刚体、关节、碰撞查询和角色控制。VS Code 的 3D Stats 可以按视口检查渲染通道。
- **.xr 语言**。`:` 表示一次性初始化，`<-` 表示持续跟随，`<->` 表示双向连接可写存储。编译器检查跨组件和回调中的非法写入。增加视图块式 `if / else` 和 `for`、可取值的 `if`、带签名的就地回调实现，以及 `let` 的可选类型标注。
- **文档与编辑器**。GitHub Pages 加入 3D 入门、示例和视口方法参考。LSP、格式化与 VS Code 高亮同步支持新语法，`MeshLoader3D` 的只读输出提供类型悬停提示。

从 0.9.0 迁移时，检查原来用 `:` 持续跟随上游值的声明并改为 `<-`。需要一次性取初值时继续使用 `:`。两个属性共享可写状态时改用 `<->`，并确保两端具有相应写入权限。 [属性语义与例子](language.md#属性初始化与绑定)

[性能基准](testing.md) · [3D 场景指南](scene3d.md) · [.xr 语言参考](language.md) · [Cargo 接入](cargo.md)

## 0.9.0 发布说明

这版重点优化复杂界面的渲染。滚动大表格、追加富文本或播放动画时，XRack 会复用更多已完成的排版和绘制结果，减少每帧重复工作。

- **窗口和图层**。跟踪窗口的变化区域，在适用场景中局部重绘。保留图层缓存，把圆角、阴影和透明合成等常用操作批量提交给 GPU。
- **表格和列表**。复用滚动时的行布局、单元格字形位置和字形纹理，降低可见区域反复变化时的排版与绘制开销。
- **富文本**。保留已经排版的段落，增量处理后续追加的 Markdown，并根据可见区域绘制。Agent Workbench 用这条路径显示流式对话、命令输出和文件链接。
- **3D**。新增基于 wgpu 的 `SceneView3D`，支持 glTF、可编程材质和后处理、阴影、拾取、GPU 实例化、粒子与拖尾。3D 视口可嵌入 2D 界面，并复用已绘制的视口纹理。
- **性能验证**。新增宽表格、长富文本、矢量路径和透明卡片动画基准，分别报告场景准备、提交、GPU 等待和总耗时。

[性能基准运行方式](testing.md) · [3D 示例](../examples/puzzle3d/README.md) · [Agent Workbench](../examples/agent-workbench/README.md)

## 0.8.0 发布说明

- `.xr` 回调支持 `if`、`for` 和记录字段更新，AOT 与解释器共用相同的语言语义。
- Android 使用 Rust 的 `android-activity` 宿主。Bazel 可构建固定的一组 APK 并运行设备回归，覆盖导航、控件、输入法、无障碍和系统文件操作。
- iOS 扩展 UIKit 文本编辑、外接键盘与指针、拖放和无障碍，并响应系统 Button Shapes 设置。加入 ARM64 真机编译链接目标，签名安装与真机交互仍待验收。
- 异步 UI 场景可以记录、回放并保存为回归测试。实时调试按请求关联热重载代次、属性写入和窗口更新。动效时间轴展示采样画面、布局坐标和各阶段耗时。
- 可选媒体功能增加基于 FFmpeg 的桌面录音和音频输入设备发现，并提供录制示例。

## 准备发布包

`python3 tools/package_xrackc.py` 用 Bazel 构建当前宿主平台的 `xrackc` 和原生 MLIR 静态库，在 `.tools/dist/` 生成压缩包和 SHA-256 校验文件。压缩包也携带生成的 Rust 绑定、ANTLR 解析器源码和许可证。编译器和 MLIR 的 Cargo 包暂存模板分别在 `bundles/compiler` 和 `bundles/mlir`。

```sh
cargo package --locked -p xrack-runtime -p xrack-assets -p xrack-layout \
  -p xrack-renderer-femtovg -p xrack-platform-winit
```

Cargo 会验证打包后的源码，并在发布时按依赖顺序上传这些 crate。Cargo 版桌面宿主使用上游 AccessKit，Bazel 版带有 macOS 三态复选框无障碍值修补。

发布版 `xrack-renderer-femtovg` 通过精确版本依赖取得同版本的 `xrack-femtovg`。打包工具从 `Cargo.lock` 校验 FemtoVG 上游源码，在暂存区应用与 Bazel 相同的补丁。应用继续通过 `xrack` 使用渲染器，使用底层 FemtoVG 类型时从 `xrack::renderer::femtovg` 导入。

手动运行 [Release XRack](../.github/workflows/release-crates.yml) 并输入 workspace 版本号，可以预检运行时包。正式发布时，给 `main` 上已合并的提交推送与 workspace 版本一致的 tag。发布流程跑各平台 CI，并从五份 Bazel 归档生成宿主编译器包、目标平台 MLIR 包和解析器包。发布依次上传运行时和语法基础包、补丁版 FemtoVG 和渲染器、Bazel 产物对应的包，以及 `xrack-interpreter`、`xrack` 和 `xrack-build` 等上层包。GitHub Release 提供归档及校验文件。

在 GitHub 的 `crates-io` 环境配置 `CARGO_REGISTRY_TOKEN` secret，并允许发布 tag 使用该环境。首次发布使用允许发布新 crate 的 token。上传中断后，可从同一次 workflow 重试，脚本会校验已发布的版本及对应的源码提交。暂存的渲染包还会校验上游源码与补丁摘要。升版本时同步 workspace 版本、内部 crate 依赖、宿主包版本、Bazel 目标版本和 Cargo 消费示例。

如果已有稳定 tag 的流水线需要更新发布工具，在 `main` 上手动运行 **Release XRack**，输入版本并勾选 **Publish an existing release tag from main**。`crates-io` 环境需允许 `main` 发布。工作流从 tag 读取源码，复用该提交通过 main CI 时生成的产物，并用当前工作流版本的打包工具重新验证和发布 Cargo 包与 GitHub 归档。主 CI 会逐个平台检查编译器和 MLIR 的 Cargo 包大小。

## VS Code Marketplace

稳定版本和 Cargo 使用同一个 `vX.Y.Z` tag。先同步 Cargo workspace 与 `crates/xrack-lsp/vscode/package.json` 的版本，并确保该提交的 main CI 通过。发布工作流复用 CI 的五个平台 VSIX，通过 `vscode-marketplace` 环境和 Marketplace 的 GitHub OIDC 信任策略发布。 [Release XRack](../.github/workflows/release-crates.yml) · [Marketplace](https://marketplace.visualstudio.com/items?itemName=SunJiahe.xrack)

如果 Marketplace 环境仅允许版本标签，Cargo 补发成功后，重跑原 tag 的发布工作流。工作流会按来源校验已发布的 Cargo 包并跳过重复上传。允许 `main` 部署到 Marketplace 时，也可以从 `main` 手动运行 **Release XRack**，输入同一个稳定版本并勾选 **Publish the verified VSIX packages for an existing stable Cargo release**。补发使用原 tag 对应的 CI 产物，并检查该 tag 已成功发布 Cargo。手动运行期间，`vscode-marketplace` 环境需要允许 `main` 部署。

## PyPI

推送 `vX.Y.Z` 前，确认目标提交在 `main` 上的 **Python wheels** 工作流已经通过。稳定 tag 触发 [Python wheels](../.github/workflows/python.yml) 的发布任务，复用该提交的五个平台 wheel，校验版本和平台后通过 `pypi` 环境的 Trusted Publishing 上传。

## GitHub Actions

[CI](../.github/workflows/ci.yml) 在 macOS arm64、Linux x64 与 arm64、Windows x64 与 arm64 上测试 Cargo 桌面依赖栈与 Bazel 编译器。每个平台用暂存的发布产物运行仓库外的 AOT 和 MLIR 解释器示例，再上传归档与校验文件。Linux x64 任务额外上传通过 Cargo 验证的运行时包。真实 GPU 测试使用本地显式目标 `//tests:gpu_test`。

编译器任务通过 `setup-bazel` 缓存 Bazelisk、依赖下载和编译产物。缓存按操作系统和架构隔离。依赖下载缓存跟踪 Bazel 版本、`MODULE.bazel` 和 `Cargo.lock`，编译产物缓存额外跟踪 Bazel 构建规则。Windows 编译器任务从仓库的 Actions variables 读取上述两个 Microsoft 许可变量。
