use anyhow::{bail, Result};
use evdev::KeyCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPattern {
    pub shift: bool,
    pub code: KeyCode,
}

impl KeyPattern {
    /// 解析 "z" / "shift+z"；仅允许可选的 shift 前缀。
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        let (shift, name) = match s.strip_prefix("shift+") {
            Some(rest) => (true, rest.trim()),
            None => (false, s),
        };
        if name.contains('+') {
            bail!("不支持的修饰键组合: {s}（只允许可选 shift 前缀）");
        }
        let code = parse_key(name).ok_or_else(|| anyhow::anyhow!("未知按键名: {name}"))?;
        Ok(Self { shift, code })
    }

    pub fn label(&self) -> String {
        let base = key_name(self.code).unwrap_or_else(|| self.code.0.to_string());
        if self.shift {
            format!("super+shift+{base}")
        } else {
            format!("super+{base}")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    pub mods: Vec<Modifier>,
    pub code: KeyCode,
}

impl Chord {
    /// 解析 "ctrl+c" / "ctrl+shift+c" / "home"。
    pub fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s
            .split('+')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        if parts.is_empty() {
            bail!("空的按键序列");
        }
        let (key, mods_str) = parts.split_last().unwrap();
        let mut mods = Vec::new();
        for m in mods_str {
            let md = match *m {
                "ctrl" => Modifier::Ctrl,
                "shift" => Modifier::Shift,
                "alt" => Modifier::Alt,
                other => bail!("未知修饰键: {other}"),
            };
            if !mods.contains(&md) {
                mods.push(md);
            }
        }
        let code = parse_key(key).ok_or_else(|| anyhow::anyhow!("未知按键名: {key}"))?;
        Ok(Self { mods, code })
    }

    pub fn label(&self) -> String {
        let mut out = String::new();
        for m in &self.mods {
            out.push_str(match m {
                Modifier::Ctrl => "ctrl+",
                Modifier::Shift => "shift+",
                Modifier::Alt => "alt+",
            });
        }
        out.push_str(&key_name(self.code).unwrap_or_else(|| self.code.0.to_string()));
        out
    }

    fn mod_code(m: Modifier) -> u16 {
        match m {
            Modifier::Ctrl => KeyCode::KEY_LEFTCTRL.0,
            Modifier::Shift => KeyCode::KEY_LEFTSHIFT.0,
            Modifier::Alt => KeyCode::KEY_LEFTALT.0,
        }
    }

    /// 生成注入序列：(keycode, value)。skip_shift 为 true 时跳过 Shift 的按下/释放。
    pub fn events(&self, skip_shift: bool) -> Vec<(u16, i32)> {
        let mut out = Vec::new();
        for m in &self.mods {
            if skip_shift && *m == Modifier::Shift {
                continue;
            }
            out.push((Self::mod_code(*m), 1));
        }
        out.push((self.code.0, 1));
        out.push((self.code.0, 0));
        for m in self.mods.iter().rev() {
            if skip_shift && *m == Modifier::Shift {
                continue;
            }
            out.push((Self::mod_code(*m), 0));
        }
        out
    }
}

/// 字母键与 evdev 键码的映射（evdev 键码按扫描码排列，并非字母顺序）。
const LETTER_KEYS: [(char, KeyCode); 26] = [
    ('a', KeyCode::KEY_A),
    ('b', KeyCode::KEY_B),
    ('c', KeyCode::KEY_C),
    ('d', KeyCode::KEY_D),
    ('e', KeyCode::KEY_E),
    ('f', KeyCode::KEY_F),
    ('g', KeyCode::KEY_G),
    ('h', KeyCode::KEY_H),
    ('i', KeyCode::KEY_I),
    ('j', KeyCode::KEY_J),
    ('k', KeyCode::KEY_K),
    ('l', KeyCode::KEY_L),
    ('m', KeyCode::KEY_M),
    ('n', KeyCode::KEY_N),
    ('o', KeyCode::KEY_O),
    ('p', KeyCode::KEY_P),
    ('q', KeyCode::KEY_Q),
    ('r', KeyCode::KEY_R),
    ('s', KeyCode::KEY_S),
    ('t', KeyCode::KEY_T),
    ('u', KeyCode::KEY_U),
    ('v', KeyCode::KEY_V),
    ('w', KeyCode::KEY_W),
    ('x', KeyCode::KEY_X),
    ('y', KeyCode::KEY_Y),
    ('z', KeyCode::KEY_Z),
];

pub fn parse_key(name: &str) -> Option<KeyCode> {
    let lower = name.trim().to_ascii_lowercase();
    let n = lower.as_str();
    if n.len() == 1 {
        let b = n.as_bytes()[0];
        if b.is_ascii_lowercase() {
            return LETTER_KEYS
                .iter()
                .find(|(ch, _)| *ch == b as char)
                .map(|(_, code)| *code);
        }
        if (b'1'..=b'9').contains(&b) {
            return Some(KeyCode::new(KeyCode::KEY_1.0 + (b - b'1') as u16));
        }
        if b == b'0' {
            return Some(KeyCode::KEY_0);
        }
    }
    Some(match n {
        "comma" => KeyCode::KEY_COMMA,
        "dot" => KeyCode::KEY_DOT,
        "slash" => KeyCode::KEY_SLASH,
        "semicolon" => KeyCode::KEY_SEMICOLON,
        "apostrophe" => KeyCode::KEY_APOSTROPHE,
        "grave" => KeyCode::KEY_GRAVE,
        "leftbrace" => KeyCode::KEY_LEFTBRACE,
        "rightbrace" => KeyCode::KEY_RIGHTBRACE,
        "backslash" => KeyCode::KEY_BACKSLASH,
        "equal" => KeyCode::KEY_EQUAL,
        "minus" => KeyCode::KEY_MINUS,
        "enter" => KeyCode::KEY_ENTER,
        "tab" => KeyCode::KEY_TAB,
        "space" => KeyCode::KEY_SPACE,
        "backspace" => KeyCode::KEY_BACKSPACE,
        "delete" => KeyCode::KEY_DELETE,
        "insert" => KeyCode::KEY_INSERT,
        "escape" => KeyCode::KEY_ESC,
        "home" => KeyCode::KEY_HOME,
        "end" => KeyCode::KEY_END,
        "pageup" => KeyCode::KEY_PAGEUP,
        "pagedown" => KeyCode::KEY_PAGEDOWN,
        "left" => KeyCode::KEY_LEFT,
        "right" => KeyCode::KEY_RIGHT,
        "up" => KeyCode::KEY_UP,
        "down" => KeyCode::KEY_DOWN,
        _ => return None,
    })
}

pub fn key_name(code: KeyCode) -> Option<String> {
    if let Some((ch, _)) = LETTER_KEYS.iter().find(|(_, c)| *c == code) {
        return Some(ch.to_string());
    }
    if (KeyCode::KEY_1.0..=KeyCode::KEY_9.0).contains(&code.0) {
        return Some(((b'1' + (code.0 - KeyCode::KEY_1.0) as u8) as char).to_string());
    }
    let name = match code {
        KeyCode::KEY_0 => "0",
        KeyCode::KEY_COMMA => "comma",
        KeyCode::KEY_DOT => "dot",
        KeyCode::KEY_SLASH => "slash",
        KeyCode::KEY_SEMICOLON => "semicolon",
        KeyCode::KEY_APOSTROPHE => "apostrophe",
        KeyCode::KEY_GRAVE => "grave",
        KeyCode::KEY_LEFTBRACE => "leftbrace",
        KeyCode::KEY_RIGHTBRACE => "rightbrace",
        KeyCode::KEY_BACKSLASH => "backslash",
        KeyCode::KEY_EQUAL => "equal",
        KeyCode::KEY_MINUS => "minus",
        KeyCode::KEY_ENTER => "enter",
        KeyCode::KEY_TAB => "tab",
        KeyCode::KEY_SPACE => "space",
        KeyCode::KEY_BACKSPACE => "backspace",
        KeyCode::KEY_DELETE => "delete",
        KeyCode::KEY_INSERT => "insert",
        KeyCode::KEY_ESC => "escape",
        KeyCode::KEY_HOME => "home",
        KeyCode::KEY_END => "end",
        KeyCode::KEY_PAGEUP => "pageup",
        KeyCode::KEY_PAGEDOWN => "pagedown",
        KeyCode::KEY_LEFT => "left",
        KeyCode::KEY_RIGHT => "right",
        KeyCode::KEY_UP => "up",
        KeyCode::KEY_DOWN => "down",
        _ => return None,
    };
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use evdev::KeyCode;

    #[test]
    fn parse_simple_key() {
        let p = KeyPattern::parse("z").unwrap();
        assert_eq!(
            p,
            KeyPattern {
                shift: false,
                code: KeyCode::KEY_Z
            }
        );
    }

    #[test]
    fn parse_shift_key() {
        let p = KeyPattern::parse("shift+z").unwrap();
        assert_eq!(
            p,
            KeyPattern {
                shift: true,
                code: KeyCode::KEY_Z
            }
        );
        assert_eq!(p.label(), "super+shift+z");
    }

    #[test]
    fn reject_extra_modifier_in_pattern() {
        assert!(KeyPattern::parse("alt+z").is_err());
        assert!(KeyPattern::parse("ctrl+z").is_err());
    }

    #[test]
    fn parse_chord_with_modifiers() {
        let c = Chord::parse("ctrl+shift+c").unwrap();
        assert_eq!(c.mods, vec![Modifier::Ctrl, Modifier::Shift]);
        assert_eq!(c.code, KeyCode::KEY_C);
        assert_eq!(c.label(), "ctrl+shift+c");
    }

    #[test]
    fn parse_named_keys() {
        for (name, code) in [
            ("comma", KeyCode::KEY_COMMA),
            ("slash", KeyCode::KEY_SLASH),
            ("equal", KeyCode::KEY_EQUAL),
            ("minus", KeyCode::KEY_MINUS),
            ("enter", KeyCode::KEY_ENTER),
            ("backspace", KeyCode::KEY_BACKSPACE),
            ("pageup", KeyCode::KEY_PAGEUP),
            ("pagedown", KeyCode::KEY_PAGEDOWN),
            ("home", KeyCode::KEY_HOME),
            ("end", KeyCode::KEY_END),
        ] {
            assert_eq!(parse_key(name), Some(code), "{name}");
        }
    }

    #[test]
    fn reject_unknown_key_or_modifier() {
        assert!(Chord::parse("ctrl+nosuchkey").is_err());
        assert!(Chord::parse("super+c").is_err());
    }

    #[test]
    fn chord_events_order() {
        let c = Chord::parse("ctrl+shift+c").unwrap();
        assert_eq!(
            c.events(false),
            vec![
                (KeyCode::KEY_LEFTCTRL.0, 1),
                (KeyCode::KEY_LEFTSHIFT.0, 1),
                (KeyCode::KEY_C.0, 1),
                (KeyCode::KEY_C.0, 0),
                (KeyCode::KEY_LEFTSHIFT.0, 0),
                (KeyCode::KEY_LEFTCTRL.0, 0),
            ]
        );
    }

    #[test]
    fn chord_events_skip_shift() {
        let c = Chord::parse("ctrl+shift+c").unwrap();
        assert_eq!(
            c.events(true),
            vec![
                (KeyCode::KEY_LEFTCTRL.0, 1),
                (KeyCode::KEY_C.0, 1),
                (KeyCode::KEY_C.0, 0),
                (KeyCode::KEY_LEFTCTRL.0, 0),
            ]
        );
    }

    #[test]
    fn key_name_roundtrip() {
        for name in ["a", "c", "z", "0", "9", "comma", "pageup"] {
            let code = parse_key(name).unwrap();
            assert_eq!(key_name(code).as_deref(), Some(name));
        }
    }
}
