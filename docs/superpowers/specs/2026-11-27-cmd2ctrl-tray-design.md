# cmd2ctrl 托盘图标设计

**状态:** 已批准  
**日期:** 2026-11-27  
**关联:** [cmd2ctrl 主设计](./2026-11-27-cmd2ctrl-design.md)

## 目标

cmd2ctrl 作为后台常驻进程运行时，在系统托盘（StatusNotifier）显示图标和菜单，提供运行时控制入口。

## 非目标（YAGNI）

- Wayland 支持（当前 X11-only 不变）
- Reload 后弹通知气泡（日志记录足够）
- 单条绑定的运行时 toggle（CLI `--enable/--disable` 即可）
- 运行时绘制图标状态（用两套 PNG 区分）
- 跨平台托盘（目标平台 Linux，GTK 已覆盖 GNOME/KDE/Xfce/deepin）

## 依赖

| crate | 用途 |
|---|---|
| `tray-icon 0.15` + `features = ["gtk"]` | 跨平台托盘抽象，内部选 GTK/Qt/WinAPI |
| `gtk 0.18`（gtk-rs） | GTK 事件循环、菜单、MessageDialog |
| `arc-swap 1`（可选） | 原子替换 BindingTable；也可用 `Arc<RwLock<>>`，待定 |

系统依赖：`libgtk-3-dev`（构建时）、`libgtk-3-0`（运行时，deb `$auto` 自动声明）。

## 架构

### 线程模型

当前：主线程 → `runner::run_loop()` 无限循环（热插拔 + 设备子线程）。

改造后：

```
main()
 ├─ 解析 CLI + 加载 Config + 构造 BindingTable
 ├─ if want_tray && $DISPLAY 存在 && 非 list_* 命令：
 │    runner::spawn()  → 后台线程（原有 runner 逻辑 + channel poll）
 │    tray::init()      → 主线程 gtk::init() + gtk::main()
 │    gtk::main() 返回后 join runner
 ├─ else（headless / dry-run / --no-tray）：
 │    runner::run_loop() → 主线程直接跑（保留原签名作降级入口）
```

GTK 事件循环跑在主线程。runner 迁到后台线程，通过 `mpsc::channel` 接收控制指令，通过 `Arc<RwLock<RuntimeState>>` 暴露只读状态。

### tray.rs 新增模块

```rust
pub struct RuntimeState {
    pub paused: bool,
    pub active_keyboards: usize,
    pub dry_run: bool,
    pub enabled_bindings: usize,
    pub total_bindings: usize,
}

pub enum TrayCmd {
    TogglePause,
    ReloadConfig(Option<PathBuf>),  // None → 默认路径
    ListBindings,                   // 托盘自己处理（显示 dialog）
    OpenConfigFile,                 // 托盘自己处理（xdg-open）
    Exit,
}

pub struct RunnerHandle {
    pub cmd_tx: Sender<TrayCmd>,
    pub state:  Arc<RwLock<RuntimeState>>,
}
```

### runner 改造

**`spawn()` 新增入口：**

```rust
pub fn spawn(table: BindingTable, config: Config, dry_run: bool)
    -> (JoinHandle<()>, RunnerHandle)
```

返回 `(JoinHandle, RunnerHandle)`。JoinHandle 可 join 等 runner 退出；RunnerHandle 是托盘控制入口。

**内部主循环新增 channel poll：**

```rust
loop {
    thread::sleep(Duration::from_secs(2));

    // 1. 现有热插拔扫描逻辑（不动）
    running.retain(|_, h| !h.is_finished());
    for (path, dev) in collect_keyboards(&include, &exclude) {
        if !running.contains_key(&path) { /* spawn */ }
    }

    // 2. 新：非阻塞检查托盘指令
    match cmd_rx.try_recv() {
        Ok(TrayCmd::TogglePause) => { paused.store(!paused.load(Ordering::SeqCst), Ordering::SeqCst); }
        Ok(TrayCmd::ReloadConfig(path)) => {
            let new_table = load_and_build_table(path)?;
            table_store.store(new_table);  // ArcSwap 或 swap RwLock
        }
        Ok(TrayCmd::Exit) => { stop.store(true, Ordering::SeqCst); break; }
        Ok(TrayCmd::ListBindings) | Ok(TrayCmd::OpenConfigFile) => {} // 托盘自己处理
        Err(Empty) => {}
        Err(Disconnected) => { stop.store(true, Ordering::SeqCst); break; }
    }

    // 3. 更新共享状态
    update_state(&state, active_kbs, paused, enabled, total, dry_run);

    // 4. 检查致命错误
    if stop.load(Ordering::SeqCst) { break; }
}
```

**设备子线程 paused 透传：**

```rust
loop {
    if paused.load(Ordering::Relaxed) {
        for raw in dev.fetch_events()? {
            if raw.event_type() == EventType::KEY {
                sink.emit(&Out::Pass(Ev::Key { code: raw.code(), value: raw.value() }))?;
            }
        }
    } else {
        // 原有 machine.step 逻辑不动
    }
}
```

**BindingTable 热替换：**

runner 原来持有 `Arc<BindingTable>` 常量引用。Reload 需要替换为新值。两种方案：

- **ArcSwap（推荐）**：`table: ArcSwap<BindingTable>`，`store()` 替换，设备子线程下一次事件循环 `load()` 读到新值。无锁，适合读多写少。
- **Arc<RwLock<BindingTable>>**：兼容现有 Mutex 用法，但每次 step 要加读锁。

选 ArcSwap，新增依赖 `arc-swap = "1"`。

### 菜单结构

| 序号 | 菜单项 | 类型 | 行为 |
|---|---|---|---|
| 1 | `cmd2ctrl v0.1.0 — 运行中 · 3 键盘 · 42/60 绑定` | 标题（不可选） | tooltip 副标题，托盘每 2 秒刷新 |
| 2 | `□ 暂停/继续` | CheckboxMenuItem | TogglePause |
| 3 | `重载配置` | MenuItem | ReloadConfig |
| 4 | `列出绑定…` | MenuItem | 托盘线程弹 gtk::MessageDialog（用 list_bindings_text） |
| 5 | `打开配置文件` | MenuItem | `std::process::Command::new("xdg-open").arg(path).spawn()` |
| — | — 分隔线 — | SeparatorMenuItem | |
| 6 | `退出` | MenuItem | Exit → 等 runner JoinHandle → `gtk::main_quit()` |

### 图标

- 内置 16x16 / 22x22 / 32x32 PNG 三张（激活态），用 `include_bytes!` 编译进二进制
- 暂停态：另一套灰色图标（同样三尺寸）
- 图标放在 `packaging/icons/cmd2ctrl-{active,paused}-{16,22,32}.png`（6 张）
- 生成方式：SVG 源 → `convert -resize` 生成 PNG，SVG 入库

### CLI 变更

`Args` 新增：

```rust
/// 禁用托盘图标（headless 环境或不想显示）
#[arg(long)]
pub no_tray: bool,
```

托盘自动启用条件：
- 未传 `--no-tray`
- 非 `--list-devices` / `--list-bindings` 命令
- `$DISPLAY` 环境变量存在

### 错误处理矩阵

| 场景 | 处理 |
|---|---|
| GTK init 失败（缺 libgtk-3-0） | `warn!` 日志，降级 headless 跑 runner |
| `$DISPLAY` 不存在（TTY） | `info!("tray disabled: no DISPLAY")`，降级 headless |
| dry_run 模式 | 托盘正常启用，tooltip 注明 "dry-run"，不创建 uinput |
| 托盘 channel 断开（GTK 崩） | runner 收到 `Disconnected` → 置 stop → 释放 grab → 进程退出 |
| ReloadConfig 时配置损坏 | 保留旧 BindingTable，托盘弹 error dialog 提示 |
| ReloadConfig 成功后 | 更新 tooltip 统计，不弹通知 |
| 退出时 runner 没及时退出 | `JoinHandle::join()` 有超时（5 秒），超时则 `warn!` 后继续 |

### 文件变动汇总

| 文件 | 变动 |
|---|---|
| `src/main.rs` | 加 `--no-tray`；分 tray / headless 两路；重构 main 流程 |
| `src/runner.rs` | 新增 `spawn()` + `RunnerHandle` + channel + paused + ArcSwap table；保留原 `run_loop()` 作 headless 入口 |
| **新** `src/tray.rs` | GTK 初始化、菜单构建、事件处理、tooltip 刷新、图标管理 |
| `Cargo.toml` | 新增 `tray-icon`、`gtk`、`arc-swap` 依赖；可能新增 `shell-words` 或直接用 Command |
| **新** `packaging/icons/` | 6 张 PNG + 1 张 SVG 源 |

### 已决策细节

- **ArcSwap vs RwLock**：选 `arc-swap 1`。BindingTable 读多写少，ArcSwap 无锁热替换。设备线程调用 `machine.step(ev, table.load(), focus_fn)` 时拿到 `&BindingTable` 的一个快照引用，ArcSwap 保证引用有效性直到释放。
- **ReloadConfig 默认路径**：runner 内部持有原始 `Config`，Reload 时优先用 `Config::default_path()`，或用 `Config::load(Some(custom_path))` 如果 CLI 传了 `--config`。
- **托盘按钮行为**：左键点击托盘图标 → 触发菜单显示（tray-icon 默认行为，不必额外代码）。
- **线程安全**：`Sender<TrayCmd>`、`Arc<RwLock<RuntimeState>>`、`ArcSwap<BindingTable>`、`AtomicBool` 都是 Send，跨线程传递安全。

### 图标设计

初始做一张抽象 "⌘→Ctrl" 风格占位图标（SVG），后续可替换。尺寸 16/22/32 三档，各两张（激活态 / 暂停灰度态）。PNG 用 Inkscape 或 `rsvg-convert -w <N> -h <N>` 从 SVG 导出。
