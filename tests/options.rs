use mpx::config::{Config, Justify, ModeKeys, StatusPosition};
use mpx::options;

#[test]
fn set_parses_each_kind_of_value() {
    let mut c = Config::default();
    options::set(&mut c, "mouse", "off").unwrap();
    options::set(&mut c, "base-index", "1").unwrap();
    options::set(&mut c, "status-position", "top").unwrap();
    options::set(&mut c, "status-justify", "center").unwrap();
    options::set(&mut c, "mode-keys", "emacs").unwrap();
    options::set(&mut c, "status-left", "[#S] ").unwrap();
    assert!(!c.mouse);
    assert_eq!(c.base_index, 1);
    assert_eq!(c.status_position, StatusPosition::Top);
    assert_eq!(c.status_justify, Justify::Centre);
    assert_eq!(c.mode_keys, ModeKeys::Emacs);
    assert_eq!(c.status_left, "[#S] ");
}

#[test]
fn a_bad_value_names_the_option_and_changes_nothing() {
    let mut c = Config::default();
    let err = options::set(&mut c, "status-interval", "soon").unwrap_err();
    assert!(err.starts_with("status-interval:"), "{err}");
    assert!(options::set(&mut c, "mouse", "maybe").is_err());
    assert_eq!(c, Config::default());
}

#[test]
fn unknown_and_daemon_options_are_told_apart() {
    let mut c = Config::default();
    assert_eq!(options::set(&mut c, "no-such", "1").unwrap_err(), "unknown option: no-such");
    let err = options::set(&mut c, "remain-on-exit", "on").unwrap_err();
    assert!(err.contains("reload-config"), "{err}");
    assert!(options::is_known("remain-on-exit") && options::is_known("mouse"));
    assert!(!options::is_known("no-such"));
}

#[test]
fn set_then_unset_restores_the_default_for_switches_and_strings() {
    assert_eq!(options::get(&Config::default(), "wrap-search").unwrap(), "on");
    // (option, a value that changes it, how `get` shows that value)
    for (name, value, shown) in [("wrap-search", "off", "off"), ("status-left", "", "\"\"")] {
        let mut c = Config::default();
        let default = options::get(&c, name).unwrap();
        options::set(&mut c, name, value).unwrap();
        assert_ne!(c, Config::default(), "{name} changed");
        assert_eq!(options::get(&c, name).unwrap(), shown);
        options::unset(&mut c, name).unwrap();
        assert_eq!(c, Config::default(), "{name} restored");
        assert_eq!(options::get(&c, name).unwrap(), default);
    }
}

#[test]
fn get_and_list_print_values_as_tmux_does() {
    let mut c = Config::default();
    options::set(&mut c, "status-left", "a b").unwrap();
    options::set(&mut c, "set-clipboard", "off").unwrap();
    assert_eq!(options::get(&c, "status-left").unwrap(), "\"a b\"");
    assert_eq!(options::get(&c, "set-clipboard").unwrap(), "off");
    assert_eq!(options::get(&c, "status-left-length").unwrap(), "10");
    assert_eq!(options::get(&c, "prefix").unwrap(), "C-b");
    let names: Vec<_> = options::list(&c).into_iter().map(|(n, _)| n).collect();
    assert!(names.windows(2).all(|w| w[0] < w[1]), "sorted: {names:?}");
}
