use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn area(&self) -> u32 {
        self.width as u32 * self.height as u32
    }

    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    pub fn split_horizontal(&self, ratio: f32) -> (Rect, Rect) {
        let (left_w, right_w) = divide(self.width, ratio);
        let left = Rect::new(self.x, self.y, left_w, self.height);
        let right_x = self.x + (left_w + 1).min(self.width.saturating_sub(1));
        let right = Rect::new(right_x, self.y, right_w, self.height);
        (left, right)
    }

    pub fn split_vertical(&self, ratio: f32) -> (Rect, Rect) {
        let (top_h, bottom_h) = divide(self.height, ratio);
        let top = Rect::new(self.x, self.y, self.width, top_h);
        let bottom_y = self.y + (top_h + 1).min(self.height.saturating_sub(1));
        let bottom = Rect::new(self.x, bottom_y, self.width, bottom_h);
        (top, bottom)
    }
}

/// Sizes of the two sides of a split of `total` cells around a one-cell divider. The first side
/// takes `ratio` of the cells left; each side keeps a cell when there are two to share.
fn divide(total: u16, ratio: f32) -> (u16, u16) {
    let avail = total.saturating_sub(1);
    let floor = u16::from(avail >= 2);
    let first = ((avail as f32) * ratio.clamp(0.1, 0.9)).round() as u16;
    let first = first.clamp(floor, avail.saturating_sub(floor).max(floor));
    (first, avail - first)
}
