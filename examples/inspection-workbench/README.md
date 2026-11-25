# 巡检工作台

用 XRack 做一个含数据、业务规则和文件生命周期的独立 Cargo 应用。初次打开会生成 10,000 条巡检记录，可直接搜索编号 `INSP-00003`，点击一行后用「编辑选中项」打开详情。点击表头可以排序，拖动分隔条可调整工作区和详情宽度。

- 按处理状态和编号、设备、站点或负责人组合筛选。表格只实例化视口附近的行，详情通过记录编号关联数据。排序和筛选期间可继续编辑同一条记录。
- 编辑负责人、状态和备注。标记完成前需要复检备注。应用修改后更新一条模型记录、四个仪表数字和最近变更，并保留审计记录。
- 「保存项目」或「另存为」生成可重新打开的 JSON。文件读写和导入校验在 Tokio 线程执行，结果回到 XRack UI 线程。后台保存期间可以继续编辑，应用用修订号记录真正保存到磁盘的快照。
- 有详情草稿或项目修改时，关闭窗口会询问是否放弃。打开另一份项目之前，先应用或撤销当前草稿并保存修改。

界面、分栏、属性绑定和交互写在 [workbench.xr](ui/workbench.xr)。[project.rs](src/project.rs) 定义项目格式与校验，[controller.rs](src/controller.rs) 管理模型更新和文件操作状态。

## 构建与验收

从仓库根目录用预编译 `xrackc` 归档暂存 SDK，测试脚本将应用复制到独立目录并执行 Cargo release 测试和构建。

```sh
export CARGO_TARGET_DIR="$PWD/.tools/cargo-build"
python3 tools/test_cargo_consumer.py .tools/dist aarch64-apple-darwin --inspection-workbench
```

`xrack`、`xrack-build` 和 `xrack-testing` 的对应版本发布后，也可在本目录直接运行 `cargo test --release` 和 `cargo run --release`。设置 `XRACK_WORKBENCH_SNAPSHOT` 为 PNG 的绝对路径可额外跑一次原生 GPU 离屏绘制，生成已经选中记录的工作台图。Windows、Linux 和 macOS 的常规测试无需图形桌面。

在 macOS 可用以下命令打包。输出目录应尚未存在，签名用于本地验收。

```sh
sh tools/bundle-macos-app.sh \
  .tools/cargo-build/release/xrack-inspection-workbench \
  ".tools/inspection-workbench/Inspection Workbench.app" dev.xrack.inspections 0.9.0
open ".tools/inspection-workbench/Inspection Workbench.app"
```
