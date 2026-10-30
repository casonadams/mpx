use mpx::config::BorderStyle;
use mpx::protocol::{PopupAnchor, PopupExit, PopupOpts, PopupSize};
use mpx::tmux_shim::{parse_invocation, popup_opts};

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

fn opts(words: &[&str]) -> Result<PopupOpts, String> {
    let inv = parse_invocation(&argv(words))?;
    popup_opts(&inv, true)
}

#[test]
fn defaults_are_a_bordered_half_size_popup_centred_on_the_screen() {
    let popup = opts(&["display-popup", "cat"]).expect("valid");
    let spec = &popup.spec;
    assert_eq!(spec.width, PopupSize::Percent(50));
    assert_eq!(spec.height, PopupSize::Percent(50));
    assert_eq!(spec.x, PopupAnchor::Centre);
    assert_eq!(spec.y, PopupAnchor::Centre);
    assert!(spec.border);
    assert_eq!(spec.lines, None, "the pane border lines are the default");
    assert_eq!(spec.border_color, None);
    assert_eq!(popup.exit, PopupExit::Stay);
    assert!(popup.wait);
    assert_eq!(
        popup.spawn.command, "sh",
        "the command runs through sh -c, as tmux runs it"
    );
    assert_eq!(popup.spawn.args, vec!["-c", "cat"]);
}

#[test]
fn the_command_is_run_through_sh_when_given_as_words() {
    let popup = opts(&["display-popup", "echo hi", "more"]).expect("valid");
    assert_eq!(popup.spawn.command, "sh");
    assert_eq!(popup.spawn.args, vec!["-c", "echo hi more"]);
}

#[test]
fn no_command_leaves_the_program_to_the_default_command() {
    let popup = opts(&["display-popup"]).expect("valid");
    assert!(popup.spawn.command.is_empty());
}

#[test]
fn one_e_closes_on_exit_and_two_e_only_on_success() {
    assert_eq!(
        opts(&["display-popup", "-E", "x"]).unwrap().exit,
        PopupExit::Close
    );
    assert_eq!(
        opts(&["display-popup", "-EE", "x"]).unwrap().exit,
        PopupExit::CloseOnSuccess
    );
}

#[test]
fn sizes_are_cells_or_percentages_and_zero_is_refused() {
    let popup = opts(&["display-popup", "-w", "40", "-h", "30%", "x"]).expect("valid");
    assert_eq!(popup.spec.width, PopupSize::Cells(40));
    assert_eq!(popup.spec.height, PopupSize::Percent(30));
    assert!(opts(&["display-popup", "-w", "0", "x"]).is_err());
    assert!(opts(&["display-popup", "-h", "150%", "x"]).is_err());
    assert!(opts(&["display-popup", "-w", "wide", "x"]).is_err());
}

#[test]
fn positions_name_centre_edges_pane_or_cells() {
    let popup = opts(&["display-popup", "-x", "R", "-y", "P", "x"]).expect("valid");
    assert_eq!(popup.spec.x, PopupAnchor::End);
    assert_eq!(popup.spec.y, PopupAnchor::Pane);
    let popup = opts(&["display-popup", "-x", "7", "-y", "B", "x"]).expect("valid");
    assert_eq!(popup.spec.x, PopupAnchor::Cells(7));
    assert_eq!(popup.spec.y, PopupAnchor::End);
    assert_eq!(
        opts(&["display-popup", "-x", "T", "x"])
            .map(|p| p.spec.x)
            .ok(),
        None,
        "T is a vertical edge, not a horizontal one"
    );
}

#[test]
fn b_and_capital_b_control_the_border() {
    assert!(!opts(&["display-popup", "-B", "x"]).unwrap().spec.border);
    assert!(
        !opts(&["display-popup", "-b", "none", "x"])
            .unwrap()
            .spec
            .border
    );
    assert_eq!(
        opts(&["display-popup", "-b", "rounded", "x"])
            .unwrap()
            .spec
            .lines,
        Some(BorderStyle::Rounded)
    );
    assert_eq!(
        opts(&["display-popup", "-b", "simple", "x"])
            .unwrap()
            .spec
            .lines,
        Some(BorderStyle::Ascii)
    );
    assert!(opts(&["display-popup", "-b", "heavy", "x"]).is_err());
}

#[test]
fn border_colour_is_the_fg_of_the_border_style() {
    let popup = opts(&["display-popup", "-S", "fg=#3fb950,bg=black", "x"]).unwrap();
    assert_eq!(popup.spec.border_color.as_deref(), Some("#3fb950"));
}

#[test]
fn repeated_e_sets_each_variable_and_needs_an_equals_sign() {
    let popup = opts(&["display-popup", "-e", "A=1", "-e", "B=two words", "x"]).unwrap();
    assert_eq!(
        popup.spawn.env,
        vec![
            ("A".to_string(), "1".to_string()),
            ("B".to_string(), "two words".to_string())
        ]
    );
    assert!(opts(&["display-popup", "-e", "NOVALUE", "x"]).is_err());
}

#[test]
fn start_directory_is_d() {
    let popup = opts(&["display-popup", "-d", "/tmp/work", "x"]).unwrap();
    assert_eq!(popup.spawn.cwd.as_deref(), Some("/tmp/work"));
}

#[test]
fn wait_is_the_callers_choice() {
    let inv = parse_invocation(&argv(&["display-popup", "-E", "x"])).unwrap();
    assert!(!popup_opts(&inv, false).unwrap().wait);
    assert!(popup_opts(&inv, true).unwrap().wait);
}
