use std::sync::Arc;
use std::time::Duration;

use gtk::prelude::*;
use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};

use crate::binding::BindingTable;
use crate::config::default_path;
use crate::runner::{RunnerHandle, TrayCmd};

const ICON_PATH: &str = "packaging/icons/cmd2ctrl-active.png";

/// 从文件加载 SVG/PNG 为 tray_icon::Icon。失败返回 None。
fn load_icon(path: &str) -> Option<tray_icon::Icon> {
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_file(path).ok()?;
    let width = pixbuf.width() as u32;
    let height = pixbuf.height() as u32;
    // 确保是 RGBA 格式
    let pixels = unsafe {
        if pixbuf.has_alpha() {
            pixbuf.pixels().to_vec()
        } else {
            // 无 alpha 通道，转为 RGBA
            let mut rgba = Vec::with_capacity((width * height * 4) as usize);
            let rowstride = pixbuf.rowstride() as usize;
            let raw = pixbuf.pixels();
            for y in 0..height as usize {
                for x in 0..width as usize {
                    let idx = y * rowstride + x * 3;
                    rgba.push(raw[idx]);
                    rgba.push(raw[idx + 1]);
                    rgba.push(raw[idx + 2]);
                    rgba.push(255);
                }
            }
            rgba
        }
    };
    tray_icon::Icon::from_rgba(pixels, width, height).ok()
}

/// 启动 GTK 事件循环（由主线程调用，阻塞直到 gtk::main_quit）。
pub fn run(handle: RunnerHandle, initial_table: BindingTable) -> anyhow::Result<()> {
    gtk::init()?;

    // --- 托盘图标 ---
    let icon = load_icon(ICON_PATH);
    let mut builder = tray_icon::TrayIconBuilder::new()
        .with_tooltip("cmd2ctrl")
        .with_title("cmd2ctrl");
    if let Some(ic) = &icon {
        builder = builder.with_icon(ic.clone());
    }

    // --- 菜单项 ---
    let menu = Menu::new();

    let enabled_count = initial_table.all().iter().filter(|b| b.enabled).count();
    let total_count = initial_table.all().len();
    let header = MenuItem::new(
        format!(
            "cmd2ctrl — 运行中 · 0 键盘 · {}/{} 绑定",
            enabled_count, total_count
        ),
        false, // disabled (header-only)
        None,
    );

    let pause = MenuItem::new("暂停", true, None);
    let reload = MenuItem::new("重载配置", true, None);
    let list = MenuItem::new("列出绑定…", true, None);
    let open_cfg = MenuItem::new("打开配置文件", true, None);
    let exit = MenuItem::new("退出", true, None);

    menu.append(&header)
        .map_err(|e| anyhow::anyhow!("menu append failed: {e}"))?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| anyhow::anyhow!("menu separator failed: {e}"))?;
    menu.append(&pause)
        .map_err(|e| anyhow::anyhow!("menu append failed: {e}"))?;
    menu.append(&reload)
        .map_err(|e| anyhow::anyhow!("menu append failed: {e}"))?;
    menu.append(&list)
        .map_err(|e| anyhow::anyhow!("menu append failed: {e}"))?;
    menu.append(&open_cfg)
        .map_err(|e| anyhow::anyhow!("menu append failed: {e}"))?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| anyhow::anyhow!("menu separator failed: {e}"))?;
    menu.append(&exit)
        .map_err(|e| anyhow::anyhow!("menu append failed: {e}"))?;

    // --- 构建 TrayIcon ---
    let tray = builder
        .with_menu(Box::new(menu))
        .build()
        .map_err(|e| anyhow::anyhow!("tray icon build failed: {e}"))?;

    // --- 用 Arc 包装 RunnerHandle 以便在闭包中 clone ---
    let handle = Arc::new(handle);

    // --- 提前 clone 第二个定时器需要的菜单项，避免 move 冲突 ---
    let header_for_refresh = header.clone();
    let pause_for_refresh = pause.clone();
    let state_for_refresh = handle.state.clone();

    // --- 菜单事件轮询（GTK 主循环内） ---
    let h_for_pause = handle.clone();
    let h_for_reload = handle.clone();
    let h_for_exit = handle.clone();
    let list_text = crate::binding::list_bindings_text(initial_table.all());

    gtk::glib::timeout_add_local(Duration::from_millis(150), move || {
        while let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if event.id == *pause.id() {
                let _ = h_for_pause.cmd_tx.send(TrayCmd::TogglePause);
            } else if event.id == *reload.id() {
                let _ = h_for_reload.cmd_tx.send(TrayCmd::ReloadConfig(None));
            } else if event.id == *list.id() {
                // 在 GTK 主线程弹出 dialog
                let dialog = gtk::MessageDialog::new(
                    Option::<&gtk::Window>::None,
                    gtk::DialogFlags::empty(),
                    gtk::MessageType::Info,
                    gtk::ButtonsType::Ok,
                    list_text.as_str(),
                );
                dialog.set_title("cmd2ctrl - 绑定列表");
                dialog.set_default_width(800);
                dialog.set_default_height(600);
                dialog.connect_response(|d, _| d.close());
                dialog.show_all();
            } else if event.id == *open_cfg.id() {
                let _ = std::process::Command::new("xdg-open")
                    .arg(default_path())
                    .spawn();
            } else if event.id == *exit.id() {
                let _ = h_for_exit.cmd_tx.send(TrayCmd::Exit);
                // 延迟退出，给 runner 一点时间清理
                gtk::glib::MainContext::default().spawn_local(async {
                    gtk::glib::timeout_future(Duration::from_millis(500)).await;
                    gtk::main_quit();
                });
            }
        }
        gtk::glib::ControlFlow::Continue
    });

    // --- 定时器：每 2 秒刷新 header 状态 + pause label ---
    gtk::glib::timeout_add_local(Duration::from_secs(2), move || {
        if let Ok(st) = state_for_refresh.read() {
            let status = if st.paused { "已暂停" } else { "运行中" };
            let dry_suffix = if st.dry_run { " (dry-run)" } else { "" };
            header_for_refresh.set_text(format!(
                "cmd2ctrl — {status}{dry_suffix} · {} 键盘 · {}/{} 绑定",
                st.active_keyboards, st.enabled_bindings, st.total_bindings,
            ));
            pause_for_refresh.set_text(if st.paused { "继续" } else { "暂停" });
        }
        gtk::glib::ControlFlow::Continue
    });

    // 保持 TrayIcon 存活
    let _ = tray;
    gtk::main();
    Ok(())
}
