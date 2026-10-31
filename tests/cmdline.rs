//! Command strings: splitting into commands and words.
use mpx::cmdline::split_commands;

fn words(line: &str) -> Vec<Vec<String>> {
    split_commands(line).unwrap()
}

#[test]
fn command_strings_split_on_unescaped_semicolons_and_respect_quotes() {
    assert_eq!(
        words("set -g status off ; split-window -h"),
        [
            vec!["set", "-g", "status", "off"],
            vec!["split-window", "-h"]
        ]
    );
    assert_eq!(
        words("display 'a ; b' \"c d\" e\\;f"),
        [vec!["display", "a ; b", "c d", "e;f"]]
    );
    assert_eq!(
        words("send-keys \"say \\\"hi\\\"\";"),
        [vec!["send-keys", "say \"hi\""]]
    );
    assert!(words("  ;; ").is_empty());
    assert_eq!(words("a '' b"), [vec!["a", "", "b"]]);
    assert!(split_commands("echo 'open").is_err());
    assert!(split_commands("echo \"open").is_err());
}

#[test]
fn double_quotes_unescape_only_quote_backslash_and_dollar() {
    assert_eq!(
        words(r#"echo "a\"b" "c\\d" "e\$f" "g\nh""#),
        [vec!["echo", "a\"b", "c\\d", "e$f", "g\\nh"]]
    );
    assert_eq!(words(r"echo a\ b"), [vec!["echo", "a b"]]);
}
