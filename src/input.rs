use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTarget {
    SourceLine(usize),
    Comment(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitArea {
    pub rect: Rect,
    pub target: HitTarget,
}

impl HitArea {
    #[must_use]
    pub fn new(rect: Rect, target: HitTarget) -> Self {
        Self { rect, target }
    }

    #[must_use]
    pub fn contains(self, column: u16, row: u16) -> bool {
        column >= self.rect.x
            && column < self.rect.x.saturating_add(self.rect.width)
            && row >= self.rect.y
            && row < self.rect.y.saturating_add(self.rect.height)
    }
}

#[must_use]
pub fn hit_test(areas: &[HitArea], column: u16, row: u16) -> Option<HitTarget> {
    areas
        .iter()
        .rev()
        .find(|area| area.contains(column, row))
        .map(|area| area.target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_hit_areas_win_like_render_z_order() {
        let areas = [
            HitArea::new(Rect::new(0, 0, 10, 2), HitTarget::SourceLine(1)),
            HitArea::new(Rect::new(2, 0, 4, 1), HitTarget::Comment(7)),
        ];
        assert_eq!(hit_test(&areas, 3, 0), Some(HitTarget::Comment(7)));
        assert_eq!(hit_test(&areas, 9, 1), Some(HitTarget::SourceLine(1)));
        assert_eq!(hit_test(&areas, 10, 1), None);
    }
}
