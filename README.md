# MacCtrl

把外接 Mac 键盘上的 **Command** 键组合映射为 **Ctrl** 组合，专为 Linux（X11）桌面设计。

- **终端感知**：在终端窗口里，`Ctrl+C` 不会杀进程；默认透传 Super 组合，或用安全的 `Ctrl+Shift+C` 替代
- **多键盘统一处理**：内置、USB、蓝牙键盘同时存在时全部接管
- **系统托盘**：暂停/继续、重载配置、列出绑定、退出，托盘菜单直接控制
- **零后台进程**：每个键盘一个设备线程，`Ctrl+C` 进程退出自动释放所有抓取

![状态](https://img.shields.io/badge/status-alpha-orange) ![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

## 快速安装

### Debian / deepin / Ubuntu

```bash
# 加用户到 input 组 + 重载 uinput 规则（安装包自动提供规则文件）
sudo usermod -aG input "$USER"
sudo udevadm control --reload-rules && sudo udevadm trigger

# 安装 deb
sudo dpkg -i macctrl_0.1.0-1_amd64.deb

# 重新登录后启用服务
systemctl --user enable --now macctrl
```

## 卸载

```bash
# 停掉用户服务
systemctl --user disable --now macctrl

# 卸载 deb
sudo dpkg -r macctrl

# （可选）删除配置文件
rm -rf ~/.config/macctrl

# （可选）从 input 组移除自己
gpasswd -d "$USER" input
```

## 构建

```bash
# 系统依赖（GTK 3 用于托盘）
sudo apt install -y libgtk-3-dev libayatana-appindicator3-dev libxdo-dev librsvg2-bin

# 构建
cargo build --release

# 打包 deb
cargo deb
```

## 配置

默认路径 `~/.config/macctrl/config.toml`。

```toml
# 开启默认关闭的 cmd-d
[bindings.cmd-d]
enabled = true

# 禁用默认开启的 cmd-b
[bindings.cmd-b]
enabled = false

# 自定义绑定（组合 + 目标）
[bindings.indent]
keys = "shift+]"
gui = "ctrl+shift+]"
terminal = ""

# 自定义终端窗口类名（默认已覆盖常见终端）
[x11]
terminal_classes = ["myterm"]

# 只处理名称包含 Apple 的键盘
[device]
include_patterns = ["Apple"]
exclude_patterns = ["VMware"]
```

## 快捷键目录（内置 60 项）

```
cmd-c      → Ctrl+C  /  Ctrl+Shift+C (终端)
cmd-v      → Ctrl+V  /  Ctrl+Shift+V (终端)
cmd-x      → Ctrl+X
cmd-z      → Ctrl+Z
cmd-a      → Ctrl+A
...
```

默认 **35 项开启**（高频组合），**25 项关闭**（遮挡桌面快捷键）。完整列表：

```bash
macctrl --list-bindings
```

## CLI

```
macctrl [OPTIONS]

选项:
      --config <PATH>       配置文件路径
      --device <NAME>       只处理名称匹配的键盘（可重复）
      --list-devices        列出事件设备
      --list-bindings       列出快捷键目录
      --enable <ID>         本次运行强制启用绑定（可重复）
      --disable <ID>        本次运行强制关闭绑定（可重复）
      --dry-run             只观察不注入（调试用）
      --no-tray             禁用托盘图标（headless 环境）
      --verbose             输出 debug 日志
  -h, --help                帮助
  -V, --version             版本
```

## 工作原理

```
键盘事件 → evdev grab → 状态机（延迟 Super 转发）
                          ├── 命中已启用绑定 → 注入目标 chord
                          └── 未命中 → 透传 Super + 原按键
                                          ↑
                        X11 焦点探针判定当前窗口模式
                        （Gui 还是 Terminal）
```

- **Super 键延迟转发**：按下后不立即发送，等普通键按下时才决定（命中 → 注入目标；未命中 → flush 补出 Super 点击）
- **跨设备组合键不支持**：每块键盘独立状态机（一块按 Super、另一块按 C 不会触发映射）
- **Wayland 不支持**：`$XDG_SESSION_TYPE` 必须是 `x11`

## 目录结构

```
src/
  main.rs      # CLI + 入口
  runner.rs    # 设备事件循环 + 托盘控制通道
  tray.rs      # GTK 托盘 + 菜单
  state.rs     # 按键状态机
  binding.rs   # 快捷键目录 + 绑定表
  config.rs    # TOML 配置解析
  focus.rs     # X11 焦点探针（识别终端窗口）
  inject.rs    # uinput 虚拟设备注入
  device.rs    # 键盘筛选
  key.rs       # Linux 键码映射

packaging/
  macctrl.service      # systemd 用户级服务
  macctrl.desktop      # 桌面自启
  99-uinput.rules       # udev 权限规则
  icons/macctrl.png

config/
  config.toml           # 示例配置
```

## License

MIT OR Apache-2.0
