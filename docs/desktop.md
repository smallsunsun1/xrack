# 桌面宿主与异步

[返回项目首页](../README.md)

## 异步 Rust

- `xrack::invoke_from_event_loop(move || { … })` 可以从任意线程投递一次 `Send + 'static` 闭包。桌面事件循环在 UI 线程执行它。投递成功表示入队，队列满或事件循环关闭时返回错误。
- `ui.as_weak().upgrade_in_event_loop(move |ui| { … })` 适合更新指定组件。弱句柄可发送到后台，组件本身仍留在 UI 线程。如果执行时组件已销毁，闭包会跳过。
- `xrack::spawn_local(async move { … })` 在 UI 线程轮询 Future，支持捕获 `Rc` 等 `!Send` 状态。可以在 UI 回调、投递到事件循环的闭包或其他本地任务中调用。返回的 `LocalTask<T>` 可以等待或用 `abort()` 取消，丢弃句柄会让任务继续运行。从窗口回调启动的本地任务会在该窗口关闭时取消。

先注册组件的 `on_*` 回调，再通过普通的 `run(ui)` 或 `Desktop::open(ui)` 启动界面。`bazel run //examples/async_demo:async_demo` 使用多线程 Tokio runtime 运行后台搜索，再用 `spawn_local` 等待 Tokio 的 `JoinHandle`。请求完成后，UI Future 核对请求 ID，再更新结果列表、候选和单行数据。也可以在后台拿到结果后直接调用弱句柄的 `upgrade_in_event_loop`。网络请求由应用的 Tokio runtime 驱动。

## 桌面语音

启用 Cargo 的 `media` feature 后，桌面 Rust 应用可通过 `xrack::voice` 接入麦克风和语音输出。Bazel 使用 `--config=media`。采集调用原生 FFmpeg `libavdevice`，每帧提供 10 毫秒的 48 kHz 单声道 PCM。应用负责连接通话传输，收到远端帧后把它们送到播放队列。

```rust
use xrack::voice::{VoiceCapture, VoicePlayback};

let microphone = VoiceCapture::start(None)?;
let playback = VoicePlayback::open()?;
let remote_audio = playback.sender();
```

`VoiceCapture::start` 会在工作线程打开设备。用 `status()` 查看设备状态，用 `try_frame()` 取帧或在后台用 `wait_frame(timeout)` 等待。Tokio 应用可以在 `spawn_blocking` 里取帧，通过有界 channel 交给异步网络任务。`VoiceFrame::sequence` 可用于发现积压时被丢弃的帧。停止通话时调用 `stop()`，或者释放采集句柄。如果要在重开设备前确认上一次采集已退出，可在后台用 `wait_stopped(timeout)` 等待。保持 `VoicePlayback` 存活，把可克隆的 `remote_audio` 交给收包线程，调用 `push(&frame)` 播放。采集和播放队列各保留最多 12 帧。

macOS 使用 AVFoundation，`None` 选择系统默认麦克风，显式名称可用设备名或序号。Windows 使用 DirectShow，自动选择可用音频输入，也可传入音频设备名。Linux 使用 ALSA，支持例如 `default` 或 `hw:0,0`。macOS 应用包的 `Info.plist` 需要 `NSMicrophoneUsageDescription`，启用 App Sandbox 时还需音频输入 entitlement。 [Apple 麦克风权限](https://developer.apple.com/documentation/bundleresources/information-property-list/nsmicrophoneusagedescription)

通话连接、信令、编解码和回声消除可以放在独立的业务层。通过组件弱句柄更新静音状态和电平等显示数据，PCM 在后台传递。

用 [桌面语音示例](../examples/voice/main.rs) 检查本机麦克风。默认采集 3 秒，只在终端输出帧数、丢帧数和峰值电平。可用 `--device` 选设备，用 `--seconds` 调整时长。戴上耳机后可加 `--monitor`，把采集到的声音送到本地输出设备。

```sh
bazel run --config=media //examples/voice:voice
```

## 多窗口

`Window` 可绑定 `no-frame: true;` 隐藏系统标题栏。`WindowMoveArea` 可包住自定义标题栏，空白位置拖过 4 个逻辑像素后开始系统移动，里面的按钮和其他交互控件仍处理各自的输入。设置 `enabled: false;` 只停用该区域的窗口拖动。打开菜单、下拉框或对话框时会暂停窗口手势。

无边框窗口可用 `resize-border-width: 8px;` 在四边和四角启用缩放命中区。该区域避开可交互控件，`resizable: false;` 关闭缩放。窗口的 `min-width`、`min-height`、`max-width` 和 `max-height` 长度会传给桌面宿主。Windows 和 Linux 使用 winit 的系统边缘缩放，macOS 通过 AppKit 更新窗口边框并保留对侧边缘。运行 `bazel run //examples/layout:layout -- --frameless` 可体验。

回调可以调用 `root.minimize()`、`root.maximize()`、`root.restore()`、`root.toggle-maximized()`、`root.set-full-screen(bool)` 和 `root.toggle-full-screen()`。`restore()` 请求恢复普通窗口。窗口的 `window-active`、`minimized`、`maximized` 和 `full-screen` 是只读的响应式状态，由宿主从原生窗口采样。`minimized-known` 表示当前后端能否报告最小化，例如 Wayland 上该值为 `false`。同一批切换请求按发出顺序解析，退出全屏后需要调整最大化状态时，宿主会等待退出确认。自定义标题栏可用这些属性更新按钮文字和无障碍名称。

`xrack_platform_winit::run_desktop` 在同一个 winit 事件循环中运行多个顶层组件。通过 `Desktop::open` 可在启动时或任意窗口的 UI 回调中创建新窗口。`DesktopWindow::close` 关闭指定窗口。所有窗口关闭、所有托盘图标隐藏或移除后，事件循环退出。

顶层 `.xr` `Window` 可实现 `close-requested() => { … }`，返回 `true` 允许关闭，返回 `false` 保留窗口。标题栏、Rust 的 `DesktopWindow::close()` 和 `.xr` 的 `root.close()` 都经过这个处理器。`root.close()` 在当前回调结束后提交关闭请求。处理器可先拒绝，再打开 `MessageDialog`，由确认回调清除未保存标记并重新请求关闭。运行 `bazel run //examples/form:form -- --close-guard` 体验。

在根 `Window` 下声明命名的 `SecondaryWindow`，并在其中放一个继承 `Window` 的组件实例。`visible <=> root.show-inspector;` 把显示状态和父组件同步，`show()`、`hide()` 控制显示，`close()` 提交子窗口的关闭请求。子窗口批准关闭后会将 `visible` 写回 `false`。隐藏会释放原生窗口并保留子组件实例，重新显示时可继续编辑。父窗口退出时，宿主先释放它拥有的辅助窗口。运行 `bazel run //examples/counter:counter -- --secondary` 体验。

每个窗口维护自己的焦点、编辑状态、计时器、原生对话框和本地任务。共享的响应式模型变化后，宿主在同一轮事件中刷新依赖失效的窗口，并合并各窗口的唤醒时间。兼容的 surface 复用 GPU 设备。运行 `bazel run //examples/counter:counter -- --windows` 可验证独立计数、共享状态、异步更新、动态开窗和关闭。

### iPad 指针输入

iPadOS 上，UIKit 将鼠标和触控板悬停送入 `HoverHandler`，滚轮和双指滚动使用 XRack 的 `WheelHandler` 与视口滚动路径。鼠标右键、触控板次要点击和 Control-click 可打开 `ContextMenuArea`。手指也可以长按打开菜单或滑动列表。[Pointer inspection 示例](../examples/ios/README.md#ipad-鼠标与触控板)

### iOS 文件与相册

iOS 上，同一 XRack 窗口的 `.xr` 原生对话框与 Rust 发起的 Files、相册和分享面板一起排队。排队期间取消 Rust 请求会跳过对应面板。面板已经打开时，取消 Rust 请求会关闭它，UIKit 完成关闭后才展示下一个。文件导出在开始复制前取消时不会写入目标目录。已开始的文件复制和照片导入在后台处理。

发起 `.xr` 对话框的组件离开活动场景时，iOS 宿主会关闭它已打开的系统面板。该请求的结果会被丢弃，其余有效请求继续排队。

`DesktopWindow::select_color(initial, show_alpha).await` 会排队打开 iOS 系统色板，返回用户关闭面板时的最终 `Color`。隐藏透明度控件时，返回值保留初始 alpha。色板输出转换为 XRack 使用的 8 位 sRGB RGBA，超出 sRGB 范围的通道会裁剪。丢弃 Future 会取消请求。

`DesktopWindow::select_color_with_preview(initial, show_alpha, on_preview).await` 也会把编辑中的颜色交给回调。相邻的快速更新可能合并，回调由等待 Future 的任务执行。可以在 `spawn_local` 中更新临时色块，等 Future 返回后再提交最终颜色。

`DesktopWindow::select_directory().await` 打开 Files 并返回用户授权的 `IosDirectory`，取消时返回 `Ok(None)`。在工作线程用 `directory.with_read_access(relative_path, callback)` 读取目录或文件，传空路径可枚举目录。访问在回调结束时释放。

`directory.bookmark_data()` 可以生成由应用保存在私有存储中的系统书签。应用下次启动后，在工作线程用 `IosDirectory::from_bookmark(&bytes)` 恢复目录，再协调读取。书签失效、目录不可用或用户撤销授权时会返回错误，应用可以再次打开 Files 请求选择。

`DesktopWindow::select_photos(limit).await` 打开 iOS 系统相册，`limit` 是允许选择的最大数量，必须大于零。成功选择得到 `IosPhotoSelection`，取消得到 `Ok(None)`。选择器只交付用户选中的图片。用 `selection.paths()` 获取按选择顺序排列的应用本地副本，保持 `selection` 存活即可继续使用这些路径。

`selection.thumbnail(index, maximum_edge)` 可在后台线程用 ImageIO 解码预览，再把结果交回 UI 线程。调用缩略图的 `into_image()` 得到可写入 `.xr` 图像属性的 `Image`。Bazel 构建的 iOS 应用应链接 `PhotosUI` 和 `ImageIO`，[Pocket Tasks](../examples/ios/README.md#系统相册选图) 演示了完整的调用。

`DesktopWindow::export_files(paths).await` 可以一次将多个本地文件复制到用户在 Files 中选定的目录。路径列表必须非空，所有路径都必须指向已有的绝对路径文件。调用方需保留这些文件直到等待结束，例如直接传入 `selection.paths()` 并保留选择对象。所有副本写入后返回 `Ok(true)`，取消选目录时返回 `Ok(false)`。遇到同名文件，副本会增加数字后缀。写入错误通过 `Err` 返回。

`DesktopWindow::share_files(paths).await` 将一组已有的本地文件交给同一个 iOS 分享面板。和 Files 导出一样，调用方需保留源文件直到等待结束。系统活动完成时返回 `Ok(true)`，用户关闭面板或取消活动时返回 `Ok(false)`。

### 系统托盘

Linux、macOS 和 Windows 桌面宿主可用 `Desktop::register_tray` 注册继承 `SystemTrayIcon` 的顶层组件。托盘应用可以独立运行，也可从菜单打开窗口。`icon` 接收 PNG 或 SVG 图像，`title` 和 `tooltip` 支持响应式绑定。`visible: false` 隐藏图标，组件和本地任务仍保留，`DesktopTray::close()` 释放它。最后一个窗口和托盘都关闭后，桌面事件循环退出。

托盘可声明一个静态 `Menu`，菜单内容支持子菜单、分隔线、动态可见性、勾选状态和共享 `Action`。在 macOS 上，`icon-template` 启用 AppKit 模板图标，带菜单的图标打开原生菜单，纯图标可处理 `clicked()`。在 Windows 上，左键激活触发 `clicked()`，右键或键盘菜单键打开原生菜单，空的 `tooltip` 使用 `title`，Explorer 重启后会重新注册图标。

Linux 使用会话 D-Bus 上的 StatusNotifierItem 和 DBusMenu，需要支持该协议的面板或桌面扩展。面板请求激活时触发 `clicked()`，菜单由面板呈现。面板稍后启动或重启时，应用会重新注册图标。D-Bus 回调通过事件队列交给 UI 线程，面板缓存的旧菜单命令会在菜单更新后失效。

托盘回调可通过 `spawn_local` 调度 UI 线程任务。运行 `bazel run //examples/tray:tray`，在系统托盘打开 **XRack** 菜单。Linux 可用 `dbus-run-session -- bazel run //crates/xrack-platform-winit:async_test -- status_notifier_round_trip --ignored` 在独立会话中运行协议测试。
