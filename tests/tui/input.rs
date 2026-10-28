use mpx::tui::command::Command;
use mpx::tui::input::{OverlayNext, overlay_next};
use mpx::tui::overlay::OverlayAction;

#[test]
fn overlay_that_ignores_the_key_stays_open() {
    assert_eq!(overlay_next(OverlayAction::None), OverlayNext::Stay);
}

#[test]
fn overlay_that_closes_runs_nothing() {
    assert_eq!(
        overlay_next(OverlayAction::Close),
        OverlayNext::Close(Vec::new())
    );
}

#[test]
fn overlay_run_closes_and_hands_back_its_commands_in_order() {
    let cmds = vec![Command::Detach, Command::NextWindow];
    assert_eq!(
        overlay_next(OverlayAction::Run(cmds.clone())),
        OverlayNext::Close(cmds)
    );
}
