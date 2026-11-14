# 发布与 CI

[返回项目首页](../README.md)

## 准备发布包

`python3 tools/package_xrackc.py` 用 Bazel 构建当前宿主平台的 `xrackc` 和原生 MLIR 静态库，在 `.tools/dist/` 生成压缩包和 SHA-256 校验文件。压缩包也携带生成的 Rust 绑定、ANTLR 解析器源码和许可证。编译器和 MLIR 的 Cargo 包暂存模板分别在 `bundles/compiler` 和 `bundles/mlir`。

```sh
cargo package --locked -p xrack-runtime -p xrack-assets -p xrack-layout \
  -p xrack-renderer-femtovg -p xrack-platform-winit
```

Cargo 会验证打包后的源码，并在发布时按依赖顺序上传这些 crate。Cargo 版桌面宿主使用上游 AccessKit，Bazel 版带有 macOS 三态复选框无障碍值修补。

手动运行 [Release XRack](../.github/workflows/release-crates.yml) 并输入 workspace 版本号，可以预检运行时包。正式发布时，给 `main` 上已合并的提交推送与 workspace 版本一致的 tag。发布流程跑各平台 CI，并从五份 Bazel 归档生成宿主编译器包、目标平台 MLIR 包和解析器包。发布顺序为运行时和语法基础包、Bazel 产物对应的包，再到 `xrack-interpreter`、`xrack` 和 `xrack-build` 等上层包。GitHub Release 提供归档及校验文件。

在 GitHub 的 `crates-io` 环境配置 `CARGO_REGISTRY_TOKEN` secret，并允许发布 tag 使用该环境。首次发布使用允许发布新 crate 的 token。上传中断后，可从同一次 workflow 重试，脚本会校验已发布的版本及对应的源码提交。升版本时同步 workspace 版本、内部 crate 依赖、宿主包版本、Bazel 目标版本和 Cargo 消费示例。

如果已有 tag 的流水线需要使用更新后的 CI 配置，在 `main` 上手动运行 **Release XRack**，输入版本并勾选 **Publish an existing release tag from main**。`crates-io` 环境需允许 `main` 发布。工作流会从已有 tag 对应的提交重新构建并验证所有平台的产物。

## VS Code Marketplace

稳定版本和 Cargo 使用同一个 `vX.Y.Z` tag。先同步 Cargo workspace 与 `crates/xrack-lsp/vscode/package.json` 的版本，并确保该提交的 main CI 通过。发布工作流复用 CI 的五个平台 VSIX，通过 `vscode-marketplace` 环境和 Marketplace 的 GitHub OIDC 信任策略发布。 [Release XRack](../.github/workflows/release-crates.yml) · [Marketplace](https://marketplace.visualstudio.com/items?itemName=SunJiahe.xrack)

## GitHub Actions

[CI](../.github/workflows/ci.yml) 在 macOS arm64、Linux x64 与 arm64、Windows x64 与 arm64 上测试 Cargo 桌面依赖栈与 Bazel 编译器。每个平台用暂存的发布产物运行仓库外的 AOT 和 MLIR 解释器示例，再上传归档与校验文件。Linux x64 任务额外上传通过 Cargo 验证的运行时包。真实 GPU 测试使用本地显式目标 `//tests:gpu_test`。

编译器任务通过 `setup-bazel` 缓存 Bazelisk、依赖下载和编译产物。缓存按操作系统和架构隔离。依赖下载缓存跟踪 Bazel 版本、`MODULE.bazel` 和 `Cargo.lock`，编译产物缓存额外跟踪 Bazel 构建规则。Windows 编译器任务从仓库的 Actions variables 读取上述两个 Microsoft 许可变量。
