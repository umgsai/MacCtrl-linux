# cmd2ctrl 设计文档

- 日期：2026-11-27
- 目标平台：Linux / X11（不限发行版与桌面环境；Deepin 25 + DDE 仅为当前验证环境）
- 状态：待实现

## 1. 背景

用户在 Linux 上外接 Mac 键盘。Mac 键盘的 Command 键在 Linux 下上报为 `KEY_LEFTMETA`（Super，mod4），而用户习惯的编辑快捷键是 `Command+C/V/X/Z/S/A/F` 等。

直接把 Super 全局映射为 Ctrl 不可行：终端里的 `Ctrl+C` 是发送 `SIGINT` 中断进程，若 `Cmd+C` 也变成 `Ctrl+C`，在终端中复制就会误杀进程。因此需要一个**感知焦点窗口类型**的按键转换工具：

- 普通 GUI 窗口：`Cmd+X` → `Ctrl+X`
- 终端窗口：`Cmd+C` → `Ctrl+Shift+C`（复制）、`Cmd+V` → `Ctrl+Shift+V`（粘贴），物理 `Ctrl+C` 保持原样（中断）
- 白名单之外的 Super 组合原样交给桌面环境，不破坏 DDE / GNOME / KDE / XFCE 等桌面自身的 Super+E / Super+D / Super+方向键 / Super 启动器等快捷键

## 2. 目标与非目标

### 目标

1. 仅转换白名单组合：`Super + {C, V, X, Z, S, A, F, T, W, N, Q, Y, R}`。
2. 按当前焦点窗口是否为终端，选择不同的目标按键序列。
3. 终端内物理 `Ctrl+C` / `Ctrl+Z` / `Ctrl+S` 行为完全不受影响。
4. **不区分键盘来源**：所有物理键盘（内置键盘、USB/蓝牙外接键盘、多键盘同时连接）行为一致；鼠标、触摸板、电源键、仅媒体键设备不受影响。
5. 守护进程退出或崩溃后键盘立即恢复原生行为。

### 非目标（v1 明确不做）

- Wayland 支持（当前会话为 X11；架构上不排除后续通过合成器扩展实现）。
- `Cmd+Tab`、`Cmd+Space` 等桌面级快捷键的 Mac 化。
- 输入法、应用内部快捷键的改写。
- 鼠标按键、媒体键、Fn 键的特殊处理（一律透传）。

### 适用范围与环境要求

实现只依赖 Linux 通用机制，不对任何发行版或桌面环境做硬绑定：

- **输入层**：`evdev`（`/dev/input/event*`）与 `uinput`，属于内核通用接口。
- **焦点层**：X11 + EWMH（`_NET_ACTIVE_WINDOW`、`WM_CLASS`）；非 EWMH 的窗口管理器通过 `GetInputFocus` 回退，仍可工作。
- **桌面环境**：DDE、GNOME（Xorg 会话）、KDE Plasma（Xorg 会话）、XFCE、Cinnamon、MATE、LXDE 等均可，不依赖 DDE 专有 D-Bus 接口或配置路径。
- **终端**：按 `WM_CLASS` 识别，不依赖具体终端的私有 IPC；终端键位差异通过配置覆盖。

发行版差异只体现在**安装步骤**（udev 规则、用户组、systemd/自启动方式），由第 10 节给出通用做法与常见变体，代码中不出现发行版判断。

## 3. 当前验证环境（仅作参考，并非限定）

| 项 | 值 |
|---|---|
| 会话 | `XDG_SESSION_TYPE=x11`，`XDG_CURRENT_DESKTOP=DDE`，`DISPLAY=:0` |
| 系统 | Deepin 25（crimson），内核 6.6.159 |
| 工具链 | rustc / cargo 已安装；xdotool、setxkbmap 可用；未安装 xremap/keyd/kanata |
| Command 键 | `Super_L` = keycode 0x85 / `KEY_LEFTMETA`，属 mod4 |
| `/dev/uinput` | `root:root 0600`（需 udev 规则放开） |
| 用户组 | 当前用户不在 `input` 组 |
| 输入设备 | 当前未连接外接键盘，仅有内置 `AT Translated Set 2 keyboard` |

结论：设备不可写死路径，必须在运行期枚举。当前环境只有一块内置键盘，正好验证"不依赖外接键盘"的设计目标；uinput 通常需要 udev 规则 + 加入 `input` 组（部分发行版已预置规则，见第 10 节）。以上仅为一种参考环境，代码不得据此硬编码。

## 4. 总体架构

```
所有键盘型设备 (/dev/input/event*, 含内置键盘, KEY_LEFTMETA)
        │ 能力检测后 EVIOCGRAB 独占抓取（每设备一个读取线程）
        ▼
┌──────────────────────────────────────────────────┐
│ cmd2ctrl 守护进程                                 │
│   device   枚举所有键盘型设备、能力检测、热插拔重扫   │
│   state    每设备一个延迟修饰键状态机（纯函数，可单测）│
│   focus    X11 焦点查询：_NET_ACTIVE_WINDOW → WM_CLASS │
│   mapping  两张键表：GUI 表 / 终端表                │
│   inject   uinput 虚拟键盘注入（统一出口，串行化）    │
└──────────────────────────────────────────────────┘
        │ 虚拟键盘事件
        ▼
  GUI 应用 / 终端模拟器（桌面环境照旧接收未被接管的 Super 组合）
```

**设备筛选（能力检测，而非名称白名单）**：一个事件设备被判定为"键盘"当且仅当它支持 `KEY_A`、`KEY_Z`、`KEY_SPACE`、`KEY_ENTER`（排除电源键、Video Bus、WMI 热键、纯媒体键等）。额外排除名称/VID-PID 与自身创建的虚拟设备匹配的设备，防止注入事件被再次抓取形成回环。

**线程模型**：每个被抓取的键盘一个读取线程（阻塞读 evdev），共享一个 uinput 设备；注入路径加锁串行化，保证多键盘并发时事件顺序不乱。焦点查询在每个读取线程内各自持有 X11 连接（x11rb 同步接口），避免跨线程共享连接的复杂度。主线程负责枚举、启动读取线程、每 2 秒重扫一次设备列表以支持热插拔。若实测焦点查询拖慢输入，再改为集中式焦点缓存 + 事件驱动刷新。

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
| SuperPending | 非按键事件（EV_MSC / EV_SYN / LED 等） | 原样转发 | SuperPending |
| SuperForwarded | 任意事件（含 Super 松开） | 原样转发 | Super 松开后 Idle |
| Consumed | 白名单键松开 / 重复键 | 无 | Consumed |
| Consumed | 其他普通键按下/松开 | 原样转发（此时 Super 未被按下，按键为无修饰原键） | Consumed |
| Consumed | Super 松开 | 无 | Idle |

补充规则：

- **"白名单键"的判定分两步**：先按当前焦点类型选出 GUI 表或终端表；若该键在表中有映射，按"有映射"处理（进入 `Consumed`）；若无映射（例如终端模式下的 `x/z/s/a/f/q`），按"非白名单键"处理，透传原始 Super 组合（进入 `SuperForwarded`）。
- 白名单键的**松开事件**在 `Consumed` 状态一律吞掉，避免目标应用收到"只有松开没有按下"的异常序列。
- 注入 chord 时先注入修饰键、再注入主键、再逆序释放修饰键，每个事件后附 `SYN_REPORT`。
- 按住白名单组合不放时，不做 chord 重复注入，避免影响目标应用的按键重复逻辑。
- `Super_R`（`KEY_RIGHTMETA`）与 `Super_L` 同等处理。

### 5.3 为什么可接受

延迟仅发生在"按下 Super 后、按下一个键之前"这段人工间隔内。Super 单独按下仍会在松开时补发，启动器行为不变；Super+E 等组合在第二个键到达时一起下发，桌面环境仍能识别。

## 6. 终端识别

1. 通过 x11rb 连接 `DISPLAY`，在 root 窗口读取 `_NET_ACTIVE_WINDOW` 属性（`WINDOW` 类型，EWMH 标准，DDE / GNOME / KDE / XFCE 等均实现）。
2. 若无该属性（非 EWMH 窗口管理器），回退 `GetInputFocus`；若焦点为 `PointerRoot`/`None` 则视为非终端。
3. 从焦点窗口向上查找，直到找到带有 `WM_CLASS` 的窗口（应对部分程序把输入焦点交给无 `WM_CLASS` 的子窗口）。读取 `WM_CLASS` 得到 `(instance, class)`，两者转小写后与终端白名单做精确匹配（非子串匹配，避免误伤）。
4. 每次被接管的组合键按下时重新查询一次，保证焦点切换立即生效；不做跨按键缓存。若实测查询耗时影响输入手感，再引入"缓存 + 事件驱动刷新"策略（见第 14 节）。

默认终端白名单（覆盖主流发行版常见终端，可按需配置）：

```
deepin-terminal, gnome-terminal, kgx, konsole, xfce4-terminal, xterm, urxvt,
alacritty, kitty, wezterm, tilix, terminator, st, mate-terminal, lxterminal,
qterminal, sakura
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

上表终端快捷键遵循主流终端（Deepin Terminal、GNOME Terminal、Konsole、Xfce4 Terminal、Tilix、Alacritty、Kitty、WezTerm 等）的默认键位。`xterm` / `urxvt` 的复制粘贴默认键位不同，若使用需在 `[terminal_map]` 中覆盖。任何终端的键位被用户改过时，同样通过配置覆盖，不改代码。

## 8. 配置

路径：`~/.config/cmd2ctrl/config.toml`（不存在时使用内置默认值）。

```toml
[device]
# 默认留空 = 处理所有键盘型设备（内置 + 外接，不做区分）
# 如需限制到部分键盘，填入名称子串（不区分大小写），多个为 OR 关系
include_patterns = []
# 额外排除的设备名称子串；自身虚拟设备名会被代码自动排除
exclude_patterns = []

[x11]
# 终端窗口 WM_CLASS 白名单（小写，精确匹配）
terminal_classes = [
  "deepin-terminal", "gnome-terminal", "kgx", "konsole", "xfce4-terminal",
  "xterm", "urxvt", "alacritty", "kitty", "wezterm", "tilix",
  "terminator", "st", "mate-terminal", "lxterminal", "qterminal", "sakura",
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

  --list-devices   列出所有事件设备：名称、路径、VID:PID、是否被判为键盘、是否可读
  --device         只处理名称匹配的键盘（可重复；默认处理所有键盘）
  --dry-run        只输出决策（当前焦点类型、将注入的 chord），不创建 uinput、不注入
  --verbose        输出每个被处理事件的调试日志
```

## 10. 权限与部署

以下为通用步骤；不同发行版/桌面的差异以"变体"标注，代码与文档均不假设 DDE 或 systemd 一定存在。

1. 确认 `/dev/uinput` 权限：`ls -l /dev/uinput`。若已是 `root:input 0660` 或用户可写，则跳过本步与第 2 步。
2. 写入 udev 规则 `/etc/udev/rules.d/99-uinput.rules`（文件名可自定，需 `.rules` 后缀）：

   ```
   KERNEL=="uinput", SUBSYSTEM=="misc", GROUP="input", MODE="0660"
   ```

   执行 `sudo udevadm control --reload-rules && sudo udevadm trigger --name-match=uinput`（非 systemd 发行版对应为 `udevadm` 单独执行）。

3. `sudo usermod -aG input "$USER"`，重新登录生效（`id -nG` 应包含 `input`）。
   - 变体：若发行版使用 `uinput` 专用组或已有预置规则，按 `ls -l /dev/uinput` 的属组加入对应组。
   - 变体：无法改权限时，以 root 运行同一二进制（文档注明，不作为默认路径）。

4. `cargo build --release`，二进制安装到 `~/.local/bin/cmd2ctrl`（或任意 `PATH` 内路径）。

5. 设置登录自启，二选一：
   - **systemd 发行版**：user 服务 `~/.config/systemd/user/cmd2ctrl.service`：

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
   - **非 systemd 或需要跟随具体桌面会话**：`~/.config/autostart/cmd2ctrl.desktop`，`Exec=.../cmd2ctrl`，由 XDG autostart 拉起。

6. 启动时检测 `XDG_SESSION_TYPE`；非 `x11` 时打印明确提示并以非零码退出（避免在 Wayland 下静默失效）。

## 11. 故障处理与安全

| 场景 | 行为 |
|---|---|
| 进程正常退出 / 崩溃 | 关闭设备 fd 自动释放 EVIOCGRAB，键盘恢复原生；uinput 设备随之销毁 |
| 运行期无任何键盘被识别 | 记录日志，每 2 秒重扫设备列表（新增键盘自动接入，拔出键盘自动移除） |
| 单个设备读取出错 / 断开 | 该读取线程退出，关闭 fd 释放抓取；主线程在下一次重扫时若设备仍在则重新打开 |
| 创建 uinput 失败 | **先创建 uinput、后抓取任何键盘**；失败立即退出，绝不进入"抓了键盘却没有输出通道"的状态 |
| 运行期 uinput 写入失败 | 立即释放全部设备抓取并退出，让键盘回到原生状态（宁可功能失效，不可锁死输入） |
| X11 连接失败 | 回退终端表（见第 6 节），并降频重连 |
| 配置解析错误 | 启动即失败并打印具体键名/字段，不进入抓取状态 |
| uinput 权限不足 | 启动即报错，提示执行 udev 规则与用户组步骤，绝不静默继续 |

由于本设计会抓取**所有**键盘，启动/退出顺序是硬性约束：初始化 uinput → 枚举设备 → 逐个抓取；任何一步失败都回滚并退出。设备的打开失败只影响该设备，不影响其他键盘。

## 12. 测试策略

### 单元测试（`cargo test`）

- 状态机：第 5.2 节每一条转移一行用例（输入事件序列 → 期望输出事件序列）。
- 键表解析：`ctrl+shift+c` 等字符串解析、未知键名报错、缺失键的透传语义。
- 修饰键跟踪：多修饰键组合、重复键抑制、松开顺序。
- 终端判定：WM_CLASS 精确匹配（大小写、instance/class 两种取值）。
- 设备筛选：键盘能力位判定（含电源键/纯媒体键的否定用例）、自身虚拟设备排除、include/exclude 模式匹配。

### 手工验证清单

1. 文本编辑器：`Cmd+C/V/X/Z/S/A/F` 均等价于 `Ctrl+*`。
2. 终端（当前环境 deepin-terminal，其他环境至少再验证一个终端）：`Cmd+C` 复制选中文本；无选中时按 `Cmd+C` 不影响 `sleep 100`；`Cmd+V` 粘贴。
3. 终端内物理 `Ctrl+C` 仍中断 `sleep 100`；`Ctrl+Z`、`Ctrl+S` 行为不变。
4. `Cmd+T/W/N` 在终端内为新建/关闭标签页、新建窗口；在浏览器中为新建/关闭标签页、新窗口。
5. 桌面环境自身的 Super 组合（如 Super+E、Super+D、Super+方向键、单独按 Super）行为与未安装时一致；换桌面环境时按各自绑定重测该项。
6. 按住 `Cmd+C` 不产生连续注入；快速连按无丢键。
7. **内置键盘**单独使用即满足第 1–5 项；再接一块外接键盘，两块键盘交替输入均正常。
8. 热插拔：运行中插入/拔出外接键盘，功能自动生效/移除；杀掉守护进程后所有键盘立即可用。
9. `--dry-run` 在 GUI 与终端中输出的判定与 chord 正确。
10. 在非 EWMH 或不同桌面环境下，`--verbose` 显示焦点查询走了哪条路径且判定正确。

## 13. 工程结构

```
cmd2ctrl-linux/
├── Cargo.toml
├── src/
│   ├── main.rs        # CLI 解析、组件装配、事件循环
│   ├── config.rs      # TOML 加载与默认值、chord 解析
│   ├── device.rs      # 枚举所有键盘型设备、能力检测、热插拔重扫、EVIOCGRAB
│   ├── state.rs       # 延迟修饰键状态机（无 IO 依赖，纯逻辑）
│   ├── mapping.rs     # GUI/终端键表、按键名 ↔ keycode
│   ├── focus.rs       # x11rb：_NET_ACTIVE_WINDOW、WM_CLASS、终端判定
│   └── inject.rs      # uinput 虚拟键盘创建与事件注入
├── config/config.toml # 示例配置
├── packaging/
│   ├── 99-uinput.rules
│   ├── cmd2ctrl.service
│   └── cmd2ctrl.desktop
└── docs/superpowers/specs/2026-11-27-cmd2ctrl-design.md
```

依赖：`evdev`（含 uinput 支持）、`x11rb`、`clap`、`serde`、`toml`、`regex`、`anyhow`、`log` + `env_logger`。

## 14. 风险与已决策项

| 风险 | 应对 |
|---|---|
| 终端默认键位与用户实际配置不一致（不同终端/自定义键位） | `terminal_map` 可覆盖；默认表覆盖主流终端，xterm/urxvt 等需按需配置 |
| 焦点查询延迟影响输入手感 | 先直接查询；若不达标改为缓存 + 后台刷新 |
| 多个键盘同时输入 | 每设备独立状态机 + 共享注入锁，事件顺序按到达串行化 |
| 跨设备组合键（一块键盘按 Super、另一块按 C） | 不支持：各设备状态独立，不会组合；属于可接受的极端场景 |
| 抓取所有键盘后占用/权限异常 | 先建 uinput 再抓取；写入失败立即释放抓取并退出；单个设备失败不影响其他设备 |
| 蓝牙键盘路径变化 | 每 2 秒重扫设备列表，按能力与名称重新识别，不依赖固定路径 |
| 与桌面环境全局快捷键冲突（DDE/GNOME/KDE 等 Super 绑定各不相同） | 白名单仅覆盖编辑/应用类组合，桌面组合不接管；换桌面时按清单第 5 项验证，必要时调整白名单 |
| 非 EWMH 窗口管理器下焦点判定失效 | `GetInputFocus` + 向上查找带 `WM_CLASS` 的祖先窗口；仍失败则回退终端表 |
| Wayland 会话 | v1 不支持，启动时检测 `XDG_SESSION_TYPE` 并给出明确提示后退出 |
