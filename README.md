# pvctl

自用 PipeWire 音量悬浮面板。

- UI: GPUI + gpui-component
- 窗口: Wayland layer-shell，默认右上角悬浮
- 音频控制: `pactl -f json` 读取 PipeWire PulseAudio 兼容层

## 运行

```bash
cargo run
```

## 当前功能

- 显示一个输出设备，默认显示系统默认 sink
- 鼠标滚轮循环切换要查看/操作的输出设备
- 只能拖动音量圆点调节音量，点击轨道不会改变音量
- 将当前设备设为默认输出设备
- 设为默认前，如果目标设备音量高于 30%，先降到 30%
- 每 800ms 自动刷新设备状态
