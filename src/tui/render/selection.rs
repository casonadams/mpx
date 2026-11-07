#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionRange {
    Character {
        anchor_x: usize,
        anchor_abs_y: usize,
        cursor_x: usize,
        cursor_abs_y: usize,
    },
    Line {
        start_abs_y: usize,
        end_abs_y: usize,
    },
    /// A rectangle: the columns between the corners, on the rows between them.
    Rectangle {
        anchor_x: usize,
        anchor_abs_y: usize,
        cursor_x: usize,
        cursor_abs_y: usize,
    },
}

impl SelectionRange {
    pub fn contains_abs(&self, x: usize, abs_y: usize) -> bool {
        match self {
            Self::Line {
                start_abs_y,
                end_abs_y,
            } => {
                let min_y = (*start_abs_y).min(*end_abs_y);
                let max_y = (*start_abs_y).max(*end_abs_y);
                abs_y >= min_y && abs_y <= max_y
            }
            Self::Character {
                anchor_x,
                anchor_abs_y,
                cursor_x,
                cursor_abs_y,
            } => {
                let anchor = (*anchor_abs_y, *anchor_x);
                let cursor = (*cursor_abs_y, *cursor_x);
                let ((sy, sx), (ey, ex)) = if anchor <= cursor {
                    (anchor, cursor)
                } else {
                    (cursor, anchor)
                };
                abs_y >= sy && abs_y <= ey && (abs_y != sy || x >= sx) && (abs_y != ey || x <= ex)
            }
            Self::Rectangle {
                anchor_x,
                anchor_abs_y,
                cursor_x,
                cursor_abs_y,
            } => {
                let (x0, x1) = (*anchor_x.min(cursor_x), *anchor_x.max(cursor_x));
                let (y0, y1) = (
                    *anchor_abs_y.min(cursor_abs_y),
                    *anchor_abs_y.max(cursor_abs_y),
                );
                abs_y >= y0 && abs_y <= y1 && x >= x0 && x <= x1
            }
        }
    }
}
