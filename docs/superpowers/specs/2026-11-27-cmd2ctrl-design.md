# cmd2ctrl 设计文档

- 日期：2026-11-27
- 目标平台：Linux / X11（不限发行版与桌面环境；Deepin 25 + DDE 仅为当前验证环境）
- 状态：已实现（单元测试与 CLI 冒烟通过；真机键盘验证需先配置 `/dev/uinput` 权限）

## 1. 背景

用户在 Linux 上外接 Mac 键盘。Mac 键盘的 Command 键在 Linux 下上报为 `KEY_LEFTMETA`（Super，mod4），而用户习惯的编辑快捷键是 `Command+C/V/X/Z/S/A/F` 等。

直接把 Super 全局映射为 Ctrl 不可行：终端里的 `Ctrl+C` 是发送 `SIGINT` 中断进程，若 `Cmd+C` 也变成 `Ctrl+C`，在终端中复制就会误杀进程。因此需要一个**感知焦点窗口类型**的按键转换工具：

- 普通 GUI 窗口：`Cmd+X` → `Ctrl+X`
- 终端窗口：`Cmd+C` → `Ctrl+Shift+C`（复制）、`Cmd+V` → `Ctrl+Shift+V`（粘贴），物理 `Ctrl+C` 保持原样（中断）
- 未启用或未定义的 Super 组合原样交给桌面环境，不破坏 DDE / GNOME / KDE / XFCE 等桌面自身的 Super+E / Super+D / Super+方向键 / Super 启动器等快捷键

工具内置一份常用快捷键目录（见第 7 节），每项都可单独开启/关闭；与桌面环境冲突的项默认关闭。

## 2. 目标与非目标

### 目标

1. 仅转换已启用的绑定（内置目录见第 7 节），未启用或未定义的组合透传。
2. 按当前焦点窗口是否为终端，选择该绑定的 GUI 或终端目标按键序列。
3. 终端内物理 `Ctrl+C` / `Ctrl+Z` / `Ctrl+S` 行为完全不受影响。
4. **不区分键盘来源**：所有物理键盘（内置键盘、USB/蓝牙外接键盘、多键盘同时连接）行为一致；鼠标、触摸板、电源键、仅媒体键设备不受影响。
5. 每条快捷键都可单独开启/关闭（内置默认值 + 用户配置 + 命令行覆盖）。
6. 守护进程退出或崩溃后键盘立即恢复原生行为。

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

状态：`Idle`、`SuperPending`（收到 Super 按下但尚未转发）、`SuperForwarded`（Super 已转发，走透传）、`Consumed`（本次组合已被转换并吞掉）。

除 Super 外，只有 **Shift** 可以参与绑定（用于 `Cmd+Shift+Z` 这类组合）；Ctrl/Alt 参与的组合（Mac 的 `Cmd+Ctrl+*`、`Cmd+Option+*`）v1 不转换，一律透传。

| 当前状态 | 输入事件 | 输出 | 新状态 |
|---|---|---|---|
| Idle | Super 按下 | 无 | SuperPending |
| SuperPending | Shift 按下 | 无（暂存，不转发） | SuperPending |
| SuperPending | 有绑定的键按下 | 注入目标 chord（按下+释放），吞掉原键；暂存的 Shift 一并吞掉 | Consumed |
| SuperPending | 无绑定的键按下 | 转发 Super 按下 + 暂存 Shift 按下 + 原键按下 | SuperForwarded |
| SuperPending | Ctrl / Alt 按下 | 转发 Super 按下 + 暂存 Shift 按下 + 该修饰键 | SuperForwarded |
| SuperPending | Shift 松开（期间无其他键） | 转发 Super 按下 + Shift 按下+松开 | SuperForwarded |
| SuperPending | 重复键（Super auto-repeat） | 无 | SuperPending |
| SuperPending | Super 松开 | 转发 Super 按下（+ 暂存 Shift 的按下/松开）+ Super 松开 | Idle |
| SuperPending | 非按键事件（EV_SYN / EV_MSC / EV_LED 等） | 丢弃（`uinput.emit` 自动补 `SYN_REPORT`） | SuperPending |
| SuperForwarded | 任意事件（含 Super 松开） | 原样转发 | Super 松开后 Idle |
| Consumed | 被映射键松开 / 重复键 | 无 | Consumed |
| Consumed | 被映射键再次按下（Super 仍按住） | 重新注入目标 chord（避免"只按下不松开"卡键） | Consumed |
| Consumed | 暂存 Shift 松开 | 无 | Consumed |
| Consumed | 其他普通键按下/松开 | 原样转发（此时 Super 未被按下） | Consumed |
| Consumed | Super 松开 | 无 | Idle |

补充规则：

- **绑定匹配**：物理组合 = 实际按住的 Shift（无论按下先后）+ 键名，例如 `shift+z`；再按当前焦点类型取该绑定的 `gui` 或 `terminal` 目标。若该绑定在当前模式下没有目标（如终端模式下的 `cmd-x`），按"无绑定"处理，透传原始组合。
- **Shift 已先于 Super 转发**：若 Shift 在 Super 之前按下（已被正常转发），匹配仍成功；注入时跳过"已转发且仍按住"的修饰键，避免重复按下；若目标 chord 需要 Shift，则物理 Shift 已提供。
- **Shift 在 Super 之后按下**：Shift 被暂存，注入时由 chord 合成；其后的松开事件在 `Consumed` 中吞掉。
- 被映射键的**松开事件**在 `Consumed` 状态一律吞掉，避免目标应用收到"只有松开没有按下"的异常序列。
- 注入 chord 时先注入修饰键、再注入主键、再逆序释放修饰键，每个事件后附 `SYN_REPORT`。
- 按住已映射组合不放时，不做 chord 重复注入，避免影响目标应用的按键重复逻辑。
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

## 7. 快捷键目录（内置默认值）

id 命名规则为 `cmd-<键>` / `cmd-shift-<键>`，配置与命令行都用它引用。表中"透传"表示该模式下不拦截，原始 Super 组合照常发给系统（通常无动作）。

默认值策略：**关键编辑键默认开启；已知会遮蔽桌面快捷键或语义不一致的项默认关闭**，并在"说明"列写明原因，用户可按需打开。

### 7.1 默认开启

| id | 物理组合 | GUI 注入 | 终端注入 | 用途 |
|---|---|---|---|---|
| cmd-c | Super+C | Ctrl+C | Ctrl+Shift+C（复制） | 复制 |
| cmd-v | Super+V | Ctrl+V | Ctrl+Shift+V（粘贴） | 粘贴 |
| cmd-x | Super+X | Ctrl+X | 透传 | 剪切 |
| cmd-z | Super+Z | Ctrl+Z | 透传 | 撤销 |
| cmd-shift-z | Super+Shift+Z | Ctrl+Shift+Z | 透传 | 重做 |
| cmd-a | Super+A | Ctrl+A | 透传 | 全选 |
| cmd-f | Super+F | Ctrl+F | 透传 | 查找 |
| cmd-g | Super+G | Ctrl+G | 透传 | 查找下一个 |
| cmd-shift-g | Super+Shift+G | Ctrl+Shift+G | 透传 | 查找上一个 |
| cmd-s | Super+S | Ctrl+S | 透传 | 保存 |
| cmd-shift-v | Super+Shift+V | Ctrl+Shift+V | Ctrl+Shift+V | 无格式粘贴 |
| cmd-o | Super+O | Ctrl+O | 透传 | 打开文件 |
| cmd-n | Super+N | Ctrl+N | Ctrl+Shift+N（新窗口） | 新建文档 / 新建窗口 |
| cmd-t | Super+T | Ctrl+T | Ctrl+Shift+T（新标签） | 新建标签页 |
| cmd-w | Super+W | Ctrl+W | Ctrl+Shift+W（关标签） | 关闭标签页 / 关闭窗口 |
| cmd-q | Super+Q | Ctrl+Q | 透传 | 退出应用 |
| cmd-r | Super+R | Ctrl+R | Ctrl+R（反向搜索） | 刷新 / 终端历史反向搜索 |
| cmd-shift-r | Super+Shift+R | Ctrl+Shift+R | 透传 | 强制刷新 |
| cmd-y | Super+Y | Ctrl+Y | Ctrl+Y（readline yank） | 重做（部分应用）/ 粘贴已删除内容 |
| cmd-b | Super+B | Ctrl+B | 透传 | 加粗 |
| cmd-i | Super+I | Ctrl+I | 透传 | 斜体 |
| cmd-u | Super+U | Ctrl+U | 透传 | 下划线 |
| cmd-k | Super+K | Ctrl+K | 透传 | 插入链接 / 聚焦搜索框 |
| cmd-comma | Super+, | Ctrl+, | 透传 | 打开偏好设置 |
| cmd-slash | Super+/ | Ctrl+/ | 透传 | 切换行注释 |
| cmd-equal | Super+= | Ctrl+= | 透传 | 放大（字体 / 页面） |
| cmd-minus | Super+- | Ctrl+- | 透传 | 缩小（字体 / 页面） |
| cmd-0 | Super+0 | Ctrl+0 | 透传 | 缩放重置 |
| cmd-enter | Super+Enter | Ctrl+Enter | 透传 | 发送 / 提交 |
| cmd-shift-t | Super+Shift+T | Ctrl+Shift+T | 透传 | 恢复最近关闭的标签页 |
| cmd-shift-w | Super+Shift+W | Ctrl+Shift+W | 透传 | 关闭窗口 |
| cmd-shift-n | Super+Shift+N | Ctrl+Shift+N | 透传 | 新建无痕 / 隐私窗口 |
| cmd-shift-f | Super+Shift+F | Ctrl+Shift+F | 透传 | 在文件中查找（全局搜索） |
| cmd-shift-h | Super+Shift+H | Ctrl+Shift+H | 透传 | 在文件中替换（IDE） |
| cmd-shift-p | Super+Shift+P | Ctrl+Shift+P | 透传 | 命令面板（IDE） |

终端列"透传"的原因：这些组合在终端里有破坏性或不同语义，例如 `Ctrl+X`（readline 前缀）、`Ctrl+Z`（挂起进程）、`Ctrl+S`（冻结输出）、`Ctrl+A`（行首）、`Ctrl+F`（前移一字符）、`Ctrl+U`（删至行首）、`Ctrl+K`（删至行尾）、`Ctrl+Q`（XON）。终端内的复制/粘贴统一走 `Ctrl+Shift+C/V`，不碰 `Ctrl+C`。

### 7.2 默认关闭（已知冲突或语义差异）

| id | 物理组合 | GUI 注入 | 终端注入 | 用途 | 默认关闭原因 |
|---|---|---|---|---|---|
| cmd-d | Super+D | Ctrl+D | 透传 | 添加书签 / 复制当前行（应用相关） | Super+D = 显示桌面 |
| cmd-e | Super+E | Ctrl+E | 透传 | 用选中内容查找 / 居中当前行（应用相关） | Super+E = 文件管理器 |
| cmd-l | Super+L | Ctrl+L | 透传 | 聚焦地址栏 / 选中当前行 | Super+L = 锁屏 |
| cmd-p | Super+P | Ctrl+P | 透传 | 打印 | Super+P = 切换显示模式 |
| cmd-1 … cmd-9 | Super+1…9 | Ctrl+1…9 | 透传 | 切换到第 N 个标签页 / 应用 | Super+1…9 = 工作区/应用切换 |
| cmd-left | Super+Left | Home | Home | 跳到行首 | Super+Left = 窗口贴左 |
| cmd-right | Super+Right | End | End | 跳到行尾 | Super+Right = 窗口贴右 |
| cmd-up | Super+Up | Ctrl+Home | Ctrl+Home | 跳到文档开头 | Super+Up = 最大化 |
| cmd-down | Super+Down | Ctrl+End | Ctrl+End | 跳到文档结尾 | Super+Down = 还原窗口 |
| cmd-shift-left | Super+Shift+Left | Shift+Home | Shift+Home | 选中到行首 | Super+Shift+Left = 移动窗口到左侧工作区 |
| cmd-shift-right | Super+Shift+Right | Shift+End | Shift+End | 选中到行尾 | 同上，向右移动 |
| cmd-shift-up | Super+Shift+Up | Ctrl+Shift+Home | Ctrl+Shift+Home | 选中到文档开头 | 同上，向上移动 |
| cmd-shift-down | Super+Shift+Down | Ctrl+Shift+End | Ctrl+Shift+End | 选中到文档结尾 | 同上，向下移动 |
| cmd-pageup | Super+PageUp | Ctrl+PageUp | 透传 | 上一个标签页 | Super+PageUp = 切换工作区 |
| cmd-pagedown | Super+PageDown | Ctrl+PageDown | 透传 | 下一个标签页 | Super+PageDown = 切换工作区 |
| cmd-shift-s | Super+Shift+S | Ctrl+Shift+S | 透传 | 另存为 | Super+Shift+S = 区域截图（GNOME/DDE） |
| cmd-backspace | Super+Backspace | Ctrl+Backspace | 透传 | 删除到行首（macOS 语义） | 语义不同：macOS 删到行首，Linux `Ctrl+Backspace` 删一个词 |

说明：箭头类绑定若开启，会遮蔽窗口平铺/工作区快捷键；`cmd-left` / `cmd-right` 的 GUI 目标按 macOS 语义取 `Home` / `End`，若更习惯 Linux 的"按词跳转"可在配置里改成 `ctrl+left` / `ctrl+right`。

### 7.3 明确不内置

以下组合不在内置目录中。它们涉及桌面级行为，应由桌面环境处理；如确需接管，可在用户配置里自定义，但会遮蔽桌面原有功能，不推荐：

- `Super+Tab`、`Super+Space`、`Super+\``：应用切换、输入法切换、窗口切换。
- `Super+H`、`Super+M`、`Super+Shift+3/4/5`：隐藏、最小化、系统截图。
- `Super+F1…F12`：功能键（多数为应用或桌面保留）。
- `Super+Ctrl+*`、`Super+Alt+*`：Mac 的 `Cmd+Ctrl` / `Cmd+Option` 组合，v1 一律透传。

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

# ---- 开关与覆盖 ----
# 只需写你要改的项；未出现的 id 使用内置默认值。
# 开启默认关闭项：
[bindings.cmd-d]
enabled = true

# 关闭默认开启项：
[bindings.cmd-b]
enabled = false

# 覆盖内置目标键（例如把 Home 改成按词跳转）：
[bindings.cmd-left]
enabled = true
gui = "ctrl+left"
terminal = "ctrl+left"

# 新增自定义绑定（必须提供 keys 与 gui；keys 只支持可选的 shift 前缀）：
[bindings.cmd-shift-k]
keys = "shift+k"
gui = "ctrl+shift+k"
terminal = ""           # 空字符串 = 终端中透传
```

规则：

- `keys` 只描述 Super 之后的组合，仅允许可选前缀 `shift`（如 `z`、`shift+z`）；不支持 `ctrl` / `alt` / `meta`。
- `gui` / `terminal` 为空字符串表示该模式下透传；`terminal` 未提供则沿用该 id 的内置终端目标。
- 用户配置按 id 合并到内置目录：`enabled` / `gui` / `terminal` 可分别覆盖；新 id 追加。
- 同一物理组合在启用状态下被多个 id 使用时，启动即报错并列出冲突 id。
- chord 或键名解析失败时启动即报错，输出 id 与出错字段，不进入抓取状态。
- 命令行 `--enable <id>` / `--disable <id>` 只影响本次进程，不写回文件。

## 9. CLI

```
cmd2ctrl [--config <path>] [--device <name-pattern>]... [--list-devices] [--list-bindings]
         [--enable <id>]... [--disable <id>]... [--dry-run] [--verbose]

  --list-devices    列出所有事件设备：名称、路径、VID:PID、是否被判为键盘、是否可读
  --list-bindings   列出绑定目录：id、物理组合、GUI/终端目标、是否启用、取值来源
  --device          只处理名称匹配的键盘（可重复；默认处理所有键盘）
  --enable <id>     本次运行强制启用某个绑定（可重复，不写回配置）
  --disable <id>    本次运行强制关闭某个绑定（可重复，不写回配置）
  --dry-run         只输出决策（当前焦点类型、命中的绑定 id、将注入的 chord），不创建 uinput、不注入
  --verbose         输出每个被处理事件的调试日志
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

- 状态机：第 5.2 节每一条转移一行用例（输入事件序列 → 期望输出事件序列），含 Shift 先按/后按两种顺序。
- 绑定合并与开关：内置默认 + 用户覆盖 + 命令行覆盖的优先级；`enabled=false` 后透传；`terminal=""` 透传；新 id 追加；启用状态下物理组合重复时报错。
- 默认值断言：第 7.1 节每项默认 `enabled=true`，第 7.2 节每项默认 `enabled=false`，防止后续改动误调。
- chord 解析：`ctrl+shift+c`、`home`、`comma` 等字符串解析，未知键名报错，注入时跳过已按住的修饰键。
- 修饰键跟踪：多修饰键组合、重复键抑制、松开顺序。
- 终端判定：WM_CLASS 精确匹配（大小写、instance/class 两种取值）。
- 设备筛选：键盘能力位判定（含电源键/纯媒体键的否定用例）、自身虚拟设备排除、include/exclude 模式匹配。

### 手工验证清单

1. 文本编辑器：按第 7.1 节逐项验证默认开启的绑定（复制/粘贴/撤销/重做/保存/查找/打开/新建/标签页/格式/缩放/提交等）。
2. 终端（当前环境 deepin-terminal，其他环境至少再验证一个终端）：`Cmd+C` 复制选中文本；无选中时按 `Cmd+C` 不影响 `sleep 100`；`Cmd+V` 粘贴。
3. 终端内物理 `Ctrl+C` 仍中断 `sleep 100`；`Ctrl+Z`、`Ctrl+S`、`Ctrl+A`、`Ctrl+K`、`Ctrl+U` 行为不变。
4. `Cmd+T/W/N` 在终端内为新建/关闭标签页、新建窗口；在浏览器中为新建/关闭标签页、新窗口。
5. 桌面环境自身的 Super 组合（如 Super+E、Super+D、Super+L、Super+P、Super+方向键、Super+1、单独按 Super）行为与未安装时一致；换桌面环境时按各自绑定重测该项。
6. 开关生效：`--list-bindings` 显示默认状态；`--disable cmd-c` 后 `Cmd+C` 在 GUI 中不再复制；`--enable cmd-d` 后 `Cmd+D` 生效且桌面"显示桌面"被遮蔽（验证遮蔽行为符合预期）。
7. Shift 组合：`Cmd+Shift+Z` 在 Shift 先按、后按两种顺序下都能重做；`Cmd+Shift+T/N/W` 正常。
8. 按住 `Cmd+C` 不产生连续注入；快速连按无丢键。
9. **内置键盘**单独使用即满足第 1–7 项；再接一块外接键盘，两块键盘交替输入均正常。
10. 热插拔：运行中插入/拔出外接键盘，功能自动生效/移除；杀掉守护进程后所有键盘立即可用。
11. `--dry-run` 在 GUI 与终端中输出的判定、命中 id 与 chord 正确。
12. 在非 EWMH 或不同桌面环境下，`--verbose` 显示焦点查询走了哪条路径且判定正确。

## 13. 工程结构

```
cmd2ctrl-linux/
├── Cargo.toml
├── src/
│   ├── main.rs        # CLI 解析、组件装配、事件循环
│   ├── config.rs      # TOML 加载、与内置目录合并、chord 解析
│   ├── device.rs      # 枚举所有键盘型设备、能力检测、热插拔重扫、EVIOCGRAB
│   ├── state.rs       # 延迟修饰键状态机（无 IO 依赖，纯逻辑）
│   ├── mapping.rs     # 内置绑定目录、启用状态、按键名 ↔ keycode
│   ├── focus.rs       # x11rb：_NET_ACTIVE_WINDOW、WM_CLASS、终端判定
│   └── inject.rs      # uinput 虚拟键盘创建与事件注入
├── config/config.toml # 示例配置
├── packaging/
│   ├── 99-uinput.rules
│   ├── cmd2ctrl.service
│   └── cmd2ctrl.desktop
└── docs/superpowers/specs/2026-11-27-cmd2ctrl-design.md
```

依赖：`evdev`（含 uinput 支持）、`x11rb`、`clap`、`serde`、`toml`、`anyhow`、`log` + `env_logger`。

## 14. 风险与已决策项

| 风险 | 应对 |
|---|---|
| 终端默认键位与用户实际配置不一致（不同终端/自定义键位） | 每条绑定的 `terminal` 可覆盖；默认表覆盖主流终端，xterm/urxvt 等需按需配置 |
| 焦点查询延迟影响输入手感 | 先直接查询；若不达标改为缓存 + 后台刷新 |
| 多个键盘同时输入 | 每设备独立状态机 + 共享注入锁，事件顺序按到达串行化 |
| 跨设备组合键（一块键盘按 Super、另一块按 C） | 不支持：各设备状态独立，不会组合；属于可接受的极端场景 |
| 抓取所有键盘后占用/权限异常 | 先建 uinput 再抓取；写入失败立即释放抓取并退出；单个设备失败不影响其他设备 |
| 蓝牙键盘路径变化 | 每 2 秒重扫设备列表，按能力与名称重新识别，不依赖固定路径 |
| 与桌面环境全局快捷键冲突（DDE/GNOME/KDE 等 Super 绑定各不相同） | 已知冲突项默认关闭并在目录中标注；由于是内核级抓取，启用即会**遮蔽**桌面快捷键，这是开关的显式后果，`--list-bindings` 可随时核对 |
| 快捷键目录长期演进 | 目录集中在 `mapping.rs` 的内置表，新增/调整只需改一处，并有默认值断言测试防止误改 |
| 非 EWMH 窗口管理器下焦点判定失效 | `GetInputFocus` + 向上查找带 `WM_CLASS` 的祖先窗口；仍失败则回退终端表 |
| Wayland 会话 | v1 不支持，启动时检测 `XDG_SESSION_TYPE` 并给出明确提示后退出 |
