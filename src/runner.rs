use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use evdev::{AttributeSet, Device, EventType, KeyCode};
use log::{info, warn};

use crate::binding::{BindingTable, Mode};
use crate::config::Config;
use crate::device;
use crate::focus::FocusProbe;
use crate::inject::Sink;
use crate::state::{Ev, Machine};

pub fn run_loop(table: BindingTable, config: Config, dry_run: bool) -> Result<()> {
    let table = Arc::new(table);
    let classes = Arc::new(config.terminal_classes());
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
        for k in table.all_injected_keys() {
            keys.insert(k);
        }
        Arc::new(Mutex::new(Sink::real(&keys)?))
    };

    let stop = Arc::new(AtomicBool::new(false));
    let mut running: HashMap<PathBuf, thread::JoinHandle<()>> = HashMap::new();
    for (path, dev) in keyboards {
        running.insert(
            path.clone(),
            spawn_device(
                path,
                dev,
                table.clone(),
                classes.clone(),
                sink.clone(),
                stop.clone(),
                !dry_run,
            ),
        );
    }

    // 主线程只负责热插拔重扫；设备各自的读取线程独立运行。
    loop {
        thread::sleep(Duration::from_secs(2));
        if stop.load(Ordering::SeqCst) {
            warn!("检测到致命错误，进程退出以释放所有键盘抓取");
            return Ok(());
        }
        running.retain(|_, h| !h.is_finished());
        for (path, dev) in collect_keyboards(&include, &exclude) {
            if !running.contains_key(&path) {
                running.insert(
                    path.clone(),
                    spawn_device(
                        path,
                        dev,
                        table.clone(),
                        classes.clone(),
                        sink.clone(),
                        stop.clone(),
                        !dry_run,
                    ),
                );
            }
        }
    }
}

fn collect_keyboards(include: &[String], exclude: &[String]) -> Vec<(PathBuf, Device)> {
    evdev::enumerate()
        .filter(|(_, dev)| {
            dev.name()
                .map(|n| device::matches_filters(n, include, exclude) && !n.contains("cmd2ctrl"))
                .unwrap_or(false)
        })
        .filter(|(_, dev)| {
            dev.supported_keys()
                .map(device::is_keyboard)
                .unwrap_or(false)
        })
        .collect()
}

fn spawn_device(
    path: PathBuf,
    mut dev: Device,
    table: Arc<BindingTable>,
    classes: Arc<Vec<String>>,
    sink: Arc<Mutex<Sink>>,
    stop: Arc<AtomicBool>,
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
                let outs = machine.step(ev, &table, || {
                    probe
                        .as_ref()
                        .map(|p| p.mode(&classes))
                        .unwrap_or(Mode::Terminal)
                });
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
