use evdev::KeyCode;

use crate::binding::{BindingTable, Mode};
use crate::key::{Chord, KeyPattern};

/// 输入事件：按键事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ev {
    Key { code: u16, value: i32 },
}

/// 状态机的输出：原样放行事件，或注入目标按键序列。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    Pass(Ev),
    Inject {
        id: String,
        chord: Chord,
        skip_shift: bool,
    },
}

/// 延迟 Super 状态机的内部状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// 空闲：无按键按下。
    Idle,
    /// 已按下 Super，尚未放行，等待可能的组合键。
    Pending,
    /// 已放行 Super 及其后的按键，等待 Super 释放。
    Forwarded,
    /// 组合键已命中并注入，记录占用该组合的按键码。
    Consumed(u16),
}

/// 延迟 Super 状态机：Super 按下时先不转发，以判断是否为需转换的组合键。
pub struct Machine {
    state: State,
    /// 组合键中按下但尚未放行的 Shift。
    shift_buffered: bool,
    /// Shift 已在 Super 之前或之前放行过，注入时需跳过 Shift。
    shift_forwarded: bool,
}

const SUPER_L: u16 = KeyCode::KEY_LEFTMETA.0;
const SUPER_R: u16 = KeyCode::KEY_RIGHTMETA.0;
const SHIFT_L: u16 = KeyCode::KEY_LEFTSHIFT.0;
const SHIFT_R: u16 = KeyCode::KEY_RIGHTSHIFT.0;
const CTRL_L: u16 = KeyCode::KEY_LEFTCTRL.0;
const CTRL_R: u16 = KeyCode::KEY_RIGHTCTRL.0;
const ALT_L: u16 = KeyCode::KEY_LEFTALT.0;
const ALT_R: u16 = KeyCode::KEY_RIGHTALT.0;

fn ev_key(code: u16, value: i32) -> Ev {
    Ev::Key { code, value }
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            shift_buffered: false,
            shift_forwarded: false,
        }
    }

    /// 处理单个事件，返回需要发往下游的事件序列。
    pub fn step<F: FnOnce() -> Mode>(&mut self, ev: Ev, table: &BindingTable, mode: F) -> Vec<Out> {
        let Ev::Key { code, value } = ev;

        // 修饰键单独处理，以便缓冲或放行。
        if code == SUPER_L || code == SUPER_R {
            return self.on_super(value, ev);
        }
        if code == SHIFT_L || code == SHIFT_R {
            return self.on_shift(value, ev);
        }
        if code == CTRL_L || code == CTRL_R || code == ALT_L || code == ALT_R {
            return self.on_ctrl_alt(value, ev);
        }

        match self.state {
            State::Pending => {
                if value == 1 {
                    let pattern = KeyPattern {
                        shift: self.shift_buffered || self.shift_forwarded,
                        code: KeyCode::new(code),
                    };
                    if let Some((id, chord)) = table.resolve(pattern, mode()) {
                        // 命中绑定：注入目标 chord，并记录被吞下的按键。
                        let skip_shift = self.shift_forwarded;
                        self.state = State::Consumed(code);
                        return vec![Out::Inject {
                            id,
                            chord,
                            skip_shift,
                        }];
                    }
                }
                self.flush_pending(code, value)
            }
            State::Consumed(mapped) => {
                // 被转换的组合键：其释放与自动重复一并吞掉，避免泄漏到下游。
                if code == mapped && (value == 0 || value == 2) {
                    vec![]
                } else if code == mapped && value == 1 {
                    // Super 仍按住时再次按下同一键：重新解析并注入，
                    // 否则会输出一次缺少 Super 的普通按键，且其释放被吞掉造成卡键。
                    let pattern = KeyPattern {
                        shift: self.shift_buffered || self.shift_forwarded,
                        code: KeyCode::new(code),
                    };
                    match table.resolve(pattern, mode()) {
                        Some((id, chord)) => vec![Out::Inject {
                            id,
                            chord,
                            skip_shift: self.shift_forwarded,
                        }],
                        None => vec![Out::Pass(ev)],
                    }
                } else {
                    vec![Out::Pass(ev)]
                }
            }
            _ => vec![Out::Pass(ev)],
        }
    }

    /// 未命中绑定：先补发 Super，再放行当前按键。
    fn flush_pending(&mut self, code: u16, value: i32) -> Vec<Out> {
        let mut out = vec![Out::Pass(ev_key(SUPER_L, 1))];
        if self.shift_buffered {
            out.push(Out::Pass(ev_key(SHIFT_L, 1)));
            self.shift_forwarded = true;
            self.shift_buffered = false;
        }
        out.push(Out::Pass(ev_key(code, value)));
        self.state = State::Forwarded;
        out
    }

    fn on_super(&mut self, value: i32, ev: Ev) -> Vec<Out> {
        match (self.state, value) {
            // 空闲时按下 Super：进入待定，暂不转发。
            (State::Idle, 1) => {
                self.state = State::Pending;
                vec![]
            }
            // 待定期间重复按下或自动重复：吞掉。
            (State::Pending, 1) | (State::Pending, 2) => vec![],
            // 待定期间松开 Super：视为单纯点击，补发按下与释放。
            (State::Pending, 0) => {
                let mut out = Vec::new();
                if self.shift_buffered {
                    out.push(Out::Pass(ev_key(SHIFT_L, 1)));
                }
                out.push(Out::Pass(ev_key(SUPER_L, 1)));
                out.push(Out::Pass(ev_key(SUPER_L, 0)));
                if self.shift_buffered {
                    self.shift_buffered = false;
                    self.shift_forwarded = true;
                }
                self.state = State::Idle;
                out
            }
            // 组合键注入后松开 Super：不转发，回到空闲。
            (State::Consumed(_), 0) => {
                self.state = State::Idle;
                vec![]
            }
            (_, 0) => {
                self.state = State::Idle;
                vec![Out::Pass(ev)]
            }
            _ => vec![Out::Pass(ev)],
        }
    }

    fn on_shift(&mut self, value: i32, ev: Ev) -> Vec<Out> {
        match (self.state, value) {
            // Super 待定期间按下 Shift：先缓冲，等待是否构成 shift 组合。
            (State::Pending, 1) => {
                self.shift_buffered = true;
                vec![]
            }
            (State::Pending, 2) => vec![],
            // 待定期间松开 Shift：Super 不构成组合键，补发 Super 与 Shift。
            (State::Pending, 0) => {
                let mut out = vec![Out::Pass(ev_key(SUPER_L, 1))];
                if self.shift_buffered {
                    out.push(Out::Pass(ev_key(SHIFT_L, 1)));
                    out.push(Out::Pass(ev_key(SHIFT_L, 0)));
                    self.shift_buffered = false;
                    self.shift_forwarded = false;
                } else {
                    out.push(Out::Pass(ev));
                    self.shift_forwarded = false;
                }
                self.state = State::Forwarded;
                out
            }
            (State::Consumed(_), 0) => {
                if self.shift_buffered {
                    // 缓冲的 Shift 从未放行，松开也一并吞掉。
                    self.shift_buffered = false;
                    vec![]
                } else {
                    self.shift_forwarded = false;
                    vec![Out::Pass(ev)]
                }
            }
            (_, 0) => {
                self.shift_forwarded = false;
                vec![Out::Pass(ev)]
            }
            (_, 1) => {
                self.shift_forwarded = true;
                vec![Out::Pass(ev)]
            }
            _ => vec![],
        }
    }

    fn on_ctrl_alt(&mut self, value: i32, ev: Ev) -> Vec<Out> {
        // Super 待定期间按下 Ctrl/Alt：不是可转换的组合，放行 Super 后原样转发。
        if value == 1 && self.state == State::Pending {
            let mut out = vec![Out::Pass(ev_key(SUPER_L, 1))];
            if self.shift_buffered {
                out.push(Out::Pass(ev_key(SHIFT_L, 1)));
                self.shift_forwarded = true;
                self.shift_buffered = false;
            }
            out.push(Out::Pass(ev));
            self.state = State::Forwarded;
            return out;
        }
        vec![Out::Pass(ev)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binding::{Binding, BindingTable, Mode, Source};
    use crate::key::{Chord, KeyPattern};
    use evdev::KeyCode;

    fn table() -> BindingTable {
        let mk = |id: &str, keys: &str, gui: &str, term: Option<&str>| Binding {
            id: id.to_string(),
            pattern: KeyPattern::parse(keys).unwrap(),
            gui: Some(Chord::parse(gui).unwrap()),
            terminal: term.map(|s| Chord::parse(s).unwrap()),
            enabled: true,
            source: Source::Default,
        };
        BindingTable::new(vec![
            mk("cmd-c", "c", "ctrl+c", Some("ctrl+shift+c")),
            mk("cmd-x", "x", "ctrl+x", None),
            mk("cmd-shift-z", "shift+z", "ctrl+shift+z", None),
        ])
        .unwrap()
    }

    fn k(code: KeyCode, value: i32) -> Ev {
        Ev::Key {
            code: code.0,
            value,
        }
    }

    fn last_inject(outs: &[Out]) -> (&str, &Chord, bool) {
        match outs.last().unwrap() {
            Out::Inject {
                id,
                chord,
                skip_shift,
            } => (id.as_str(), chord, *skip_shift),
            other => panic!("期望 Inject，实际 {other:?}"),
        }
    }

    #[test]
    fn copy_in_gui() {
        let t = table();
        let mut m = Machine::new();
        assert!(m
            .step(k(KeyCode::KEY_LEFTMETA, 1), &t, || Mode::Gui)
            .is_empty());
        let outs = m.step(k(KeyCode::KEY_C, 1), &t, || Mode::Gui);
        assert_eq!(outs.len(), 1);
        let (id, chord, skip) = last_inject(&outs);
        assert_eq!(id, "cmd-c");
        assert_eq!(chord, &Chord::parse("ctrl+c").unwrap());
        assert!(!skip);
    }

    #[test]
    fn copy_in_terminal_uses_shift_variant() {
        let t = table();
        let mut m = Machine::new();
        m.step(k(KeyCode::KEY_LEFTMETA, 1), &t, || Mode::Terminal);
        let outs = m.step(k(KeyCode::KEY_C, 1), &t, || Mode::Terminal);
        let (_, chord, _) = last_inject(&outs);
        assert_eq!(chord, &Chord::parse("ctrl+shift+c").unwrap());
    }

    #[test]
    fn terminal_without_mapping_passes_through() {
        let t = table();
        let mut m = Machine::new();
        m.step(k(KeyCode::KEY_LEFTMETA, 1), &t, || Mode::Terminal);
        let outs = m.step(k(KeyCode::KEY_X, 1), &t, || Mode::Terminal);
        assert_eq!(outs[0], Out::Pass(k(KeyCode::KEY_LEFTMETA, 1)));
        assert_eq!(outs[1], Out::Pass(k(KeyCode::KEY_X, 1)));
    }

    #[test]
    fn unbound_super_combo_passes_through() {
        let t = table();
        let mut m = Machine::new();
        m.step(k(KeyCode::KEY_LEFTMETA, 1), &t, || Mode::Gui);
        let outs = m.step(k(KeyCode::KEY_E, 1), &t, || Mode::Gui);
        assert_eq!(outs[0], Out::Pass(k(KeyCode::KEY_LEFTMETA, 1)));
        assert_eq!(outs[1], Out::Pass(k(KeyCode::KEY_E, 1)));
    }

    #[test]
    fn super_alone_emits_click() {
        let t = table();
        let mut m = Machine::new();
        m.step(k(KeyCode::KEY_LEFTMETA, 1), &t, || Mode::Gui);
        let outs = m.step(k(KeyCode::KEY_LEFTMETA, 0), &t, || Mode::Gui);
        assert_eq!(
            outs,
            vec![
                Out::Pass(k(KeyCode::KEY_LEFTMETA, 1)),
                Out::Pass(k(KeyCode::KEY_LEFTMETA, 0)),
            ]
        );
    }

    #[test]
    fn plain_ctrl_c_passes_untouched() {
        let t = table();
        let mut m = Machine::new();
        assert_eq!(
            m.step(k(KeyCode::KEY_LEFTCTRL, 1), &t, || Mode::Gui),
            vec![Out::Pass(k(KeyCode::KEY_LEFTCTRL, 1))]
        );
        assert_eq!(
            m.step(k(KeyCode::KEY_C, 1), &t, || Mode::Gui),
            vec![Out::Pass(k(KeyCode::KEY_C, 1))]
        );
        assert_eq!(
            m.step(k(KeyCode::KEY_C, 0), &t, || Mode::Gui),
            vec![Out::Pass(k(KeyCode::KEY_C, 0))]
        );
    }

    #[test]
    fn other_keys_pass_in_idle() {
        let t = table();
        let mut m = Machine::new();
        assert_eq!(
            m.step(k(KeyCode::KEY_A, 1), &t, || Mode::Gui),
            vec![Out::Pass(k(KeyCode::KEY_A, 1))]
        );
    }

    #[test]
    fn shift_after_super_matches_shift_binding() {
        let t = table();
        let mut m = Machine::new();
        m.step(k(KeyCode::KEY_LEFTMETA, 1), &t, || Mode::Gui);
        assert!(m
            .step(k(KeyCode::KEY_LEFTSHIFT, 1), &t, || Mode::Gui)
            .is_empty());
        let outs = m.step(k(KeyCode::KEY_Z, 1), &t, || Mode::Gui);
        let (id, chord, skip) = last_inject(&outs);
        assert_eq!(id, "cmd-shift-z");
        assert_eq!(chord, &Chord::parse("ctrl+shift+z").unwrap());
        assert!(!skip);
    }

    #[test]
    fn shift_before_super_is_forwarded_and_not_reinjected() {
        let t = table();
        let mut m = Machine::new();
        assert_eq!(
            m.step(k(KeyCode::KEY_LEFTSHIFT, 1), &t, || Mode::Gui),
            vec![Out::Pass(k(KeyCode::KEY_LEFTSHIFT, 1))]
        );
        m.step(k(KeyCode::KEY_LEFTMETA, 1), &t, || Mode::Gui);
        let outs = m.step(k(KeyCode::KEY_Z, 1), &t, || Mode::Gui);
        let (id, _, skip) = last_inject(&outs);
        assert_eq!(id, "cmd-shift-z");
        assert!(skip);
    }

    #[test]
    fn mapped_key_release_and_repeat_are_swallowed() {
        let t = table();
        let mut m = Machine::new();
        m.step(k(KeyCode::KEY_LEFTMETA, 1), &t, || Mode::Gui);
        m.step(k(KeyCode::KEY_C, 1), &t, || Mode::Gui);
        assert!(m.step(k(KeyCode::KEY_C, 2), &t, || Mode::Gui).is_empty());
        assert!(m.step(k(KeyCode::KEY_C, 0), &t, || Mode::Gui).is_empty());
        assert!(m
            .step(k(KeyCode::KEY_LEFTMETA, 0), &t, || Mode::Gui)
            .is_empty());
        assert_eq!(
            m.step(k(KeyCode::KEY_C, 1), &t, || Mode::Gui)[0],
            Out::Pass(k(KeyCode::KEY_C, 1))
        );
    }

    #[test]
    fn buffered_shift_release_after_injection_is_swallowed() {
        let t = table();
        let mut m = Machine::new();
        m.step(k(KeyCode::KEY_LEFTMETA, 1), &t, || Mode::Gui);
        m.step(k(KeyCode::KEY_LEFTSHIFT, 1), &t, || Mode::Gui);
        m.step(k(KeyCode::KEY_Z, 1), &t, || Mode::Gui);
        assert!(m.step(k(KeyCode::KEY_Z, 0), &t, || Mode::Gui).is_empty());
        assert!(m
            .step(k(KeyCode::KEY_LEFTSHIFT, 0), &t, || Mode::Gui)
            .is_empty());
    }

    #[test]
    fn shift_release_flushes_pending_super() {
        let t = table();
        let mut m = Machine::new();
        m.step(k(KeyCode::KEY_LEFTMETA, 1), &t, || Mode::Gui);
        m.step(k(KeyCode::KEY_LEFTSHIFT, 1), &t, || Mode::Gui);
        let outs = m.step(k(KeyCode::KEY_LEFTSHIFT, 0), &t, || Mode::Gui);
        assert_eq!(
            outs,
            vec![
                Out::Pass(k(KeyCode::KEY_LEFTMETA, 1)),
                Out::Pass(k(KeyCode::KEY_LEFTSHIFT, 1)),
                Out::Pass(k(KeyCode::KEY_LEFTSHIFT, 0)),
            ]
        );
    }

    #[test]
    fn repeat_press_of_same_key_while_super_held_reinjects() {
        let t = table();
        let mut m = Machine::new();
        m.step(k(KeyCode::KEY_LEFTMETA, 1), &t, || Mode::Gui);
        m.step(k(KeyCode::KEY_C, 1), &t, || Mode::Gui);
        assert!(m.step(k(KeyCode::KEY_C, 0), &t, || Mode::Gui).is_empty());
        let outs = m.step(k(KeyCode::KEY_C, 1), &t, || Mode::Gui);
        let (id, chord, _) = last_inject(&outs);
        assert_eq!(id, "cmd-c");
        assert_eq!(chord, &Chord::parse("ctrl+c").unwrap());
        assert!(m.step(k(KeyCode::KEY_C, 0), &t, || Mode::Gui).is_empty());
    }
}
