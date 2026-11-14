# 本地 HLS 直播

从仓库根目录运行。示例用 Python 3 启动本地 HTTP 服务，用带 `libx264` 编码器的 FFmpeg 生成两档合成测试画面。播放器使用 XRack 自带的媒体库。

```sh
python3 examples/video/live_server.py
```

在 VS Code 打开 [live.xr](live.xr)，运行 **XRack: Open Live Preview**。也可以在另一个终端运行原生窗口。

```sh
bazel run --config=media //examples/video:live
```

1. 直播会自动播放，默认静音。进度条下方显示当前码率、播放位置和回看范围。
2. 把进度条拖到左侧并松开，观察画面回到较早位置。点击 **Go live** 回到直播。
3. 等码率从 120 kbps 升到 480 kbps，再点预览里的 **模拟弱网**。播放器会按 180 kbps 向播放缓冲供给 HLS 数据，并自动降到 120 kbps。点 **恢复正常** 后，缓冲稳定时会再次升档。两次切换都使用同一条直播流。

回看窗口最多保留 12 个一秒分片。服务只监听 `127.0.0.1:8785`。前台运行时用 Ctrl+C 结束，临时分片会自动清理。
