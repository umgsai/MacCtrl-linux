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
            Ok(Some(win)) => match self.wm_class(win) {
                Ok(Some((instance, class))) => {
                    if is_terminal_class(&instance, &class, classes) {
                        Mode::Terminal
                    } else {
                        Mode::Gui
                    }
                }
                Ok(None) => Mode::Gui,
                Err(_) => Mode::Terminal,
            },
            Ok(None) => Mode::Gui,
            Err(_) => Mode::Terminal,
        }
    }

    fn active_window(&self) -> Result<Option<Window>> {
        let reply = self
            .conn
            .get_property(false, self.root, AtomEnum::WINDOW, self.active_atom, 0, 1)?
            .reply()?;
        Ok(reply.value32().and_then(|mut it| it.next()))
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
}
