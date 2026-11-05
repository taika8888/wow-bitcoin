use serde::Serialize;

/// Geometry in Windows' space: physical pixels, top-left origin, y growing down. Screen rects are in virtual-screen
/// coordinates, where the primary monitor's top-left corner is (0, 0) and other monitors can sit at negative x or y.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    pub const fn at(origin: Point, size: Size) -> Self {
        Self::new(origin.x, origin.y, size.width, size.height)
    }

    pub fn min_x(&self) -> f64 {
        self.x
    }

    pub fn min_y(&self) -> f64 {
        self.y
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }

    pub fn mid_x(&self) -> f64 {
        self.x + self.width / 2.0
    }

    pub fn mid_y(&self) -> f64 {
        self.y + self.height / 2.0
    }

    pub fn origin(&self) -> Point {
        Point { x: self.x, y: self.y }
    }

    pub fn size(&self) -> Size {
        Size { width: self.width, height: self.height }
    }

    pub fn offset(&self, dx: f64, dy: f64) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    /// The top and left edges are inside, the bottom and right edges are not, as with a Win32 RECT.
    pub fn contains(&self, point: Point) -> bool {
        (self.min_x()..self.max_x()).contains(&point.x) && (self.min_y()..self.max_y()).contains(&point.y)
    }

    pub fn contains_rect(&self, other: &Rect) -> bool {
        other.min_x() >= self.min_x()
            && other.max_x() <= self.max_x()
            && other.min_y() >= self.min_y()
            && other.max_y() <= self.max_y()
    }

    /// The overlap, or None when the rects only touch or are apart.
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let (left, right) = (self.min_x().max(other.min_x()), self.max_x().min(other.max_x()));
        let (top, bottom) = (self.min_y().max(other.min_y()), self.max_y().min(other.max_y()));
        (right > left && bottom > top).then(|| Rect::new(left, top, right - left, bottom - top))
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.intersection(other).is_some()
    }

    pub fn area(&self) -> f64 {
        self.width * self.height
    }
}
