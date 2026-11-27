use std::collections::HashMap;

use anyhow::{bail, Result};

use crate::key::{Chord, KeyPattern};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Gui,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Default,
    Config,
    Cli,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub id: String,
    pub pattern: KeyPattern,
    pub gui: Option<Chord>,
    pub terminal: Option<Chord>,
    pub enabled: bool,
    pub source: Source,
}

impl Binding {
    /// 返回该绑定在指定模式下的目标 chord；未启用时返回 None。
    pub fn target(&self, mode: Mode) -> Option<&Chord> {
        if !self.enabled {
            return None;
        }
        match mode {
            Mode::Gui => self.gui.as_ref(),
            Mode::Terminal => self.terminal.as_ref(),
        }
    }
}

/// 构造一条绑定，字符串在编译期由测试保证合法。
fn b(id: &str, keys: &str, gui: Option<&str>, terminal: Option<&str>, enabled: bool, source: Source) -> Binding {
    Binding {
        id: id.to_string(),
        pattern: KeyPattern::parse(keys).expect("内置按键模式必须合法"),
        gui: gui.map(|s| Chord::parse(s).expect("内置 GUI chord 必须合法")),
        terminal: terminal.map(|s| Chord::parse(s).expect("内置终端 chord 必须合法")),
        enabled,
        source,
    }
}

/// 构造一条默认启用的内置绑定。
fn on(id: &str, keys: &str, gui: &str, terminal: Option<&str>) -> Binding {
    b(id, keys, Some(gui), terminal, true, Source::Default)
}

/// 构造一条默认关闭的内置绑定。
fn off(id: &str, keys: &str, gui: &str, terminal: Option<&str>) -> Binding {
    b(id, keys, Some(gui), terminal, false, Source::Default)
}

pub fn builtin_bindings() -> Vec<Binding> {
    vec![
        on("cmd-c", "c", "ctrl+c", Some("ctrl+shift+c")),
        on("cmd-v", "v", "ctrl+v", Some("ctrl+shift+v")),
        on("cmd-x", "x", "ctrl+x", None),
        on("cmd-z", "z", "ctrl+z", None),
        on("cmd-shift-z", "shift+z", "ctrl+shift+z", None),
        on("cmd-a", "a", "ctrl+a", None),
        on("cmd-f", "f", "ctrl+f", None),
        on("cmd-g", "g", "ctrl+g", None),
        on("cmd-shift-g", "shift+g", "ctrl+shift+g", None),
        on("cmd-s", "s", "ctrl+s", None),
        on("cmd-shift-v", "shift+v", "ctrl+shift+v", Some("ctrl+shift+v")),
        on("cmd-o", "o", "ctrl+o", None),
        on("cmd-n", "n", "ctrl+n", Some("ctrl+shift+n")),
        on("cmd-t", "t", "ctrl+t", Some("ctrl+shift+t")),
        on("cmd-w", "w", "ctrl+w", Some("ctrl+shift+w")),
        on("cmd-q", "q", "ctrl+q", None),
        on("cmd-r", "r", "ctrl+r", Some("ctrl+r")),
        on("cmd-shift-r", "shift+r", "ctrl+shift+r", None),
        on("cmd-y", "y", "ctrl+y", Some("ctrl+y")),
        on("cmd-b", "b", "ctrl+b", None),
        on("cmd-i", "i", "ctrl+i", None),
        on("cmd-u", "u", "ctrl+u", None),
        on("cmd-k", "k", "ctrl+k", None),
        on("cmd-comma", "comma", "ctrl+comma", None),
        on("cmd-slash", "slash", "ctrl+slash", None),
        on("cmd-equal", "equal", "ctrl+equal", None),
        on("cmd-minus", "minus", "ctrl+minus", None),
        on("cmd-0", "0", "ctrl+0", None),
        on("cmd-enter", "enter", "ctrl+enter", None),
        on("cmd-shift-t", "shift+t", "ctrl+shift+t", None),
        on("cmd-shift-w", "shift+w", "ctrl+shift+w", None),
        on("cmd-shift-n", "shift+n", "ctrl+shift+n", None),
        on("cmd-shift-f", "shift+f", "ctrl+shift+f", None),
        on("cmd-shift-h", "shift+h", "ctrl+shift+h", None),
        on("cmd-shift-p", "shift+p", "ctrl+shift+p", None),
        off("cmd-d", "d", "ctrl+d", None),
        off("cmd-e", "e", "ctrl+e", None),
        off("cmd-l", "l", "ctrl+l", None),
        off("cmd-p", "p", "ctrl+p", None),
        off("cmd-1", "1", "ctrl+1", None),
        off("cmd-2", "2", "ctrl+2", None),
        off("cmd-3", "3", "ctrl+3", None),
        off("cmd-4", "4", "ctrl+4", None),
        off("cmd-5", "5", "ctrl+5", None),
        off("cmd-6", "6", "ctrl+6", None),
        off("cmd-7", "7", "ctrl+7", None),
        off("cmd-8", "8", "ctrl+8", None),
        off("cmd-9", "9", "ctrl+9", None),
        off("cmd-left", "left", "home", Some("home")),
        off("cmd-right", "right", "end", Some("end")),
        off("cmd-up", "up", "ctrl+home", Some("ctrl+home")),
        off("cmd-down", "down", "ctrl+end", Some("ctrl+end")),
        off("cmd-shift-left", "shift+left", "shift+home", Some("shift+home")),
        off("cmd-shift-right", "shift+right", "shift+end", Some("shift+end")),
        off("cmd-shift-up", "shift+up", "ctrl+shift+home", Some("ctrl+shift+home")),
        off("cmd-shift-down", "shift+down", "ctrl+shift+end", Some("ctrl+shift+end")),
        off("cmd-pageup", "pageup", "ctrl+pageup", None),
        off("cmd-pagedown", "pagedown", "ctrl+pagedown", None),
        off("cmd-shift-s", "shift+s", "ctrl+shift+s", None),
        off("cmd-backspace", "backspace", "ctrl+backspace", None),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(id: &str) -> Binding {
        builtin_bindings().into_iter().find(|b| b.id == id).unwrap_or_else(|| panic!("缺少 {id}"))
    }

    #[test]
    fn enabled_count_is_35() {
        let n = builtin_bindings().iter().filter(|b| b.enabled).count();
        assert_eq!(n, 35);
    }

    #[test]
    fn enabled_spot_checks() {
        let c = find("cmd-c");
        assert!(c.enabled);
        assert_eq!(c.pattern, KeyPattern::parse("c").unwrap());
        assert_eq!(c.target(Mode::Gui), Some(&Chord::parse("ctrl+c").unwrap()));
        assert_eq!(c.target(Mode::Terminal), Some(&Chord::parse("ctrl+shift+c").unwrap()));

        let x = find("cmd-x");
        assert!(x.enabled);
        assert_eq!(x.target(Mode::Terminal), None);

        let sz = find("cmd-shift-z");
        assert_eq!(sz.pattern, KeyPattern::parse("shift+z").unwrap());
        assert_eq!(sz.target(Mode::Gui), Some(&Chord::parse("ctrl+shift+z").unwrap()));

        let y = find("cmd-y");
        assert_eq!(y.target(Mode::Terminal), Some(&Chord::parse("ctrl+y").unwrap()));

        let comma = find("cmd-comma");
        assert_eq!(comma.pattern, KeyPattern::parse("comma").unwrap());
    }

    #[test]
    fn no_duplicate_patterns() {
        let mut seen = std::collections::HashMap::new();
        for b in builtin_bindings() {
            let key = (b.pattern.shift, b.pattern.code.0);
            assert!(seen.insert(key, b.id.clone()).is_none(), "重复按键模式: {}", b.pattern.label());
        }
    }

    #[test]
    fn disabled_count_is_25() {
        let n = builtin_bindings().iter().filter(|b| !b.enabled).count();
        assert_eq!(n, 25);
    }

    #[test]
    fn disabled_spot_checks() {
        for id in ["cmd-d", "cmd-e", "cmd-l", "cmd-p"] {
            assert!(!find(id).enabled, "{id} 应默认关闭");
        }
        for n in 1..=9 {
            assert!(!find(&format!("cmd-{n}")).enabled);
        }
        for id in [
            "cmd-left", "cmd-right", "cmd-up", "cmd-down",
            "cmd-shift-left", "cmd-shift-right", "cmd-shift-up", "cmd-shift-down",
            "cmd-pageup", "cmd-pagedown", "cmd-shift-s", "cmd-backspace",
        ] {
            assert!(!find(id).enabled, "{id} 应默认关闭");
        }
        assert_eq!(find("cmd-left").target(Mode::Gui), None);
        assert_eq!(find("cmd-left").gui, Some(Chord::parse("home").unwrap()));
        assert_eq!(find("cmd-up").gui, Some(Chord::parse("ctrl+home").unwrap()));
        assert_eq!(find("cmd-backspace").gui, Some(Chord::parse("ctrl+backspace").unwrap()));
    }

    #[test]
    fn total_count_is_60() {
        assert_eq!(builtin_bindings().len(), 60);
    }
}
