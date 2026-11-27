use anyhow::{Context, Result};
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode};

use crate::state::{Ev, Out};

/// 事件下游：真实 uinput 虚拟设备，或仅打印的 dry-run 模式。
pub enum Sink {
    Real(VirtualDevice),
    Dry,
}

impl Sink {
    /// 创建名为 cmd2ctrl 的虚拟键盘，仅暴露给定按键能力。
    pub fn real(keys: &AttributeSet<KeyCode>) -> Result<Self> {
        let dev = VirtualDevice::builder()?
            .name("cmd2ctrl virtual keyboard")
            .with_keys(keys)?
            .build()
            .context("创建 uinput 设备失败（检查 /dev/uinput 权限与 input 组）")?;
        Ok(Self::Real(dev))
    }

    pub fn dry() -> Self {
        Self::Dry
    }

    /// 将状态机输出写入下游；dry-run 下仅打印命中信息。
    pub fn emit(&mut self, out: &Out) -> Result<()> {
        if let Sink::Dry = self {
            if let Out::Inject {
                id,
                chord,
                skip_shift,
            } = out
            {
                println!(
                    "[dry-run] 命中 {id} -> {} (skip_shift={skip_shift})",
                    chord.label()
                );
            }
            return Ok(());
        }
        let events = events_for(out);
        if events.is_empty() {
            return Ok(());
        }
        let batch: Vec<InputEvent> = events
            .iter()
            .map(|(t, c, v)| InputEvent::new(*t, *c, *v))
            .collect();
        if let Sink::Real(dev) = self {
            dev.emit(&batch)?;
        }
        Ok(())
    }
}

/// 把状态机输出展开为 (type, code, value) 序列；uinput 的 emit 会自动补 SYN_REPORT。
pub fn events_for(out: &Out) -> Vec<(u16, u16, i32)> {
    match out {
        Out::Pass(Ev::Key { code, value }) => vec![(EventType::KEY.0, *code, *value)],
        Out::Inject {
            chord, skip_shift, ..
        } => chord
            .events(*skip_shift)
            .into_iter()
            .map(|(c, v)| (EventType::KEY.0, c, v))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::Chord;
    use crate::state::{Ev, Out};
    use evdev::{EventType, KeyCode};

    #[test]
    fn key_pass_event() {
        let out = Out::Pass(Ev::Key {
            code: KeyCode::KEY_A.0,
            value: 1,
        });
        assert_eq!(
            events_for(&out),
            vec![(EventType::KEY.0, KeyCode::KEY_A.0, 1)]
        );
    }

    #[test]
    fn inject_expands_chord() {
        let out = Out::Inject {
            id: "cmd-c".to_string(),
            chord: Chord::parse("ctrl+shift+c").unwrap(),
            skip_shift: false,
        };
        assert_eq!(
            events_for(&out),
            vec![
                (EventType::KEY.0, KeyCode::KEY_LEFTCTRL.0, 1),
                (EventType::KEY.0, KeyCode::KEY_LEFTSHIFT.0, 1),
                (EventType::KEY.0, KeyCode::KEY_C.0, 1),
                (EventType::KEY.0, KeyCode::KEY_C.0, 0),
                (EventType::KEY.0, KeyCode::KEY_LEFTSHIFT.0, 0),
                (EventType::KEY.0, KeyCode::KEY_LEFTCTRL.0, 0),
            ]
        );
    }

    #[test]
    fn inject_skip_shift_omits_shift() {
        let out = Out::Inject {
            id: "cmd-shift-z".to_string(),
            chord: Chord::parse("ctrl+shift+z").unwrap(),
            skip_shift: true,
        };
        assert_eq!(
            events_for(&out),
            vec![
                (EventType::KEY.0, KeyCode::KEY_LEFTCTRL.0, 1),
                (EventType::KEY.0, KeyCode::KEY_Z.0, 1),
                (EventType::KEY.0, KeyCode::KEY_Z.0, 0),
                (EventType::KEY.0, KeyCode::KEY_LEFTCTRL.0, 0),
            ]
        );
    }
}
