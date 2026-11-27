use evdev::{AttributeSetRef, KeyCode};

/// 键盘能力判定：必须具备字母、空格与回车，排除电源键、纯媒体键等设备。
pub fn is_keyboard(keys: &AttributeSetRef<KeyCode>) -> bool {
    keys.contains(KeyCode::KEY_A)
        && keys.contains(KeyCode::KEY_Z)
        && keys.contains(KeyCode::KEY_SPACE)
        && keys.contains(KeyCode::KEY_ENTER)
}

/// include 为空表示全部；exclude 优先于 include；不区分大小写，子串匹配。
pub fn matches_filters(name: &str, include: &[String], exclude: &[String]) -> bool {
    let n = name.to_ascii_lowercase();
    if exclude.iter().any(|p| n.contains(&p.to_ascii_lowercase())) {
        return false;
    }
    include.is_empty() || include.iter().any(|p| n.contains(&p.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use evdev::{AttributeSet, KeyCode};

    fn keyboard_keys() -> AttributeSet<KeyCode> {
        let mut s = AttributeSet::new();
        for k in [
            KeyCode::KEY_A,
            KeyCode::KEY_Z,
            KeyCode::KEY_SPACE,
            KeyCode::KEY_ENTER,
        ] {
            s.insert(k);
        }
        s
    }

    #[test]
    fn real_keyboard_is_detected() {
        assert!(is_keyboard(&keyboard_keys()));
    }

    #[test]
    fn power_button_is_not_keyboard() {
        let mut s = AttributeSet::new();
        s.insert(KeyCode::KEY_POWER);
        assert!(!is_keyboard(&s));
    }

    #[test]
    fn partial_keys_not_keyboard() {
        let mut s = AttributeSet::new();
        s.insert(KeyCode::KEY_A);
        s.insert(KeyCode::KEY_Z);
        assert!(!is_keyboard(&s));
    }

    #[test]
    fn filters_are_case_insensitive_substrings() {
        let inc = vec!["Magic Keyboard".to_string()];
        let exc = vec!["trackpad".to_string()];
        assert!(matches_filters("Apple Magic Keyboard", &inc, &exc));
        assert!(!matches_filters("Apple Magic Trackpad", &inc, &exc));
        assert!(!matches_filters("Dell Keyboard", &inc, &exc));
        assert!(matches_filters("Anything", &[], &[]));
    }

    #[test]
    fn exclude_wins_over_include() {
        let inc = vec!["keyboard".to_string()];
        let exc = vec!["virtual".to_string()];
        assert!(!matches_filters("virtual keyboard", &inc, &exc));
    }
}
