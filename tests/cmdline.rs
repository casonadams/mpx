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

#[test]
fn unquoted_backslash_before_an_ordinary_character_is_kept() {
    assert_eq!(
        words(r"load-buffer C:\Users\RUNNER~1\in.txt"),
        [vec!["load-buffer", r"C:\Users\RUNNER~1\in.txt"]]
    );
    assert_eq!(words(r"echo a\nb \#"), [vec!["echo", r"a\nb", r"\#"]]);
    assert_eq!(words(r"echo trailing\"), [vec!["echo", r"trailing\"]]);
}

#[test]
fn unquoted_backslash_still_escapes_quotes_backslashes_semicolons_and_spaces() {
    assert_eq!(words(r"echo a\\b"), [vec!["echo", r"a\b"]]);
    assert_eq!(words(r#"echo \"hi\""#), [vec!["echo", "\"hi\""]]);
    assert_eq!(words(r"echo \'x\'"), [vec!["echo", "'x'"]]);
    assert_eq!(words(r"a\;b ; c"), [vec!["a;b"], vec!["c"]]);
    assert_eq!(words(r"echo a\ b"), [vec!["echo", "a b"]]);
}
