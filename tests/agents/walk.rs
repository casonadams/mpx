use mpx::agents::{Visit, visit_entry};

#[test]
fn directories_are_entered_only_above_the_session_depth() {
    assert_eq!(visit_entry(true, false, false, 0, 2), Visit::Recurse);
    assert_eq!(visit_entry(true, false, false, 1, 2), Visit::Recurse);
    assert_eq!(visit_entry(true, false, false, 2, 2), Visit::Skip);
}

#[test]
fn transcripts_are_kept_exactly_at_the_session_depth() {
    assert_eq!(visit_entry(false, true, true, 1, 1), Visit::Keep);
    assert_eq!(visit_entry(false, true, true, 0, 1), Visit::Skip);
    assert_eq!(visit_entry(false, true, true, 2, 1), Visit::Skip);
}

#[test]
fn other_files_are_skipped_even_at_the_session_depth() {
    assert_eq!(visit_entry(false, true, false, 1, 1), Visit::Skip);
}

#[test]
fn symlinks_and_other_entries_are_skipped() {
    assert_eq!(visit_entry(false, false, true, 1, 1), Visit::Skip);
}
