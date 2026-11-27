use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use evdev::{AttributeSet, Device, EventType, KeyCode};
use log::{info, warn};

use crate::binding::{BindingTable, Mode};
use crate::config::Config;
use crate::device;
use crate::focus::FocusProbe;
use crate::inject::Sink;
use crate::state::{Ev, Machine};

#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct RuntimeState {
    pub paused: bool,
    pub active_keyboards: usize,
    pub dry_run: bool,
    pub enabled_bindings: usize,
    pub total_bindings: usize,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum TrayCmd {
    TogglePause,
    ReloadConfig(Option<PathBuf>),
    ListBindings,
    OpenConfigFile,
    Exit,
}

#[allow(dead_code)]
pub struct RunnerHandle {
    pub cmd_tx: mpsc::Sender<TrayCmd>,
    pub state: Arc<RwLock<RuntimeState>>,
}

type TrayCtrl = (
    mpsc::Receiver<TrayCmd>,
    Arc<RwLock<RuntimeState>>,
    Option<PathBuf>,
);

/// 后台启动 runner，返回 JoinHandle 和托盘控制句柄。
#[allow(dead_code)]
pub fn spawn(
    table: BindingTable,
    config: Config,
    dry_run: bool,
    config_path: Option<PathBuf>,
) -> (thread::JoinHandle<()>, RunnerHandle) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<TrayCmd>();
    let enabled_count = table.all().iter().filter(|b| b.enabled).count();
    let total_count = table.all().len();
    let state = Arc::new(RwLock::new(RuntimeState {
        dry_run,
        enabled_bindings: enabled_count,
        total_bindings: total_count,
        ..RuntimeState::default()
    }));
    let state_for_handle = state.clone();
    let join = thread::spawn(move || {
        run_loop_inner(table, config, dry_run, Some((cmd_rx, state, config_path)));
    });
    (
        join,
        RunnerHandle {
            cmd_tx,
            state: state_for_handle,
        },
    )
}

/// headless 入口（无托盘）。保持原签名兼容。
pub fn run_loop(table: BindingTable, config: Config, dry_run: bool) -> Result<()> {
    run_loop_inner(table, config, dry_run, None);
    Ok(())
}

fn run_loop_inner(table: BindingTable, config: Config, dry_run: bool, tray: Option<TrayCtrl>) {
    let table_arc = Arc::new(table);
    let table_store = Arc::new(RwLock::new(Arc::clone(&table_arc)));
    let classes = Arc::new(config.terminal_classes());
    let titles = Arc::new(config.terminal_titles());
    let include = Arc::new(config.device.include_patterns.clone());
    let exclude = Arc::new(config.device.exclude_patterns.clone());

    let keyboards = collect_keyboards(&include, &exclude);
    if keyboards.is_empty() {
        warn!("未发现可用键盘，等待热插拔（每 2 秒重扫）");
    }

    let sink = if dry_run {
        info!("dry-run 模式：被动读取设备，不抓取、不注入");
        Arc::new(Mutex::new(Sink::dry()))
    } else {
        // 虚拟设备需声明所有可能注入的按键，包括各绑定里的修饰键。
        let mut keys = AttributeSet::<KeyCode>::new();
        for (_, dev) in &keyboards {
            if let Some(supported) = dev.supported_keys() {
                for k in supported.iter() {
                    keys.insert(k);
                }
            }
        }
        for k in table_arc.all_injected_keys() {
            keys.insert(k);
        }
        match Sink::real(&keys) {
            Ok(s) => Arc::new(Mutex::new(s)),
            Err(e) => {
                warn!("创建虚拟输入设备失败: {e}；将以 dry-run 方式继续");
                Arc::new(Mutex::new(Sink::dry()))
            }
        }
    };

    let stop = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));

    let tray_rx = tray.as_ref().map(|(rx, _, _)| rx);
    let tray_state = tray.as_ref().map(|(_, state, _)| state);
    let tray_config_path = tray.as_ref().and_then(|(_, _, p)| p.clone());

    let mut running: HashMap<PathBuf, thread::JoinHandle<()>> = HashMap::new();
    for (path, dev) in keyboards {
        running.insert(
            path.clone(),
            spawn_device(
                path,
                dev,
                Arc::clone(&table_store),
                classes.clone(),
                titles.clone(),
                sink.clone(),
                stop.clone(),
                paused.clone(),
                !dry_run,
            ),
        );
    }

    // 主线程只负责热插拔重扫；设备各自的读取线程独立运行。
    loop {
        thread::sleep(Duration::from_secs(2));
        if stop.load(Ordering::SeqCst) {
            warn!("检测到致命错误，进程退出以释放所有键盘抓取");
            return;
        }

        // 处理托盘命令（非阻塞）
        if let Some(rx) = tray_rx {
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    TrayCmd::TogglePause => {
                        let new_val = !paused.load(Ordering::Relaxed);
                        paused.store(new_val, Ordering::Relaxed);
                        info!("托盘暂停切换: paused={new_val}");
                        if let Some(state) = tray_state {
                            if let Ok(mut s) = state.write() {
                                s.paused = new_val;
                            }
                        }
                    }
                    TrayCmd::ReloadConfig(path) => {
                        let cfg_path = path.as_deref().or(tray_config_path.as_deref());
                        match reload_table(cfg_path) {
                            Ok(new_table) => {
                                let enabled = new_table.all().iter().filter(|b| b.enabled).count();
                                let total = new_table.all().len();
                                *table_store.write().unwrap() = Arc::new(new_table);
                                info!("托盘重新加载配置成功：{enabled}/{total} 绑定");
                                if let Some(state) = tray_state {
                                    if let Ok(mut s) = state.write() {
                                        s.enabled_bindings = enabled;
                                        s.total_bindings = total;
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("托盘重新加载配置失败: {e:#}");
                            }
                        }
                    }
                    TrayCmd::ListBindings => {
                        info!("托盘请求列出绑定（日志见输出）");
                    }
                    TrayCmd::OpenConfigFile => {
                        info!("托盘请求打开配置文件");
                    }
                    TrayCmd::Exit => {
                        info!("托盘请求退出");
                        return;
                    }
                }
            }
        }

        // 刷新状态快照
        if let Some(state) = tray_state {
            if let Ok(mut s) = state.write() {
                s.paused = paused.load(Ordering::Relaxed);
                s.active_keyboards = running.len();
            }
        }

        running.retain(|_, h| !h.is_finished());
        for (path, dev) in collect_keyboards(&include, &exclude) {
            if !running.contains_key(&path) {
                running.insert(
                    path.clone(),
                    spawn_device(
                        path,
                        dev,
                        table_store.clone(),
                        classes.clone(),
                        titles.clone(),
                        sink.clone(),
                        stop.clone(),
                        paused.clone(),
                        !dry_run,
                    ),
                );
            }
        }
    }
}

fn reload_table(path: Option<&Path>) -> Result<BindingTable> {
    let cfg = Config::load(path).context("加载配置失败")?;
    let bindings = cfg
        .apply(crate::binding::builtin_bindings())
        .context("合并绑定失败")?;
    BindingTable::new(bindings).context("构造绑定表失败")
}

fn collect_keyboards(include: &[String], exclude: &[String]) -> Vec<(PathBuf, Device)> {
    evdev::enumerate()
        .filter(|(_, dev)| {
            dev.name()
                .map(|n| device::matches_filters(n, include, exclude) && !n.contains("macctrl"))
                .unwrap_or(false)
        })
        .filter(|(_, dev)| {
            dev.supported_keys()
                .map(device::is_keyboard)
                .unwrap_or(false)
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn spawn_device(
    path: PathBuf,
    mut dev: Device,
    table: Arc<RwLock<Arc<BindingTable>>>,
    classes: Arc<Vec<String>>,
    titles: Arc<Vec<String>>,
    sink: Arc<Mutex<Sink>>,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    grab: bool,
) -> thread::JoinHandle<()> {
    let name = dev.name().unwrap_or("<unknown>").to_string();
    let shown_path = path.display().to_string();
    thread::spawn(move || {
        if grab {
            if let Err(e) = dev.grab() {
                warn!("抓取 {name} ({shown_path}) 失败: {e}");
                return;
            }
            info!("已接管键盘: {name} ({shown_path})");
        }
        let mut machine = Machine::new();
        // 焦点探针失败时退化为终端模式（由 FocusProbe::mode 的调用方兜底）。
        let probe = FocusProbe::connect().ok();
        loop {
            let stream = match dev.fetch_events() {
                Ok(s) => s,
                Err(e) => {
                    warn!("读取 {name} 失败，释放该设备: {e}");
                    return;
                }
            };
            for raw in stream {
                // 只转发 EV_KEY：uinput.emit 会自动补 SYN_REPORT，EV_MSC 无转发价值。
                if raw.event_type() != EventType::KEY {
                    continue;
                }
                let ev = Ev::Key {
                    code: raw.code(),
                    value: raw.value(),
                };
                let outs = if paused.load(Ordering::Relaxed) {
                    vec![crate::state::Out::Pass(ev)]
                } else {
                    let table_ref = table.read().unwrap();
                    machine.step(ev, &table_ref, || {
                        probe
                            .as_ref()
                            .map(|p| p.mode(&classes, &titles))
                            .unwrap_or(Mode::Terminal)
                    })
                };
                let mut s = match sink.lock() {
                    Ok(s) => s,
                    Err(_) => {
                        stop.store(true, Ordering::SeqCst);
                        warn!("注入锁中毒，退出以释放键盘");
                        return;
                    }
                };
                for out in &outs {
                    if let Err(e) = s.emit(out) {
                        stop.store(true, Ordering::SeqCst);
                        warn!("注入失败，退出以释放键盘: {e}");
                        return;
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_cmd_send_recv() {
        let (tx, rx) = mpsc::channel::<TrayCmd>();
        tx.send(TrayCmd::TogglePause).unwrap();
        assert!(matches!(rx.try_recv(), Ok(TrayCmd::TogglePause)));
    }

    #[test]
    fn tray_cmd_variants() {
        use std::path::PathBuf;
        let _ = TrayCmd::TogglePause;
        let _ = TrayCmd::ReloadConfig(Some(PathBuf::from("x.toml")));
        let _ = TrayCmd::ListBindings;
        let _ = TrayCmd::OpenConfigFile;
        let _ = TrayCmd::Exit;
    }

    #[test]
    fn runtime_state_defaults() {
        let s = RuntimeState::default();
        assert!(!s.paused);
        assert!(!s.dry_run);
        assert_eq!(s.active_keyboards, 0);
        assert_eq!(s.enabled_bindings, 0);
        assert_eq!(s.total_bindings, 0);
    }
}
