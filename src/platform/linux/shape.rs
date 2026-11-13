//! Turns the notch outline into X11 SHAPE rectangles (pure, so it is tested on every OS).

/// One rectangle of the shape, in physical pixels relative to the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

/// The outline (logical points) in physical pixels.
pub fn scaled_outline(outline: &[(f32, f32)], scale: f32) -> Vec<(i16, i16)> {
    outline
        .iter()
        .map(|&(x, y)| ((x * scale).round() as i16, (y * scale).round() as i16))
        .collect()
}

/// X11 scale factor the way winit picks it: `WINIT_X11_SCALE_FACTOR`, then `Xft.dpi` / 96
/// from the root window's resources, then 1. Invalid or non-positive values are skipped.
pub fn scale_factor(env: Option<&str>, resources: Option<&str>) -> f32 {
    let valid = |value: f32| value.is_finite() && value > 0.0;
    if let Some(value) = env.and_then(|text| text.trim().parse::<f32>().ok())
        && valid(value)
    {
        return value;
    }
    resources
        .and_then(|text| {
            text.lines().find_map(|line| {
                let (key, value) = line.split_once(':')?;
                (key.trim() == "Xft.dpi")
                    .then(|| value.trim().parse::<f32>().ok())
                    .flatten()
            })
        })
        .map(|dpi| dpi / 96.0)
        .filter(|scale| valid(*scale))
        .unwrap_or(1.0)
}

/// Fills the polygon row by row (even-odd, sampled at each row's centre) and merges rows with
/// identical spans into taller rectangles.
pub fn polygon_rects(points: &[(i16, i16)]) -> Vec<Rect> {
    if points.len() < 3 {
        return Vec::new();
    }
    let top = points.iter().map(|p| p.1).min().unwrap_or(0);
    let bottom = points.iter().map(|p| p.1).max().unwrap_or(0);
    let mut rects: Vec<Rect> = Vec::new();
    // Rectangles that end on the previous row and may grow down by one.
    let mut open: Vec<usize> = Vec::new();
    for y in top..bottom {
        let centre = f32::from(y) + 0.5;
        let mut crossings: Vec<f32> = Vec::new();
        for (i, &(x0, y0)) in points.iter().enumerate() {
            let (x1, y1) = points[(i + 1) % points.len()];
            let (fy0, fy1) = (f32::from(y0), f32::from(y1));
            if (fy0 <= centre) != (fy1 <= centre) {
                let t = (centre - fy0) / (fy1 - fy0);
                crossings.push(f32::from(x0) + t * (f32::from(x1) - f32::from(x0)));
            }
        }
        crossings.sort_by(f32::total_cmp);
        let spans: Vec<(i16, i16)> = crossings
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[start, end]| (start.round() as i16, end.round() as i16))
            .filter(|(start, end)| end > start)
            .collect();
        let mut next_open = Vec::new();
        for (start, end) in spans {
            let width = (end - start) as u16;
            let grown = open.iter().copied().find(|&index| {
                let rect = rects[index];
                rect.x == start && rect.width == width
            });
            match grown {
                Some(index) => {
                    rects[index].height += 1;
                    next_open.push(index);
                }
                None => {
                    rects.push(Rect {
                        x: start,
                        y,
                        width,
                        height: 1,
                    });
                    next_open.push(rects.len() - 1);
                }
            }
        }
        open = next_open;
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_scales_and_rounds() {
        assert_eq!(
            scaled_outline(&[(0.0, 0.0), (10.5, 4.0)], 1.5),
            vec![(0, 0), (16, 6)]
        );
    }

    #[test]
    fn rectangle_becomes_one_rect() {
        let rects = polygon_rects(&[(0, 0), (10, 0), (10, 4), (0, 4)]);
        assert_eq!(
            rects,
            vec![Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 4
            }]
        );
    }

    #[test]
    fn trapezoid_narrows_row_by_row() {
        // Wide at the top, 2 px narrower on each side per row: like the notch's lower corners.
        let rects = polygon_rects(&[(0, 0), (20, 0), (16, 2), (4, 2)]);
        assert_eq!(rects.len(), 2);
        assert_eq!((rects[0].y, rects[0].height), (0, 1));
        assert_eq!((rects[1].y, rects[1].height), (1, 1));
        assert!(rects[0].width > rects[1].width);
        assert_eq!(rects[0].x + rects[0].width as i16, 20 - rects[0].x);
    }

    #[test]
    fn scale_from_env_then_xft_dpi() {
        assert_eq!(scale_factor(Some("2"), None), 2.0);
        assert_eq!(
            scale_factor(
                None,
                Some(
                    "Xft.antialias:	1
Xft.dpi:	144
"
                )
            ),
            1.5
        );
        assert_eq!(scale_factor(Some("junk"), Some("Xft.dpi: 192")), 2.0);
        assert_eq!(scale_factor(None, None), 1.0);
        assert_eq!(scale_factor(Some("0"), Some("Xft.dpi: -5")), 1.0);
    }

    #[test]
    fn degenerate_outline_has_no_shape() {
        assert!(polygon_rects(&[(0, 0), (5, 5)]).is_empty());
    }
}
