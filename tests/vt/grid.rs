use mpx::vt::Grid;

fn write_str(grid: &mut Grid, text: &str) {
    for c in text.chars() {
        grid.put_char(c);
    }
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
