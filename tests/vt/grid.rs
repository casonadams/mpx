use mpx::vt::{Cell, Grid};

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
    let kept: Vec<String> = grid.history.iter().map(|row| text(row)).collect();
    assert_eq!(
        kept,
        ["", ""],
        "r0 and r1 were evicted by the two blank rows after them"
    );
    assert_eq!(screen(&grid), ["", ""]);

    let mut grid = numbered(4, 3);
    grid.history_limit = 2;
    grid.scroll_up(3);
    let kept: Vec<String> = grid.history.iter().map(|row| text(row)).collect();
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

#[test]
fn resize_keeps_the_top_left_and_blanks_new_space() {
    let mut grid = numbered(4, 3);
    grid.resize(2, 2);
    assert_eq!(screen(&grid), ["r0", "r1"]);
    grid.resize(5, 4);
    assert_eq!(screen(&grid), ["r0", "r1", "", ""]);
    assert_eq!(grid.screen_lines().map(<[Cell]>::len).max(), Some(5));
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
    assert_eq!(grid.history[0][0].c, 'r');
    assert_eq!(grid.history[0][3].c, '0');
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
