mod binding;
mod config;
mod device;
mod focus;
mod inject;
mod key;
mod runner;
mod state;
mod tray;

use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::Parser;
use log::info;

use crate::binding::{builtin_bindings, list_bindings_text, BindingTable};
use crate::config::Config;

/// 把 Mac Command 组合映射为 Ctrl 组合（X11，终端感知）。
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Args {
    /// 配置文件路径（默认 ~/.config/macctrl/config.toml）
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// 只处理名称匹配的键盘（可重复；默认处理所有键盘）
    #[arg(long = "device")]
    pub devices: Vec<String>,
    /// 列出事件设备
    #[arg(long)]
    pub list_devices: bool,
    /// 列出快捷键目录与启用状态
    #[arg(long)]
    pub list_bindings: bool,
    /// 本次运行强制启用某个绑定 id（可重复）
    #[arg(long = "enable")]
    pub enable: Vec<String>,
    /// 本次运行强制关闭某个绑定 id（可重复）
    #[arg(long = "disable")]
    pub disable: Vec<String>,
    /// 只观察并打印决策，不创建 uinput、不抓取键盘
    #[arg(long)]
    pub dry_run: bool,
    /// 输出调试日志
    #[arg(long)]
    pub verbose: bool,
    /// 禁用托盘图标（headless 环境或不想显示）
    #[arg(long)]
    pub no_tray: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(if args.verbose { "debug" } else { "info" }),
    )
    .init();

    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "x11".to_string());
    if session != "x11" {
        bail!("仅支持 X11 会话，当前 XDG_SESSION_TYPE={session}");
    }

    let mut cfg = Config::load(args.config.as_deref())?;
    if !args.devices.is_empty() {
        cfg.device.include_patterns = args.devices.clone();
    }
    let mut bindings = cfg.apply(builtin_bindings())?;
    cfg.apply_cli(&mut bindings, &args.enable, &args.disable)?;
    let table = BindingTable::new(bindings)?;

    if args.list_bindings {
        print!("{}", list_bindings_text(table.all()));
        return Ok(());
    }

    if args.list_devices {
        for (path, dev) in evdev::enumerate() {
            let name = dev.name().unwrap_or("<unknown>");
            let keys = dev.supported_keys();
            let is_kb = keys.map(device::is_keyboard).unwrap_or(false);
            let selected = device::matches_filters(
                name,
                &cfg.device.include_patterns,
                &cfg.device.exclude_patterns,
            );
            println!(
                "{:<28} {:<40} keyboard={:<5} selected={}",
                path.display(),
                name,
                is_kb,
                is_kb && selected && !name.contains("macctrl")
            );
        }
        return Ok(());
    }

    let have_display = std::env::var("DISPLAY").is_ok();
    let want_tray = !args.no_tray && have_display;

    if want_tray {
        info!("以托盘模式启动");
        let cfg_clone = cfg.clone();
        let config_path = args.config.clone();
        let (join, handle) = runner::spawn(table.clone(), cfg_clone, args.dry_run, config_path);
        tray::run(handle, table)?;
        info!("等待 runner 退出…");
        let _ = join.join();
    } else {
        if !have_display && !args.no_tray {
            info!("tray disabled: no DISPLAY");
        } else if args.no_tray {
            info!("tray disabled by --no-tray");
        }
        let _ = runner::run_loop(table, cfg, args.dry_run);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Args::command().debug_assert();
    }

    #[test]
    fn parses_repeatable_flags() {
        let args = Args::parse_from([
            "macctrl",
            "--disable",
            "cmd-b",
            "--disable",
            "cmd-i",
            "--enable",
            "cmd-d",
            "--dry-run",
        ]);
        assert_eq!(args.disable, vec!["cmd-b".to_string(), "cmd-i".to_string()]);
        assert_eq!(args.enable, vec!["cmd-d".to_string()]);
        assert!(args.dry_run);
    }

    #[test]
    fn parses_device_filter() {
        let args = Args::parse_from(["macctrl", "--device", "Apple", "--device", "Keychron"]);
        assert_eq!(
            args.devices,
            vec!["Apple".to_string(), "Keychron".to_string()]
        );
    }
}
