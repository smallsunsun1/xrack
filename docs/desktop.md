# 桌面宿主与异步

[返回项目首页](../README.md)

## 异步 Rust

- `xrack::invoke_from_event_loop(move || { … })` 可以从任意线程投递一次 `Send + 'static` 闭包。桌面事件循环在 UI 线程执行它。投递成功表示入队，队列满或事件循环关闭时返回错误。
- `ui.as_weak().upgrade_in_event_loop(move |ui| { … })` 适合更新指定组件。弱句柄可发送到后台，组件本身仍留在 UI 线程。如果执行时组件已销毁，闭包会跳过。
- `xrack::spawn_local(async move { … })` 在 UI 线程轮询 Future，支持捕获 `Rc` 等 `!Send` 状态。可以在 UI 回调、投递到事件循环的闭包或其他本地任务中调用。返回的 `LocalTask<T>` 可以等待或用 `abort()` 取消，丢弃句柄会让任务继续运行。从窗口回调启动的本地任务会在该窗口关闭时取消。

先注册组件的 `on_*` 回调，再通过普通的 `run(ui)` 或 `Desktop::open(ui)` 启动界面。`bazel run //examples/async_demo:async_demo` 使用多线程 Tokio runtime 运行后台搜索，再用 `spawn_local` 等待 Tokio 的 `JoinHandle`。请求完成后，UI Future 核对请求 ID，再更新结果列表、候选和单行数据。也可以在后台拿到结果后直接调用弱句柄的 `upgrade_in_event_loop`。网络请求由应用的 Tokio runtime 驱动。

## 多窗口

`Window` 可绑定 `no-frame: true;` 隐藏系统标题栏。`WindowMoveArea` 可包住自定义标题栏，空白位置拖过 4 个逻辑像素后开始系统移动，里面的按钮和其他交互控件仍处理各自的输入。设置 `enabled: false;` 只停用该区域的窗口拖动。打开菜单、下拉框或对话框时会暂停窗口手势。

无边框窗口可用 `resize-border-width: 8px;` 在四边和四角启用缩放命中区。该区域避开可交互控件，`resizable: false;` 关闭缩放。窗口的 `min-width`、`min-height`、`max-width` 和 `max-height` 长度会传给桌面宿主。Windows 和 Linux 使用 winit 的系统边缘缩放，macOS 通过 AppKit 更新窗口边框并保留对侧边缘。运行 `bazel run //examples/layout:layout -- --frameless` 可体验。

回调可以调用 `root.minimize()`、`root.maximize()`、`root.restore()`、`root.toggle-maximized()`、`root.set-full-screen(bool)` 和 `root.toggle-full-screen()`。`restore()` 请求恢复普通窗口。窗口的 `window-active`、`minimized`、`maximized` 和 `full-screen` 是只读的响应式状态，由宿主从原生窗口采样。`minimized-known` 表示当前后端能否报告最小化，例如 Wayland 上该值为 `false`。同一批切换请求按发出顺序解析，退出全屏后需要调整最大化状态时，宿主会等待退出确认。自定义标题栏可用这些属性更新按钮文字和无障碍名称。

`xrack_platform_winit::run_desktop` 在同一个 winit 事件循环中运行多个顶层组件。通过 `Desktop::open` 可在启动时或任意窗口的 UI 回调中创建新窗口。`DesktopWindow::close` 关闭指定窗口。所有窗口关闭、所有托盘图标隐藏或移除后，事件循环退出。

顶层 `.xr` `Window` 可实现 `close-requested() => { … }`，返回 `true` 允许关闭，返回 `false` 保留窗口。标题栏、Rust 的 `DesktopWindow::close()` 和 `.xr` 的 `root.close()` 都经过这个处理器。`root.close()` 在当前回调结束后提交关闭请求。处理器可先拒绝，再打开 `MessageDialog`，由确认回调清除未保存标记并重新请求关闭。运行 `bazel run //examples/form:form -- --close-guard` 体验。

在根 `Window` 下声明命名的 `SecondaryWindow`，并在其中放一个继承 `Window` 的组件实例。`visible <=> root.show-inspector;` 把显示状态和父组件同步，`show()`、`hide()` 控制显示，`close()` 提交子窗口的关闭请求。子窗口批准关闭后会将 `visible` 写回 `false`。隐藏会释放原生窗口并保留子组件实例，重新显示时可继续编辑。父窗口退出时，宿主先释放它拥有的辅助窗口。运行 `bazel run //examples/counter:counter -- --secondary` 体验。

每个窗口维护自己的焦点、编辑状态、计时器、原生对话框和本地任务。共享的响应式模型变化后，宿主在同一轮事件中刷新依赖失效的窗口，并合并各窗口的唤醒时间。兼容的 surface 复用 GPU 设备。运行 `bazel run //examples/counter:counter -- --windows` 可验证独立计数、共享状态、异步更新、动态开窗和关闭。

### 系统托盘

Linux、macOS 和 Windows 桌面宿主可用 `Desktop::register_tray` 注册继承 `SystemTrayIcon` 的顶层组件。托盘应用可以独立运行，也可从菜单打开窗口。`icon` 接收 PNG 或 SVG 图像，`title` 和 `tooltip` 支持响应式绑定。`visible: false` 隐藏图标，组件和本地任务仍保留，`DesktopTray::close()` 释放它。最后一个窗口和托盘都关闭后，桌面事件循环退出。

托盘可声明一个静态 `Menu`，菜单内容支持子菜单、分隔线、动态可见性、勾选状态和共享 `Action`。在 macOS 上，`icon-template` 启用 AppKit 模板图标，带菜单的图标打开原生菜单，纯图标可处理 `clicked()`。在 Windows 上，左键激活触发 `clicked()`，右键或键盘菜单键打开原生菜单，空的 `tooltip` 使用 `title`，Explorer 重启后会重新注册图标。

Linux 使用会话 D-Bus 上的 StatusNotifierItem 和 DBusMenu，需要支持该协议的面板或桌面扩展。面板请求激活时触发 `clicked()`，菜单由面板呈现。面板稍后启动或重启时，应用会重新注册图标。D-Bus 回调通过事件队列交给 UI 线程，面板缓存的旧菜单命令会在菜单更新后失效。

托盘回调可通过 `spawn_local` 调度 UI 线程任务。运行 `bazel run //examples/tray:tray`，在系统托盘打开 **XRack** 菜单。Linux 可用 `dbus-run-session -- bazel run //crates/xrack-platform-winit:async_test -- status_notifier_round_trip --ignored` 在独立会话中运行协议测试。
