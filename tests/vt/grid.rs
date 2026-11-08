use mpx::vt::grid::ShellMark;
use mpx::vt::{Cell, CellStyle, Color, Grid, HistoryRow, ScrollHint};

fn write_str(grid: &mut Grid, text: &str) {
    for c in text.chars() {
        grid.put_char(c);
    }
}

/// A grid whose rows read `r0`, `r1`, ... with the cursor left on the last row.
fn numbered(cols: usize, rows: usize) -> Grid {
    let mut grid = Grid::new(cols, rows);
    for i in 0..rows {
        grid.move_cursor(0, i);
        write_str(&mut grid, &format!("r{i}"));
    }
    grid
}

fn screen(grid: &Grid) -> Vec<String> {
    grid.screen_lines().map(text).collect()
}

fn text(row: &[Cell]) -> String {
    row.iter()
        .map(|cell| cell.c)
        .collect::<String>()
        .trim_end()
        .to_string()
}

#[test]
fn full_history_drops_its_oldest_rows_first() {
    let mut grid = numbered(4, 2);
    grid.history_limit = 2;
    for _ in 0..4 {
        grid.newline();
    }
    let kept: Vec<String> = grid
        .history
        .iter()
        .map(|row| text(&row.to_cells(0)))
        .collect();
    assert_eq!(
        kept,
        ["", ""],
        "r0 and r1 were evicted by the two blank rows after them"
    );
    assert_eq!(screen(&grid), ["", ""]);

    let mut grid = numbered(4, 3);
    grid.history_limit = 2;
    grid.scroll_up(3);
    let kept: Vec<String> = grid
        .history
        .iter()
        .map(|row| text(&row.to_cells(0)))
        .collect();
    assert_eq!(kept, ["r1", "r2"]);
}

#[test]
fn scrolling_a_region_leaves_rows_outside_it_and_history_alone() {
    let mut grid = numbered(4, 5);
    grid.scroll_top = 1;
    grid.scroll_bottom = 3;
    grid.scroll_up(1);
    assert_eq!(screen(&grid), ["r0", "r2", "r3", "", "r4"]);
    assert!(grid.history.is_empty());

    grid.scroll_down(2);
    assert_eq!(screen(&grid), ["r0", "", "", "r2", "r4"]);
}

#[test]
fn insert_and_delete_lines_shift_from_the_cursor_to_the_region_bottom() {
    let mut grid = numbered(4, 5);
    grid.scroll_bottom = 3;
    grid.move_cursor(0, 1);
    grid.insert_lines(1);
    assert_eq!(screen(&grid), ["r0", "", "r1", "r2", "r4"]);
    grid.delete_lines(2);
    assert_eq!(screen(&grid), ["r0", "r2", "", "", "r4"]);
    assert!(grid.history.is_empty());
}

/// Writes `text` on the cursor's row and moves the cursor to the start of the next one.
fn line(grid: &mut Grid, text: &str) {
    write_str(grid, text);
    grid.carriage_return();
    grid.newline();
}

/// Five lines `r0`..`r4` written to a 3-row grid: `r0` and `r1` in history, the cursor on the
/// bottom row after `r4`.
fn with_two_in_history() -> Grid {
    let mut grid = Grid::new(4, 3);
    for i in 0..4 {
        line(&mut grid, &format!("r{i}"));
    }
    write_str(&mut grid, "r4");
    grid
}

#[test]
fn growing_pulls_history_back_and_keeps_the_cursor_on_the_bottom_row() {
    let mut grid = with_two_in_history();
    assert_eq!(grid.history_len(), 2);
    grid.resize(4, 5);
    assert_eq!(screen(&grid), ["r0", "r1", "r2", "r3", "r4"]);
    assert_eq!(grid.cursor.y, 4, "the prompt row stays the bottom row");
    assert_eq!(grid.history_len(), 0);
}

#[test]
fn growing_with_partial_history_puts_the_unfilled_rows_on_top() {
    let mut grid = Grid::new(4, 2);
    line(&mut grid, "r0");
    line(&mut grid, "r1");
    write_str(&mut grid, "r2");
    assert_eq!(screen(&grid), ["r1", "r2"]);
    assert_eq!(grid.history_len(), 1);

    grid.resize(4, 5);
    assert_eq!(screen(&grid), ["", "", "r0", "r1", "r2"]);
    assert_eq!(grid.cursor.y, 4);
    assert_eq!(grid.history_len(), 0);
}

#[test]
fn growing_with_no_history_keeps_the_content_bottom_anchored() {
    let mut grid = Grid::new(4, 3);
    write_str(&mut grid, "r0");
    grid.move_cursor(0, 2);
    write_str(&mut grid, "r2");
    grid.resize(4, 6);
    assert_eq!(screen(&grid), ["", "", "", "r0", "", "r2"]);
    assert_eq!(grid.cursor.y, 5);
}

#[test]
fn growing_with_the_cursor_above_the_bottom_leaves_blank_rows_below() {
    let mut grid = Grid::new(4, 3);
    write_str(&mut grid, "r0");
    grid.resize(4, 5);
    assert_eq!(screen(&grid), ["r0", "", "", "", ""]);
    assert_eq!(grid.cursor.y, 0, "the cursor keeps its row");
}

#[test]
fn growing_pulls_history_even_when_the_cursor_is_not_on_the_bottom() {
    let mut grid = with_two_in_history();
    grid.move_cursor(0, 0);
    grid.resize(4, 4);
    assert_eq!(screen(&grid), ["r1", "r2", "r3", "r4"]);
    assert_eq!(
        grid.cursor.y, 1,
        "the cursor moves down with the pulled row"
    );
    assert_eq!(grid.history_len(), 1);
}

#[test]
fn growing_without_scrollback_only_pads() {
    let mut grid = with_two_in_history();
    grid.set_history_limit(0);
    grid.resize(4, 6);
    assert_eq!(screen(&grid), ["", "", "", "r2", "r3", "r4"]);
    assert_eq!(grid.cursor.y, 5);
}

#[test]
fn growing_the_alternate_screen_never_pulls_history() {
    let mut grid = with_two_in_history();
    grid.set_alternate_screen(true);
    grid.resize(4, 5);
    assert_eq!(screen(&grid), ["r2", "r3", "r4", "", ""]);
    assert_eq!(grid.cursor.y, 2);
    assert_eq!(grid.history_len(), 2);
}

#[test]
fn shrinking_with_the_cursor_on_the_bottom_pushes_the_top_rows_into_history() {
    let mut grid = numbered(4, 3);
    grid.resize(4, 2);
    assert_eq!(screen(&grid), ["r1", "r2"]);
    assert_eq!(grid.cursor.y, 1, "the cursor keeps its row at the bottom");
    let kept: Vec<String> = grid
        .history
        .iter()
        .map(|row| text(&row.to_cells(0)))
        .collect();
    assert_eq!(kept, ["r0"]);
}

#[test]
fn shrinking_with_the_cursor_mid_screen_cuts_rows_from_the_bottom() {
    let mut grid = numbered(4, 5);
    grid.move_cursor(0, 1);
    grid.resize(4, 4);
    assert_eq!(screen(&grid), ["r0", "r1", "r2", "r3"]);
    assert_eq!(grid.cursor.y, 1);
    assert!(grid.history.is_empty());
}

#[test]
fn shrinking_the_alternate_screen_cuts_rows_from_the_bottom() {
    let mut grid = numbered(4, 3);
    grid.set_alternate_screen(true);
    grid.resize(4, 2);
    assert_eq!(screen(&grid), ["r0", "r1"]);
    assert_eq!(grid.cursor.y, 1);
    assert!(grid.history.is_empty());
}

#[test]
fn shrinking_without_scrollback_keeps_the_cursor_row_and_stores_nothing() {
    let mut grid = numbered(4, 3);
    grid.set_history_limit(0);
    grid.resize(4, 2);
    assert_eq!(screen(&grid), ["r1", "r2"]);
    assert!(grid.history.is_empty());
}

#[test]
fn a_grow_then_shrink_round_trip_restores_the_screen() {
    let mut grid = with_two_in_history();
    grid.resize(4, 5);
    grid.resize(4, 3);
    assert_eq!(screen(&grid), ["r2", "r3", "r4"]);
    assert_eq!(grid.cursor.y, 2);
}

#[test]
fn wrapped_rows_keep_their_flag_through_history() {
    let mut grid = Grid::new(4, 2);
    write_str(&mut grid, "abcdefgh");
    grid.carriage_return();
    grid.newline();
    assert_eq!(screen(&grid), ["efgh", ""]);
    assert!(grid.line_wrapped(0), "abcd is in history, wrapped");

    grid.resize(4, 3);
    assert_eq!(screen(&grid), ["abcd", "efgh", ""]);
    assert!(grid.line_wrapped(0), "the pulled row still wraps");
    assert!(!grid.line_wrapped(1));
}

#[test]
fn shell_marks_keep_their_rows_through_a_grow_and_a_shrink() {
    let mut grid = Grid::new(4, 3);
    write_str(&mut grid, "r0");
    grid.carriage_return();
    grid.newline();
    write_str(&mut grid, "r1");
    grid.carriage_return();
    grid.newline();
    write_str(&mut grid, "r2");
    grid.mark_shell(ShellMark::Prompt);
    assert_eq!(grid.shell_marks(ShellMark::Prompt).collect::<Vec<_>>(), [2]);

    grid.resize(4, 5);
    assert_eq!(
        grid.shell_marks(ShellMark::Prompt).collect::<Vec<_>>(),
        [4],
        "the prompt moved down with its row"
    );
    assert_eq!(grid.line_text(4).as_deref(), Some("r2"));

    grid.resize(4, 2);
    assert_eq!(
        grid.shell_marks(ShellMark::Prompt).collect::<Vec<_>>(),
        [4],
        "the prompt is still at its row number after the rows above it went to history"
    );
    assert_eq!(grid.line_text(4).as_deref(), Some("r2"));
}

#[test]
fn growing_pulls_history_back_and_keeps_marks_on_their_rows() {
    let mut grid = with_two_in_history();
    grid.mark_shell(ShellMark::Output);
    grid.resize(4, 5);
    assert_eq!(grid.shell_marks(ShellMark::Output).collect::<Vec<_>>(), [4]);
    assert_eq!(grid.line_text(4).as_deref(), Some("r4"));
}

#[test]
fn writes_characters_and_advances_cursor() {
    let mut grid = Grid::new(10, 4);
    write_str(&mut grid, "hello");
    assert_eq!(grid.cursor.x, 5);
    assert_eq!(grid.cursor.y, 0);
    assert!(grid.render_plain().starts_with("hello"));
}

#[test]
fn newline_at_bottom_scrolls_oldest_line_out() {
    let mut grid = Grid::new(10, 2);
    write_str(&mut grid, "line1");
    grid.carriage_return();
    grid.newline();
    write_str(&mut grid, "line2");
    grid.carriage_return();
    grid.newline();
    write_str(&mut grid, "line3");
    let rendered = grid.render_plain();
    assert!(!rendered.contains("line1"));
    assert!(rendered.contains("line2"));
    assert!(rendered.contains("line3"));
}

#[test]
fn scrolled_lines_are_kept_in_history() {
    let mut grid = Grid::new(10, 4);
    for i in 0..6 {
        write_str(&mut grid, &format!("row{i}"));
        grid.carriage_return();
        grid.newline();
    }
    assert!(grid.history.len() >= 2);
    assert_eq!(grid.history[0].cell(0).map(|c| c.c), Some('r'));
    assert_eq!(grid.history[0].cell(3).map(|c| c.c), Some('0'));
}

#[test]
fn clear_screen_resets_cursor_and_cells() {
    let mut grid = Grid::new(10, 2);
    write_str(&mut grid, "abc");
    grid.clear_screen(2);
    assert_eq!(grid.cursor.x, 0);
    assert_eq!(grid.cursor.y, 0);
    assert_eq!(grid.get_cell(0, 0).unwrap().c, ' ');
}

#[test]
fn line_cells_borrows_history_and_live_screen() {
    let mut grid = Grid::new(10, 2);
    write_str(&mut grid, "hist0");
    grid.carriage_return();
    grid.newline();
    write_str(&mut grid, "live0");
    grid.carriage_return();
    grid.newline();
    write_str(&mut grid, "live1");

    assert_eq!(grid.history_len(), 1);
    // History line at abs_line 0
    let h_cells = grid.line_cells(0).expect("history line 0");
    assert_eq!(h_cells[0].c, 'h');
    // Live lines at abs_line 1 and 2
    let l0_cells = grid.line_cells(1).expect("live line 0");
    assert_eq!(l0_cells[0].c, 'l');
    let l1_cells = grid.line_cells(2).expect("live line 1");
    assert_eq!(l1_cells[4].c, '1');
    // Out of bounds
    assert!(grid.line_cells(3).is_none());
}

#[test]
fn take_scroll_reports_the_net_move_of_one_region_once() {
    let mut grid = numbered(4, 5);
    grid.scroll_up(2);
    grid.scroll_down(1);
    assert_eq!(
        grid.take_scroll(),
        Some(ScrollHint {
            top: 0,
            bottom: 4,
            lines: 1
        })
    );
    assert_eq!(grid.take_scroll(), None, "taken once");
    grid.scroll_up(1);
    grid.scroll_down(1);
    assert_eq!(grid.take_scroll(), None, "no net move");
}

#[test]
fn take_scroll_gives_up_on_mixed_regions_and_resizes() {
    let mut grid = numbered(4, 5);
    grid.scroll_up(1);
    grid.scroll_top = 1;
    grid.scroll_up(1);
    assert_eq!(grid.take_scroll(), None);
    grid.scroll_up(1);
    assert!(grid.take_scroll().is_some(), "a fresh frame starts clean");
    grid.scroll_up(1);
    grid.resize(6, 5);
    assert_eq!(grid.take_scroll(), None);
}

fn styled(c: char, fg: Option<Color>, bg: Option<Color>, bold: bool) -> Cell {
    Cell {
        c,
        style: CellStyle {
            fg,
            bg,
            bold,
            ..CellStyle::default()
        },
    }
}

#[test]
fn history_rows_keep_every_cell_up_to_the_last_non_blank() {
    let cells = vec![
        styled('a', Some(Color::Red), None, true),
        styled('b', Some(Color::Red), None, true),
        styled(' ', None, None, false),
        styled('→', Some(Color::Rgb(1, 2, 3)), None, false),
        styled(' ', None, Some(Color::Blue), false),
        Cell::default(),
        Cell::default(),
    ];
    let row = HistoryRow::from_cells(&cells);
    assert_eq!(
        row.len(),
        5,
        "default blanks after the last styled cell are dropped"
    );
    for (x, cell) in cells.iter().enumerate().take(5) {
        assert_eq!(row.cell(x), Some(*cell), "column {x}");
    }
    assert_eq!(row.cell(5), None);
    assert_eq!(row.to_cells(7), cells);
}

#[test]
fn history_rows_of_blanks_and_ascii_round_trip() {
    let blank = HistoryRow::from_cells(&[Cell::default(); 4]);
    assert!(blank.is_empty());
    assert_eq!(blank.to_cells(4), vec![Cell::default(); 4]);
    let plain: Vec<Cell> = "plain text"
        .chars()
        .map(|c| styled(c, None, None, false))
        .collect();
    assert_eq!(HistoryRow::from_cells(&plain).to_cells(0), plain);
}

#[test]
fn scrolled_views_and_line_cells_read_history_as_full_width_rows() {
    let mut grid = Grid::new(6, 2);
    grid.current_style.fg = Some(Color::Green);
    write_str(&mut grid, "ab");
    grid.newline();
    grid.newline();
    grid.newline();
    assert!(grid.history_len() >= 1);
    let first = grid
        .get_cell_scrolled(0, 0, grid.history_len())
        .expect("cell");
    assert_eq!((first.c, first.style.fg), ('a', Some(Color::Green)));
    assert_eq!(
        grid.get_cell_scrolled(5, 0, grid.history_len()),
        Some(Cell::default())
    );
    let row = grid.line_cells(0).expect("history row");
    assert_eq!(row.len(), 6);
    assert_eq!(text(&row), "ab");
}
