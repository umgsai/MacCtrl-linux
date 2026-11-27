use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::binding::{Binding, Source};
use crate::key::{Chord, KeyPattern};

/// 未配置终端类名时使用的默认终端窗口类列表（X11）。
pub const DEFAULT_TERMINAL_CLASSES: &[&str] = &[
    "deepin-terminal", "gnome-terminal", "kgx", "konsole", "xfce4-terminal",
    "xterm", "urxvt", "alacritty", "kitty", "wezterm", "tilix",
    "terminator", "st", "mate-terminal", "lxterminal", "qterminal", "sakura",
];

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub device: DeviceConfig,
    #[serde(default)]
    pub x11: X11Config,
    #[serde(default)]
    pub bindings: BTreeMap<String, BindingOverride>,
}

#[derive(Debug, Default, Deserialize)]
pub struct DeviceConfig {
    #[serde(default)]
    pub include_patterns: Vec<String>,
    #[serde(default)]
    pub exclude_patterns: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct X11Config {
    #[serde(default)]
    pub terminal_classes: Vec<String>,
}

/// 针对单条绑定的配置覆盖项。
#[derive(Debug, Default, Deserialize)]
pub struct BindingOverride {
    pub enabled: Option<bool>,
    pub gui: Option<String>,
    pub terminal: Option<String>,
    pub keys: Option<String>,
}

impl Config {
    /// 从给定路径或默认路径加载配置；文件不存在时返回默认配置。
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let Some(path) = path else {
            let default = default_path();
            if default.exists() {
                return Self::load_file(&default);
            }
            return Ok(Self::default());
        };
        Self::load_file(path)
    }

    fn load_file(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("读取配置失败: {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("解析配置失败: {}", path.display()))
    }

    /// 解析 TOML 文本；空文本视为空配置。
    pub fn parse(text: &str) -> Result<Self> {
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        Ok(toml::from_str(text)?)
    }

    /// 返回终端类名列表；未配置时回落到内置默认值。
    pub fn terminal_classes(&self) -> Vec<String> {
        if self.x11.terminal_classes.is_empty() {
            DEFAULT_TERMINAL_CLASSES.iter().map(|s| s.to_string()).collect()
        } else {
            self.x11.terminal_classes.iter().map(|s| s.to_ascii_lowercase()).collect()
        }
    }

    /// 将配置合并到内置绑定列表：覆盖已有绑定或追加自定义绑定。
    pub fn apply(&self, builtin: Vec<Binding>) -> Result<Vec<Binding>> {
        let mut list = builtin;
        for (id, ov) in &self.bindings {
            let idx = list.iter().position(|b| &b.id == id);
            match idx {
                Some(i) => {
                    if ov.keys.is_some() {
                        bail!("内置绑定 {id} 不支持覆盖 keys（如需新组合请使用新的 id）");
                    }
                    let b = &mut list[i];
                    if let Some(enabled) = ov.enabled {
                        b.enabled = enabled;
                    }
                    if let Some(gui) = &ov.gui {
                        b.gui = if gui.is_empty() {
                            None
                        } else {
                            Some(Chord::parse(gui).with_context(|| format!("绑定 {id} 的 gui 非法"))?)
                        };
                    }
                    if let Some(terminal) = &ov.terminal {
                        b.terminal = if terminal.is_empty() {
                            None
                        } else {
                            Some(
                                Chord::parse(terminal)
                                    .with_context(|| format!("绑定 {id} 的 terminal 非法"))?,
                            )
                        };
                    }
                    b.source = Source::Config;
                }
                None => {
                    let keys = ov
                        .keys
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("自定义绑定 {id} 缺少 keys"))?;
                    let gui = ov
                        .gui
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("自定义绑定 {id} 缺少 gui"))?;
                    let pattern = KeyPattern::parse(keys)
                        .with_context(|| format!("绑定 {id} 的 keys 非法"))?;
                    let gui = Chord::parse(gui).with_context(|| format!("绑定 {id} 的 gui 非法"))?;
                    let terminal = match &ov.terminal {
                        Some(s) if s.is_empty() => None,
                        Some(s) => Some(
                            Chord::parse(s).with_context(|| format!("绑定 {id} 的 terminal 非法"))?,
                        ),
                        None => None,
                    };
                    list.push(Binding {
                        id: id.clone(),
                        pattern,
                        gui: Some(gui),
                        terminal,
                        enabled: ov.enabled.unwrap_or(true),
                        source: Source::Config,
                    });
                }
            }
        }
        Ok(list)
    }

    /// 应用命令行显式启用/禁用覆盖；命令行优先级最高。
    pub fn apply_cli(&self, list: &mut [Binding], enable: &[String], disable: &[String]) -> Result<()> {
        for id in enable {
            let b = list
                .iter_mut()
                .find(|b| &b.id == id)
                .ok_or_else(|| anyhow::anyhow!("未知绑定 id: {id}"))?;
            b.enabled = true;
            b.source = Source::Cli;
        }
        for id in disable {
            let b = list
                .iter_mut()
                .find(|b| &b.id == id)
                .ok_or_else(|| anyhow::anyhow!("未知绑定 id: {id}"))?;
            b.enabled = false;
            b.source = Source::Cli;
        }
        Ok(())
    }
}

/// 默认配置文件路径：$XDG_CONFIG_HOME/cmd2ctrl/config.toml。
pub fn default_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    base.join("cmd2ctrl").join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binding::{builtin_bindings, Binding, Mode, Source};
    use crate::key::Chord;

    fn parse(s: &str) -> Config {
        Config::parse(s).unwrap()
    }

    fn applied(cfg: &Config) -> Vec<Binding> {
        cfg.apply(builtin_bindings()).unwrap()
    }

    fn get(list: &[Binding], id: &str) -> Binding {
        list.iter().find(|b| b.id == id).unwrap().clone()
    }

    #[test]
    fn empty_config_keeps_defaults() {
        let list = applied(&parse(""));
        assert!(get(&list, "cmd-c").enabled);
        assert!(!get(&list, "cmd-d").enabled);
        assert_eq!(get(&list, "cmd-c").source, Source::Default);
    }

    #[test]
    fn enable_disabled_binding() {
        let list = applied(&parse("[bindings.cmd-d]\nenabled = true\n"));
        let d = get(&list, "cmd-d");
        assert!(d.enabled);
        assert_eq!(d.source, Source::Config);
        assert_eq!(d.target(Mode::Gui), Some(&Chord::parse("ctrl+d").unwrap()));
    }

    #[test]
    fn disable_enabled_binding() {
        let list = applied(&parse("[bindings.cmd-b]\nenabled = false\n"));
        assert!(!get(&list, "cmd-b").enabled);
    }

    #[test]
    fn override_chord_and_clear_terminal() {
        let list = applied(&parse(
            "[bindings.cmd-left]\nenabled = true\ngui = \"ctrl+left\"\nterminal = \"\"\n",
        ));
        let left = get(&list, "cmd-left");
        assert!(left.enabled);
        assert_eq!(left.gui, Some(Chord::parse("ctrl+left").unwrap()));
        assert_eq!(left.terminal, None);
    }

    #[test]
    fn add_custom_binding() {
        let list = applied(&parse(
            "[bindings.cmd-shift-k]\nkeys = \"shift+k\"\ngui = \"ctrl+shift+k\"\nterminal = \"\"\n",
        ));
        let custom = get(&list, "cmd-shift-k");
        assert!(custom.enabled);
        assert_eq!(custom.pattern.label(), "super+shift+k");
        assert_eq!(custom.gui, Some(Chord::parse("ctrl+shift+k").unwrap()));
        assert_eq!(custom.terminal, None);
    }

    #[test]
    fn custom_binding_requires_keys_and_gui() {
        assert!(Config::parse("[bindings.foo]\nenabled = true\n")
            .unwrap()
            .apply(builtin_bindings())
            .is_err());
        assert!(Config::parse("[bindings.foo]\nkeys = \"k\"\n")
            .unwrap()
            .apply(builtin_bindings())
            .is_err());
    }

    #[test]
    fn builtin_keys_cannot_be_overridden() {
        let err = Config::parse("[bindings.cmd-c]\nkeys = \"shift+c\"\n")
            .unwrap()
            .apply(builtin_bindings())
            .unwrap_err()
            .to_string();
        assert!(err.contains("cmd-c"), "{err}");
    }

    #[test]
    fn bad_chord_reports_id() {
        let err = Config::parse("[bindings.cmd-shift-k]\nkeys = \"shift+k\"\ngui = \"ctrl+nope\"\n")
            .unwrap()
            .apply(builtin_bindings())
            .unwrap_err()
            .to_string();
        assert!(err.contains("cmd-shift-k"), "{err}");
    }

    #[test]
    fn cli_overrides_win() {
        let cfg = parse("[bindings.cmd-c]\nenabled = false\n");
        let mut list = cfg.apply(builtin_bindings()).unwrap();
        cfg.apply_cli(&mut list, &["cmd-c".to_string()], &[]).unwrap();
        let c = get(&list, "cmd-c");
        assert!(c.enabled);
        assert_eq!(c.source, Source::Cli);
        assert!(cfg.apply_cli(&mut list, &[], &["nosuch".to_string()]).is_err());
    }

    #[test]
    fn default_terminal_classes_used_when_empty() {
        assert!(parse("").terminal_classes().iter().any(|c| c == "deepin-terminal"));
        let cfg = parse("[x11]\nterminal_classes = [\"myterm\"]\n");
        assert_eq!(cfg.terminal_classes(), vec!["myterm".to_string()]);
    }
}
