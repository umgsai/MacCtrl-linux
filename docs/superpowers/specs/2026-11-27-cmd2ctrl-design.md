# cmd2ctrl 设计文档

- 日期：2026-11-27
- 目标平台：Deepin 25 / X11 / DDE
- 状态：待实现

## 1. 背景

用户在 Linux 上外接 Mac 键盘。Mac 键盘的 Command 键在 Linux 下上报为 `KEY_LEFTMETA`（Super，mod4），而用户习惯的编辑快捷键是 `Command+C/V/X/Z/S/A/F` 等。

直接把 Super 全局映射为 Ctrl 不可行：终端里的 `Ctrl+C` 是发送 `SIGINT` 中断进程，若 `Cmd+C` 也变成 `Ctrl+C`，在终端中复制就会误杀进程。因此需要一个**感知焦点窗口类型**的按键转换工具：

- 普通 GUI 窗口：`Cmd+X` → `Ctrl+X`
- 终端窗口：`Cmd+C` → `Ctrl+Shift+C`（复制）、`Cmd+V` → `Ctrl+Shift+V`（粘贴），物理 `Ctrl+C` 保持原样（中断）
- 白名单之外的 Super 组合原样交给桌面环境，不破坏 DDE 的 Super+E / Super+D / Super+方向键 / Super 启动器等快捷键

## 2. 目标与非目标

### 目标

1. 仅转换白名单组合：`Super + {C, V, X, Z, S, A, F, T, W, N, Q, Y, R}`。
2. 按当前焦点窗口是否为终端，选择不同的目标按键序列。
3. 终端内物理 `Ctrl+C` / `Ctrl+Z` / `Ctrl+S` 行为完全不受影响。
4. 只劫持指定的外接键盘，内置键盘与鼠标不受影响。
5. 守护进程退出或崩溃后键盘立即恢复原生行为。

### 非目标（v1 明确不做）

- Wayland 支持（当前会话为 X11；架构上不排除后续通过合成器扩展实现）。
- `Cmd+Tab`、`Cmd+Space` 等桌面级快捷键的 Mac 化。
- 输入法、应用内部快捷键的改写。
- 鼠标按键、媒体键、Fn 键的特殊处理（一律透传）。

## 3. 环境事实（已实测）

| 项 | 值 |
|---|---|
| 会话 | `XDG_SESSION_TYPE=x11`，`XDG_CURRENT_DESKTOP=DDE`，`DISPLAY=:0` |
| 系统 | Deepin 25（crimson），内核 6.6.159 |
| 工具链 | rustc / cargo 已安装；xdotool、setxkbmap 可用；未安装 xremap/keyd/kanata |
| Command 键 | `Super_L` = keycode 0x85 / `KEY_LEFTMETA`，属 mod4 |
| `/dev/uinput` | `root:root 0600`（需 udev 规则放开） |
| 用户组 | 当前用户不在 `input` 组 |
| 输入设备 | 当前未连接外接键盘，仅有内置 `AT Translated Set 2 keyboard` |

结论：设备不可写死路径，必须按名称/VID-PID 匹配；uinput 需要 udev 规则 + 加入 `input` 组。

## 4. 总体架构

```
物理外接键盘 (/dev/input/eventN, KEY_LEFTMETA)
        │ EVIOCGRAB 独占抓取
        ▼
┌──────────────────────────────────────────────────┐
│ cmd2ctrl 守护进程                                 │
│   device   设备发现与抓取（按名称/VID-PID 匹配）    │
│   state    延迟修饰键状态机（纯函数，可单测）        │
│   focus    X11 焦点查询：_NET_ACTIVE_WINDOW → WM_CLASS │
│   mapping  两张键表：GUI 表 / 终端表                │
│   inject   uinput 虚拟键盘注入                      │
└──────────────────────────────────────────────────┘
        │ 虚拟键盘事件
        ▼
  GUI 应用 / 终端模拟器（DDE 照旧接收未被接管的 Super 组合）
```

线程模型：单线程事件循环即可。evdev 读取阻塞等待，X11 查询为同步短请求（每次按键 <1ms），uinput 写入非阻塞。若实测焦点查询拖慢输入，再改为后台刷新焦点缓存 + 按键时读取缓存。

## 5. 核心机制：延迟修饰键状态机

### 5.1 问题

注入 `Ctrl+C` 时若物理 Super 仍被转发、处于按下状态，X 端收到的会是 `Ctrl+Super+C`，目标应用不会识别为复制。因此 Super 的按下事件不能立即转发。

### 5.2 状态与转移

状态：`Idle`、`SuperPending`（收到 Super 按下但尚未转发）、`SuperForwarded`（Super 已转发，走透传）、`Consumed`（本次 Super 组合已被转换并吞掉）。

| 当前状态 | 输入事件 | 输出 | 新状态 |
|---|---|---|---|
| Idle | Super 按下 | 无 | SuperPending |
| SuperPending | 白名单键按下，且无 Ctrl/Alt/Shift 同时按下 | 注入目标 chord（按下+释放），吞掉原键，不转发 Super | Consumed |
| SuperPending | 白名单键按下，但有其他修饰键同时按下 | 转发 Super 按下 + 原键 | SuperForwarded |
| SuperPending | 非白名单键按下 | 转发 Super 按下 + 原键 | SuperForwarded |
| SuperPending | 其他修饰键（Ctrl/Alt/Shift）按下 | 转发 Super 按下 + 该修饰键 | SuperForwarded |
| SuperPending | 重复键（Super auto-repeat） | 无 | SuperPending |
| SuperPending | Super 松开 | 注入 Super 按下+松开（启动器/修饰键语义保留） | Idle |
| SuperPending | 其他输入（鼠标事件等） | 原样转发 | SuperPending |
| SuperForwarded | 任意事件（含 Super 松开） | 原样转发 | Super 松开后 Idle |
| Consumed | 白名单键松开 / 重复键 | 无 | Consumed |
| Consumed | 其他普通键按下/松开 | 原样转发（此时 Super 未被按下，按键为无修饰原键） | Consumed |
| Consumed | Super 松开 | 无 | Idle |

补充规则：

- 白名单键的**松开事件**在 `Consumed` 状态一律吞掉，避免目标应用收到"只有松开没有按下"的异常序列。
- 注入 chord 时先注入修饰键、再注入主键、再逆序释放修饰键，每个事件后附 `SYN_REPORT`。
- 按住白名单组合不放时，不做 chord 重复注入，避免影响目标应用的按键重复逻辑。
- `Super_R`（`KEY_RIGHTMETA`）与 `Super_L` 同等处理。

### 5.3 为什么可接受

延迟仅发生在"按下 Super 后、按下一个键之前"这段人工间隔内。Super 单独按下仍会在松开时补发，启动器行为不变；Super+E 等组合在第二个键到达时一起下发，桌面环境仍能识别。

## 6. 终端识别

1. 通过 x11rb 连接 `DISPLAY`，在 root 窗口读取 `_NET_ACTIVE_WINDOW` 属性（`WINDOW` 类型）。
2. 若失败或无值，回退 `GetInputFocus`，若焦点为 `PointerRoot`/`None` 则视为非终端。
3. 读取该窗口 `WM_CLASS`，得到 `(instance, class)`，两者转小写后与终端白名单做精确匹配（非子串匹配，避免误伤）。
4. 结果（terminal / gui）缓存最近一次查询结果，但每次白名单按键按下时重新查询一次，保证焦点切换立即生效。

默认终端白名单：

```
deepin-terminal, gnome-terminal, konsole, xfce4-terminal, xterm, urxvt,
alacritty, kitty, wezterm, tilix, terminator, st, mate-terminal, lxterminal
```

**失败回退策略**：X11 连接失败或属性读取异常时，按**终端表**处理。理由：GUI 中误发 `Ctrl+Shift+C` 无副作用，而终端中误发 `Ctrl+C` 会中断用户进程，两者代价不对称。

IDE（VS Code / Cursor 等）不列入白名单，按 GUI 处理：它们内置终端中 `Ctrl+V` 本身即可粘贴，编辑器内 `Ctrl+C/V` 也能正常工作。

## 7. 键表

### 7.1 GUI 表

| 物理组合 | 注入 |
|---|---|
| Super+C / V / X / Z / S / A / F / T / W / N / Q / Y / R | Ctrl + 对应键 |

### 7.2 终端表

| 物理组合 | 注入 | 说明 |
|---|---|---|
| Super+C | Ctrl+Shift+C | 终端复制选中内容；无选中时无动作，不会中断进程 |
| Super+V | Ctrl+Shift+V | 终端粘贴 |
| Super+T | Ctrl+Shift+T | 新建标签页 |
| Super+W | Ctrl+Shift+W | 关闭标签页 |
| Super+N | Ctrl+Shift+N | 新建窗口 |
| Super+R | Ctrl+R | readline 反向搜索（安全且常用） |
| Super+Y | Ctrl+Y | readline yank（安全） |
| Super+X / Z / S / A / F / Q | 透传原始 Super 组合 | 避免 `Ctrl+Z` 挂起进程、`Ctrl+S` 冻结输出等破坏性行为 |

上述终端快捷键以 Deepin Terminal 默认键位为准。不同终端若改过键位，可通过配置文件的 `[terminal_map]` 覆盖。

## 8. 配置

路径：`~/.config/cmd2ctrl/config.toml`（不存在时使用内置默认值）。

```toml
[device]
# 设备名称匹配（不区分大小写，子串匹配），可写多个
name_patterns = ["apple", "magic keyboard", "keychron", "usb keyboard"]

[x11]
# 终端窗口 WM_CLASS 白名单（小写，精确匹配）
terminal_classes = [
  "deepin-terminal", "gnome-terminal", "konsole", "xfce4-terminal",
  "xterm", "urxvt", "alacritty", "kitty", "wezterm", "tilix",
  "terminator", "st", "mate-terminal", "lxterminal",
]

[gui_map]
c = "ctrl+c"
v = "ctrl+v"
x = "ctrl+x"
z = "ctrl+z"
s = "ctrl+s"
a = "ctrl+a"
f = "ctrl+f"
t = "ctrl+t"
w = "ctrl+w"
n = "ctrl+n"
q = "ctrl+q"
y = "ctrl+y"
r = "ctrl+r"

[terminal_map]
c = "ctrl+shift+c"
v = "ctrl+shift+v"
t = "ctrl+shift+t"
w = "ctrl+shift+w"
n = "ctrl+shift+n"
r = "ctrl+r"
y = "ctrl+y"
# 未列出的键（x/z/s/a/f/q）在终端中原样透传
```

规则：`terminal_map` 中未出现的白名单键，在终端模式下透传原始 Super 组合；`gui_map` 中未出现的白名单键，在 GUI 模式下同样透传。解析 chord 字符串时若遇到未知键名，启动即报错退出，避免静默错误。

## 9. CLI

```
cmd2ctrl [--config <path>] [--device <name-pattern>]... [--list-devices] [--dry-run] [--verbose]

  --list-devices   列出可读取的事件设备（名称、路径、VID:PID），便于确认外接键盘
  --device         覆盖配置中的设备匹配模式，可重复
  --dry-run        只输出决策（当前焦点类型、将注入的 chord），不创建 uinput、不注入
  --verbose        输出每个被处理事件的调试日志
```

## 10. 权限与部署

1. 写入 `/etc/udev/rules.d/99-uinput.rules`：

   ```
   KERNEL=="uinput", SUBSYSTEM=="misc", GROUP="input", MODE="0660"
   ```

   执行 `sudo udevadm control --reload-rules && sudo udevadm trigger --name-match=uinput`。

2. `sudo usermod -aG input "$USER"`，重新登录生效（`id -nG` 应包含 `input`）。

3. `cargo build --release`，二进制安装到 `~/.local/bin/cmd2ctrl`。

4. systemd user 服务 `~/.config/systemd/user/cmd2ctrl.service`：

   ```ini
   [Unit]
   Description=Map Mac Command key to Ctrl with terminal-aware handling
   After=graphical-session.target
   PartOf=graphical-session.target

   [Service]
   ExecStart=%h/.local/bin/cmd2ctrl
   Restart=on-failure
   RestartSec=3

   [Install]
   WantedBy=graphical-session.target
   ```

   启用：`systemctl --user daemon-reload && systemctl --user enable --now cmd2ctrl`。

5. 无法修改 udev/用户组时的降级方式：以 root 运行同一二进制（文档中注明，不作为默认路径）。

## 11. 故障处理与安全

| 场景 | 行为 |
|---|---|
| 进程正常退出 / 崩溃 | 关闭设备 fd 自动释放 EVIOCGRAB，键盘恢复原生；uinput 设备随之销毁 |
| 未匹配到外接键盘 | 记录日志，每 2 秒重试枚举；不抓取内置键盘 |
| 设备断开重连 | 读取返回错误后回到枚举循环，重新按名称匹配 |
| X11 连接失败 | 回退终端表（见第 6 节），并降频重连 |
| 配置解析错误 | 启动即失败并打印具体键名/字段，不进入抓取状态 |
| uinput 创建失败（权限） | 启动即报错，提示执行 udev 规则与用户组步骤，绝不静默继续 |

## 12. 测试策略

### 单元测试（`cargo test`）

- 状态机：第 5.2 节每一条转移一行用例（输入事件序列 → 期望输出事件序列）。
- 键表解析：`ctrl+shift+c` 等字符串解析、未知键名报错、缺失键的透传语义。
- 修饰键跟踪：多修饰键组合、重复键抑制、松开顺序。
- 终端判定：WM_CLASS 精确匹配（大小写、instance/class 两种取值）。

### 手工验证清单

1. 文本编辑器：`Cmd+C/V/X/Z/S/A/F` 均等价于 `Ctrl+*`。
2. deepin-terminal：`Cmd+C` 复制选中文本；无选中时按 `Cmd+C` 不影响 `sleep 100`；`Cmd+V` 粘贴。
3. 终端内物理 `Ctrl+C` 仍中断 `sleep 100`；`Ctrl+Z`、`Ctrl+S` 行为不变。
4. `Cmd+T/W/N` 在终端内为新建/关闭标签页、新建窗口；在浏览器中为新建/关闭标签页、新窗口。
5. `Super+E`、`Super+D`、`Super+方向键`、单独按 Super 行为与未安装时一致。
6. 按住 `Cmd+C` 不产生连续注入；快速连按无丢键。
7. 拔出键盘再由守护进程重连后功能恢复；杀掉守护进程后键盘立即可用。
8. `--dry-run` 在 GUI 与终端中输出的判定与 chord 正确。

## 13. 工程结构

```
cmd2ctrl-linux/
├── Cargo.toml
├── src/
│   ├── main.rs        # CLI 解析、组件装配、事件循环
│   ├── config.rs      # TOML 加载与默认值、chord 解析
│   ├── device.rs      # 设备枚举、名称匹配、EVIOCGRAB
│   ├── state.rs       # 延迟修饰键状态机（无 IO 依赖，纯逻辑）
│   ├── mapping.rs     # GUI/终端键表、按键名 ↔ keycode
│   ├── focus.rs       # x11rb：_NET_ACTIVE_WINDOW、WM_CLASS、终端判定
│   └── inject.rs      # uinput 虚拟键盘创建与事件注入
├── config/config.toml # 示例配置
├── packaging/
│   ├── 99-uinput.rules
│   └── cmd2ctrl.service
└── docs/superpowers/specs/2026-11-27-cmd2ctrl-design.md
```

依赖：`evdev`（含 uinput 支持）、`x11rb`、`clap`、`serde`、`toml`、`regex`、`anyhow`、`log` + `env_logger`。

## 14. 风险与已决策项

| 风险 | 应对 |
|---|---|
| 终端默认键位与 Deepin Terminal 实际配置不一致 | `terminal_map` 可覆盖；实现后按手工清单第 2、4 项核对一次 |
| 焦点查询延迟影响输入手感 | 先直接查询；若不达标改为缓存 + 后台刷新 |
| 多个外接键盘同时匹配 | 全部抓取，行为一致；`--list-devices` 帮助确认 |
| 蓝牙键盘路径变化 | 按名称匹配而非路径；重连循环为 2 秒 |
| 与 DDE 全局快捷键冲突 | 白名单仅覆盖编辑/应用类组合，桌面组合不接管；上线后按清单第 5 项验证 |
| Wayland 会话 | v1 不支持，启动时检测 `XDG_SESSION_TYPE` 并给出明确提示后退出 |
