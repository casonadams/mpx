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
        let ratio = ratio.clamp(0.1, 0.9);
        let left_w = ((self.width as f32) * ratio).round() as u16;
        let right_w = self.width.saturating_sub(left_w);

        let left = Rect::new(self.x, self.y, left_w, self.height);
        let right = Rect::new(self.x + left_w, self.y, right_w, self.height);
        (left, right)
    }

    pub fn split_vertical(&self, ratio: f32) -> (Rect, Rect) {
        let ratio = ratio.clamp(0.1, 0.9);
        let top_h = ((self.height as f32) * ratio).round() as u16;
        let bottom_h = self.height.saturating_sub(top_h);

        let top = Rect::new(self.x, self.y, self.width, top_h);
        let bottom = Rect::new(self.x, self.y + top_h, self.width, bottom_h);
        (top, bottom)
    }
}
