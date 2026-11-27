use anyhow::{Context, Result};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, Window};

use crate::binding::Mode;

/// 从 WM_CLASS 原始字节解析 (instance, class)。
pub fn parse_wm_class(raw: &[u8]) -> Option<(String, String)> {
    let mut it = raw.split(|b| *b == 0).filter(|part| !part.is_empty());
    let instance = String::from_utf8_lossy(it.next()?).to_string();
    let class = it
        .next()
        .map(|c| String::from_utf8_lossy(c).to_string())
        .unwrap_or_else(|| instance.clone());
    Some((instance, class))
}

/// 判断窗口类名是否命中终端白名单；精确匹配且不区分大小写。
pub fn is_terminal_class(instance: &str, class: &str, whitelist: &[String]) -> bool {
    let i = instance.to_ascii_lowercase();
    let c = class.to_ascii_lowercase();
    whitelist.iter().any(|w| *w == i || *w == c)
}

/// 根据 WM_CLASS 归类窗口；无法确定窗口类型时按终端处理（fail-safe）。
pub fn mode_for_wm_class(cls: Option<(String, String)>, classes: &[String]) -> Mode {
    match cls {
        Some((instance, class)) if is_terminal_class(&instance, &class, classes) => Mode::Terminal,
        Some(_) => Mode::Gui,
        None => Mode::Terminal, // fail-safe：无法确定窗口类型时按终端处理
    }
}

/// X11 焦点探针：查询当前活动窗口并归类为 GUI 或终端。
pub struct FocusProbe {
    conn: x11rb::rust_connection::RustConnection,
    root: Window,
    active_atom: u32,
}

impl FocusProbe {
    pub fn connect() -> Result<Self> {
        let (conn, screen) = x11rb::connect(None).context("连接 X11 失败")?;
        let root = conn.setup().roots[screen].root;
        let active_atom = conn
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")?
            .reply()?
            .atom;
        Ok(Self {
            conn,
            root,
            active_atom,
        })
    }

    /// 任何查询异常都回退到终端模式：误发 Ctrl+Shift+C 无害，误发 Ctrl+C 会杀进程。
    pub fn mode(&self, classes: &[String]) -> Mode {
        match self.active_window() {
            Ok(Some(win)) => mode_for_wm_class(self.wm_class(win).ok().flatten(), classes),
            _ => match self.input_focus() {
                Ok(Some(win)) => mode_for_wm_class(self.wm_class(win).ok().flatten(), classes),
                _ => Mode::Terminal,
            },
        }
    }

    fn active_window(&self) -> Result<Option<Window>> {
        let reply = self
            .conn
            .get_property(false, self.root, AtomEnum::WINDOW, self.active_atom, 0, 1)?
            .reply()?;
        Ok(reply.value32().and_then(|mut it| it.next()))
    }

    fn input_focus(&self) -> Result<Option<Window>> {
        let reply = self.conn.get_input_focus()?.reply()?;
        // 0 = None, 1 = PointerRoot：没有具体焦点窗口
        match reply.focus {
            0 | 1 => Ok(None),
            w => Ok(Some(w)),
        }
    }

    /// 自活动窗口向上最多爬 16 层，找到第一个带 WM_CLASS 的窗口。
    fn wm_class(&self, win: Window) -> Result<Option<(String, String)>> {
        let mut cur = win;
        for _ in 0..16 {
            let reply = self
                .conn
                .get_property(false, cur, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024)?
                .reply()?;
            if let Some(pair) = parse_wm_class(&reply.value) {
                return Ok(Some(pair));
            }
            let tree = self.conn.query_tree(cur)?.reply()?;
            if tree.parent == 0 || tree.parent == self.root {
                break;
            }
            cur = tree.parent;
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_wm_class_bytes() {
        let raw = b"deepin-terminal\0Deepin-terminal\0";
        assert_eq!(
            parse_wm_class(raw),
            Some(("deepin-terminal".to_string(), "Deepin-terminal".to_string()))
        );
        assert_eq!(parse_wm_class(b"\0\0"), None);
        assert_eq!(parse_wm_class(b""), None);
    }

    #[test]
    fn terminal_detection_is_exact_and_case_insensitive() {
        let wl = classes(&["deepin-terminal", "kitty"]);
        assert!(is_terminal_class("deepin-terminal", "Deepin-terminal", &wl));
        assert!(is_terminal_class("kitty", "kitty", &wl));
        assert!(!is_terminal_class(
            "deepin-terminal-helper",
            "Deepin-terminal-helper",
            &wl
        ));
        assert!(!is_terminal_class("code", "Code", &wl));
    }

    #[test]
    fn missing_wm_class_fails_safe_to_terminal() {
        let wl = classes(&["kitty"]);
        assert_eq!(mode_for_wm_class(None, &wl), Mode::Terminal);
        assert_eq!(
            mode_for_wm_class(Some(("kitty".to_string(), "kitty".to_string())), &wl),
            Mode::Terminal
        );
        assert_eq!(
            mode_for_wm_class(Some(("code".to_string(), "Code".to_string())), &wl),
            Mode::Gui
        );
    }
}
