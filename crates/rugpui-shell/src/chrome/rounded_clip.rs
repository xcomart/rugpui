use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Tiling, Window, point, px,
};

use super::{WINDOW_BORDER_WIDTH, WINDOW_CORNER_RADIUS};

/// Keeps layout flush with the outline and clips painting at the curved corners.
pub(super) struct RoundedClip {
    child: AnyElement,
    tiling: Tiling,
}

impl RoundedClip {
    pub(super) fn new(child: impl IntoElement, tiling: Tiling) -> Self {
        Self {
            child: child.into_any_element(),
            tiling,
        }
    }
}

impl IntoElement for RoundedClip {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for RoundedClip {
    type RequestLayoutState = ();
    type PrepaintState = Vec<Bounds<Pixels>>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let regions = clip_regions(bounds, self.tiling, window.scale_factor());
        window.with_content_clip_regions(&regions, |window| self.child.prepaint(window, cx));
        regions
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _layout: &mut (),
        regions: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_content_clip_regions(regions, |window| self.child.paint(window, cx));
    }
}

/// Horizontal strips share device-pixel boundaries, so translucent content is
/// never painted twice at a seam. The middle rectangle comes first for the
/// scene's fast path; only the corner rows need splitting.
fn clip_regions(bounds: Bounds<Pixels>, tiling: Tiling, scale: f32) -> Vec<Bounds<Pixels>> {
    let edge = |tiled| if tiled { 0. } else { WINDOW_BORDER_WIDTH };
    let left = f32::from(bounds.left()) + edge(tiling.left);
    let right = f32::from(bounds.right()) - edge(tiling.right);
    let top = f32::from(bounds.top()) + edge(tiling.top);
    let bottom = f32::from(bounds.bottom()) - edge(tiling.bottom);
    if right <= left || bottom <= top {
        return Vec::new();
    }

    let radius = (WINDOW_CORNER_RADIUS - WINDOW_BORDER_WIDTH)
        .min((right - left) / 2.)
        .min((bottom - top) / 2.);
    let top_left = !tiling.top && !tiling.left;
    let top_right = !tiling.top && !tiling.right;
    let bottom_left = !tiling.bottom && !tiling.left;
    let bottom_right = !tiling.bottom && !tiling.right;
    let middle_top = if top_left || top_right {
        ((top + radius) * scale).ceil() / scale
    } else {
        top
    }
    .min(bottom);
    let middle_bottom = if bottom_left || bottom_right {
        ((bottom - radius) * scale).floor() / scale
    } else {
        bottom
    }
    .max(middle_top);

    let rect = |x1, y1, x2, y2| Bounds::from_corners(point(px(x1), px(y1)), point(px(x2), px(y2)));
    let mut regions = Vec::new();
    if middle_bottom > middle_top {
        regions.push(rect(left, middle_top, right, middle_bottom));
    }
    let inset = |distance: f32| {
        let distance = distance.clamp(0., radius);
        radius
            - (radius * radius - (radius - distance).powi(2))
                .max(0.)
                .sqrt()
    };
    for (start, end) in [(top, middle_top), (middle_bottom, bottom)] {
        for row in (start * scale).floor() as i32..(end * scale).ceil() as i32 {
            let y = (row as f32 / scale).max(start);
            let next = ((row + 1) as f32 / scale).min(end);
            if next <= y {
                continue;
            }
            // Evaluate at the device pixel's centre; the smooth outline covers
            // the boundary of the inner curve.
            let center = (y + next) / 2.;
            let top_inset = inset(center - top);
            let bottom_inset = inset(bottom - center);
            let x1 = left
                + (if top_left { top_inset } else { 0. }).max(if bottom_left {
                    bottom_inset
                } else {
                    0.
                });
            let x2 = right
                - (if top_right { top_inset } else { 0. }).max(if bottom_right {
                    bottom_inset
                } else {
                    0.
                });
            if x2 > x1 {
                regions.push(rect(x1, y, x2, next));
            }
        }
    }
    regions
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    #[test]
    fn corners_are_cut_without_insetting_the_straight_edges() {
        let regions = clip_regions(
            Bounds::new(point(px(24.), px(24.)), size(px(200.), px(100.))),
            Tiling::default(),
            1.,
        );
        let visible = |x, y| regions.iter().any(|r| r.contains(&point(px(x), px(y))));
        assert!(visible(25.1, 60.));
        assert!(visible(100., 25.1));
        assert!(!visible(25.1, 25.1));
        assert!(visible(29., 29.));
        assert!(!visible(24.9, 60.));
    }

    #[test]
    fn strips_cover_every_row_once_at_fractional_scales_and_small_sizes() {
        for scale in [1., 1.25, 1.5, 2.] {
            for width in [3., 12., 200.] {
                let regions = clip_regions(
                    Bounds::new(point(px(24.2), px(24.3)), size(px(width), px(width))),
                    Tiling::default(),
                    scale,
                );
                for (index, a) in regions.iter().enumerate() {
                    for b in &regions[index + 1..] {
                        assert!(
                            a.intersect(b).is_empty(),
                            "overlapping strips at scale {scale}"
                        );
                    }
                }
                let mut rows: Vec<_> = regions.iter().map(|r| (r.top(), r.bottom())).collect();
                rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                assert_eq!(rows.first().unwrap().0, px(25.3));
                assert_eq!(rows.last().unwrap().1, px(24.3 + width - 1.));
                for adjacent in rows.windows(2) {
                    assert_eq!(adjacent[0].1, adjacent[1].0);
                }
            }
        }
    }

    #[test]
    fn tiled_edges_stay_flush_and_maximized_windows_are_rectangular() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(200.), px(100.)));
        let all = Tiling {
            top: true,
            bottom: true,
            left: true,
            right: true,
        };
        assert_eq!(clip_regions(bounds, all, 1.25), vec![bounds]);
        let left = Tiling {
            left: true,
            ..Tiling::default()
        };
        let regions = clip_regions(bounds, left, 1.);
        assert!(regions.iter().all(|r| r.left() == px(0.)));
        assert!(regions.iter().any(|r| r.contains(&point(px(0.), px(1.1)))));
    }
}
