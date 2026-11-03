use mpx::tui::style::{Style, apply_directive, over_colours, parse};
use mpx::vt::Color;

#[test]
fn colours_come_as_names_indexes_and_hex() {
    let s = parse("fg=red,bg=colour200").unwrap();
    assert_eq!(s.fg, Some(Color::Red));
    assert_eq!(s.bg, Some(Color::Indexed(200)));
    let s = parse("fg=#102030").unwrap();
    assert_eq!(s.fg, Some(Color::Rgb(0x10, 0x20, 0x30)));
}

#[test]
fn attributes_and_their_no_forms() {
    let s = parse("bold,dim,underscore,italics,reverse").unwrap();
    assert!(s.bold && s.dim && s.underscore && s.italics && s.reverse);
    let s = parse("bold,nobold,italics,noitalics").unwrap();
    assert!(!s.bold && !s.italics);
}

#[test]
fn default_resets_the_style_and_items_apply_in_order() {
    let s = parse("fg=red,bold,default,bg=blue").unwrap();
    assert_eq!(
        s,
        Style {
            bg: Some(Color::Blue),
            ..Style::default()
        }
    );
    assert_eq!(parse("fg=default").unwrap().fg, None);
}

#[test]
fn unsupported_and_malformed_items_are_errors() {
    assert!(
        parse("blink")
            .unwrap_err()
            .contains("'blink' is not a style item")
    );
    assert!(parse("fg=notacolour").unwrap_err().contains("not a colour"));
    assert!(parse("align=right").is_err());
}

#[test]
fn directives_skip_items_mpx_cannot_draw() {
    let mut s = Style::default();
    apply_directive(&mut s, "fg=green,align=right,bold,list=on");
    assert_eq!(s.fg, Some(Color::Green));
    assert!(s.bold);
}

#[test]
fn over_lays_own_colours_and_adds_attributes() {
    let base = parse("bg=blue,underscore").unwrap();
    let own = parse("fg=red,bold").unwrap();
    let merged = base.over(&own);
    assert_eq!(merged.fg, Some(Color::Red));
    assert_eq!(merged.bg, Some(Color::Blue));
    assert!(merged.bold && merged.underscore);
}

#[test]
fn configured_colours_survive_a_default_item() {
    let s = over_colours("green", "black", "fg=default,bold").unwrap();
    assert_eq!(s.fg, Some(Color::Green));
    assert_eq!(s.bg, Some(Color::Black));
    assert!(s.bold);
}
