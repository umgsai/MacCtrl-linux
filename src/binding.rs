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

/// 绑定表：持有全部绑定，并提供按物理组合查找与启停控制。
#[derive(Debug)]
pub struct BindingTable {
    bindings: Vec<Binding>,
}

impl BindingTable {
    /// 构造绑定表；已启用的绑定之间不允许出现重复的物理按键组合。
    pub fn new(bindings: Vec<Binding>) -> Result<Self> {
        let mut seen: HashMap<(bool, u16), String> = HashMap::new();
        for b in &bindings {
            if !b.enabled {
                continue;
            }
            let key = (b.pattern.shift, b.pattern.code.0);
            if let Some(prev) = seen.insert(key, b.id.clone()) {
                bail!(
                    "按键模式 {} 被多个已启用绑定使用: {prev} / {}",
                    b.pattern.label(),
                    b.id
                );
            }
        }
        Ok(Self { bindings })
    }

    /// 按物理组合与模式解析出绑定 id 及目标 chord；仅匹配已启用的绑定。
    pub fn resolve(&self, pattern: KeyPattern, mode: Mode) -> Option<(String, Chord)> {
        let b = self.bindings.iter().find(|b| b.pattern == pattern && b.enabled)?;
        let chord = b.target(mode)?.clone();
        Some((b.id.clone(), chord))
    }

    /// 切换指定 id 的启用状态。
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<()> {
        let b = self
            .bindings
            .iter_mut()
            .find(|b| b.id == id)
            .ok_or_else(|| anyhow::anyhow!("未知绑定 id: {id}"))?;
        b.enabled = enabled;
        Ok(())
    }

    pub fn all(&self) -> &[Binding] {
        &self.bindings
    }

    /// 汇总所有绑定可能注入的按键（含修饰键），去重后返回。
    pub fn all_injected_keys(&self) -> Vec<evdev::KeyCode> {
        let mut keys = Vec::new();
        for b in &self.bindings {
            for chord in [b.gui.as_ref(), b.terminal.as_ref()].into_iter().flatten() {
                keys.push(chord.code);
                for m in &chord.mods {
                    keys.push(match m {
                        crate::key::Modifier::Ctrl => evdev::KeyCode::KEY_LEFTCTRL,
                        crate::key::Modifier::Shift => evdev::KeyCode::KEY_LEFTSHIFT,
                        crate::key::Modifier::Alt => evdev::KeyCode::KEY_LEFTALT,
                    });
                }
            }
        }
        keys.sort_by_key(|k| k.0);
        keys.dedup();
        keys
    }
}

/// 生成供 `list` 命令展示的绑定表格文本。
pub fn list_bindings_text(bindings: &[Binding]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    writeln!(
        out,
        "{:<14} {:<18} {:<20} {:<20} {:<8} {}",
        "ID", "物理组合", "GUI", "终端", "启用", "来源"
    )
    .unwrap();
    for b in bindings {
        writeln!(
            out,
            "{:<14} {:<18} {:<20} {:<20} {:<8} {}",
            b.id,
            b.pattern.label(),
            b.gui.as_ref().map(|c| c.label()).unwrap_or_else(|| "-".to_string()),
            b.terminal.as_ref().map(|c| c.label()).unwrap_or_else(|| "-".to_string()),
            if b.enabled { "yes" } else { "no" },
            match b.source {
                Source::Default => "default",
                Source::Config => "config",
                Source::Cli => "cli",
            }
        )
        .unwrap();
    }
    out
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

    #[test]
    fn table_resolves_enabled_only() {
        let t = BindingTable::new(builtin_bindings()).unwrap();
        assert_eq!(
            t.resolve(KeyPattern::parse("c").unwrap(), Mode::Gui),
            Some(("cmd-c".to_string(), Chord::parse("ctrl+c").unwrap()))
        );
        assert_eq!(
            t.resolve(KeyPattern::parse("c").unwrap(), Mode::Terminal),
            Some(("cmd-c".to_string(), Chord::parse("ctrl+shift+c").unwrap()))
        );
        assert_eq!(t.resolve(KeyPattern::parse("x").unwrap(), Mode::Terminal), None);
        assert_eq!(t.resolve(KeyPattern::parse("d").unwrap(), Mode::Gui), None);
        assert_eq!(t.resolve(KeyPattern::parse("l").unwrap(), Mode::Gui), None);
    }

    #[test]
    fn table_rejects_duplicate_enabled_pattern() {
        let mut list = builtin_bindings();
        list.push(on("cmd-c-dup", "c", "ctrl+c", None));
        let err = BindingTable::new(list).unwrap_err().to_string();
        assert!(err.contains("cmd-c-dup"), "{err}");
        assert!(err.contains("cmd-c"), "{err}");
    }

    #[test]
    fn set_enabled_toggles_lookup() {
        let mut t = BindingTable::new(builtin_bindings()).unwrap();
        t.set_enabled("cmd-d", true).unwrap();
        assert_eq!(
            t.resolve(KeyPattern::parse("d").unwrap(), Mode::Gui),
            Some(("cmd-d".to_string(), Chord::parse("ctrl+d").unwrap()))
        );
        t.set_enabled("cmd-c", false).unwrap();
        assert_eq!(t.resolve(KeyPattern::parse("c").unwrap(), Mode::Gui), None);
    }

    #[test]
    fn list_text_contains_columns_and_source() {
        let t = BindingTable::new(builtin_bindings()).unwrap();
        let text = list_bindings_text(t.all());
        assert!(text.contains("ID"));
        assert!(text.contains("cmd-c"));
        assert!(text.contains("super+c"));
        assert!(text.contains("default"));
        assert!(text.lines().count() >= 61);
    }
}
