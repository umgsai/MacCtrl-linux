# MacCtrl

在 Linux（X11）上使用 Mac 键盘时，把 **Command** 键组合映射为 **Ctrl** 键组合的托盘常驻工具。

- **终端安全**：在终端窗口里 `Cmd+C` 映射为 `Ctrl+Shift+C`，不会误杀进程；没有安全替代的组合直接透传原始 Super 组合
- **所有键盘统一接管**：内置键盘、USB / 蓝牙外接键盘同时存在时全部生效，也可按设备名筛选
- **托盘 + 应用入口**：托盘菜单提供暂停/继续、重载配置、查看绑定、打开配置、退出；桌面应用列表里点击图标即可启动，已在运行时不会重复启动
- **可逐项开关**：内置 60 项快捷键目录，35 项默认开启、25 项默认关闭（避免遮蔽桌面快捷键），配置文件可单独覆盖
- **退出即释放**：进程退出或崩溃时自动释放全部键盘抓取，不留卡键

环境要求：仅支持 X11（`$XDG_SESSION_TYPE=x11`），需要 `input` 组权限与可写的 `/dev/uinput`；Wayland 不支持。

![版本](https://img.shields.io/badge/version-1.0.1-brightgreen) ![平台](https://img.shields.io/badge/platform-linux%20%7C%20x11-lightgrey) ![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

## 快速安装

### Debian / deepin / Ubuntu

```bash
# 加用户到 input 组 + 重载 uinput 规则（安装包自动提供规则文件）
sudo usermod -aG input "$USER"
sudo udevadm control --reload-rules && sudo udevadm trigger

# 安装 deb
sudo dpkg -i macctrl_1.0.1-1_amd64.deb

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
# 窗口标题关键字：Java 系终端（FinalShell 等）的 WM_CLASS 是通用的 java-lang-Thread，
# 只能靠标题子串识别；默认已内置 "finalshell"
terminal_titles = ["finalshell", "xshell"]

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
  macctrl.desktop      # 桌面应用入口（点击启动服务）
  99-uinput.rules       # udev 权限规则
  icons/macctrl.svg    # 矢量图标源（由 32px 原图几何重绘）
  icons/macctrl-*.png  # 16/22/24/48/64/128/256 各尺寸导出

config/
  config.toml           # 示例配置
```

## License

MIT OR Apache-2.0
