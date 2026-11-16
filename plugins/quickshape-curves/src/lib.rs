//! QuickShape curves: a stroke that bends more than once, held, snaps to a curve through its bends
//! rather than a polyline, its points there to reshape it. The app hands over the stroke's points
//! and what QuickShape found first (the plugin kit passes them); this finds the bends and lays a chain of cubics through them,
//! each anchor's controls leaning the way the stroke came and went so the joins are smooth. The app
//! draws the curve and lets it be reshaped. It's a plugin, not part of the app:
//! `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! A stroke that comes back round to where it began, bending in and out on its way (a cloud, a
//! flower), snaps to a closed curve rather than the ellipse QuickShape makes of it: its last anchor
//! is its first again, smooth there as anywhere. A loop that only bends one way is left the ellipse
//! (or circle) it is.
//!
//! The interface is written down at the top of `crates/core/src/plugin_run.rs`.

/// Closed curves need the same fidelity as open strokes: preserve smaller lobes.
const CLOSED_TOLERANCE: f32 = 0.025;
/// A stroke whose ends are this close (a part of its length) comes back round: QuickShape's own
/// measure.
const CLOSED: f32 = 0.18;
/// A closed stroke's bend turns the other way from the rest when it does by more than this (the
/// sine of the angle): less is a hand's wobble.
const TURNS_BACK: f32 = 0.1;

kit::quickshape_curve!(offer);

/// The curve offered for a stroke of `points`, QuickShape having found `found` first: one for a
/// polyline (it bends more than once), a closed one for a round loop that bends in and out, or
/// when a curve is asked for by name (none found: closed if the stroke comes back round), else none.
fn offer(points: &[P], found: Option<kit::ShapeKind>) -> Option<Vec<P>> {
    let pts: Vec<P> = points.iter().copied().filter(|p| p.0.is_finite() && p.1.is_finite()).collect();
    match found {
        None if closed(&pts) => fit_closed(&pts).map(|(nodes, _)| nodes),
        None | Some(kit::ShapeKind::Polyline) => Some(fit(&pts)).filter(|nodes| nodes.len() >= 4),
        Some(kit::ShapeKind::Ellipse | kit::ShapeKind::Circle) if closed(&pts) => {
            fit_closed(&pts).filter(|(_, wavy)| *wavy).map(|(nodes, _)| nodes)
        }
        Some(_) => None,
    }
}

type P = (f32, f32);

fn sub(a: P, b: P) -> P {
    (a.0 - b.0, a.1 - b.1)
}

fn add(a: P, b: P) -> P {
    (a.0 + b.0, a.1 + b.1)
}

fn scale(a: P, k: f32) -> P {
    (a.0 * k, a.1 * k)
}

fn length(a: P) -> f32 {
    a.0.hypot(a.1)
}

/// How far `p` is from the segment `a`–`b`.
fn distance_to_segment(p: P, a: P, b: P) -> f32 {
    let ab = sub(b, a);
    let len2 = ab.0 * ab.0 + ab.1 * ab.1;
    if len2 <= 1e-9 {
        return length(sub(p, a));
    }
    let ap = sub(p, a);
    let t = ((ap.0 * ab.0 + ap.1 * ab.1) / len2).clamp(0.0, 1.0);
    length(sub(p, add(a, scale(ab, t))))
}

/// How big the stroke is: its box's diagonal.
fn size(pts: &[P]) -> f32 {
    let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
    for &(x, y) in pts {
        (lo.0, lo.1, hi.0, hi.1) = (lo.0.min(x), lo.1.min(y), hi.0.max(x), hi.1.max(y));
    }
    if pts.is_empty() { 1.0 } else { length(sub(hi, lo)).max(1.0) }
}

/// The corners that keep the path within `tolerance` (Ramer–Douglas–Peucker), ends included.
fn simplify(pts: &[P], tolerance: f32) -> Vec<P> {
    fn keep(pts: &[P], tolerance: f32, out: &mut Vec<P>) {
        let (Some(&a), Some(&b)) = (pts.first(), pts.last()) else { return };
        let far = pts
            .iter()
            .enumerate()
            .map(|(i, &p)| (i, distance_to_segment(p, a, b)))
            .fold((0, 0.0f32), |best, x| if x.1 > best.1 { x } else { best });
        if far.1 > tolerance && far.0 > 0 && far.0 + 1 < pts.len() {
            keep(&pts[..=far.0], tolerance, out);
            out.pop();
            keep(&pts[far.0..], tolerance, out);
        } else {
            out.push(a);
            out.push(b);
        }
    }
    let mut out = Vec::new();
    keep(pts, tolerance, &mut out);
    out.dedup();
    out
}

/// How far the stroke goes, along its points.
fn path_length(pts: &[P]) -> f32 {
    pts.windows(2).map(|w| length(sub(w[1], w[0]))).sum()
}

/// Whether the stroke comes back round to where it began, as QuickShape tells a closed one.
fn closed(pts: &[P]) -> bool {
    let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else { return false };
    let way = path_length(pts);
    way > 0.0 && length(sub(last, first)) <= CLOSED * way
}

/// The closed curve round the stroke's bends, and whether it bends in and out (some bend turns
/// the other way from the rest, as an ellipse's never do). None with fewer than three bends.
///
/// The stroke stops where it comes nearest its start again, once past halfway: what runs on
/// beyond lies over its start. The bends are found going once round from the point farthest from
/// the start, so the start, where the stroke closes, is no bend unless it is one; they are laid
/// from the one nearest the start, as the stroke went, and the first again at the end closes it.
fn fit_closed(pts: &[P]) -> Option<(Vec<P>, bool)> {
    let start = *pts.first()?;
    // where it closes: the point nearest its start over the second half of its way
    let half = path_length(pts) / 2.0;
    let mut gone = 0.0;
    let mut close_at = pts.len() - 1;
    let mut nearest = f32::MAX;
    for (i, w) in pts.windows(2).enumerate() {
        gone += length(sub(w[1], w[0]));
        let d = length(sub(w[1], start));
        if gone >= half && d < nearest {
            (close_at, nearest) = (i + 1, d);
        }
    }
    let pts = &pts[..=close_at];
    let far = pts.iter().enumerate().fold((0, 0.0f32), |best, (i, &p)| {
        let d = length(sub(p, start));
        if d > best.1 { (i, d) } else { best }
    });
    // once round, from the far point back to it
    let mut ring: Vec<P> = pts[far.0..].to_vec();
    ring.extend_from_slice(&pts[..=far.0]);
    let mut anchors = simplify(&ring, CLOSED_TOLERANCE * size(pts));
    anchors.pop();
    let n = anchors.len();
    if n < 3 {
        return None;
    }
    let first = (0..n).min_by(|&a, &b| length(sub(anchors[a], start)).total_cmp(&length(sub(anchors[b], start))))?;
    anchors.rotate_left(first);
    let at = |i: usize| anchors[i % n];
    // which way each bend turns, of those that clearly do
    let turns: Vec<f32> = (0..n)
        .filter_map(|i| {
            let (u, v) = (sub(at(i), at(i + n - 1)), sub(at(i + 1), at(i)));
            let sine = (u.0 * v.1 - u.1 * v.0) / (length(u) * length(v)).max(1e-6);
            (sine.abs() > TURNS_BACK).then_some(sine)
        })
        .collect();
    let wavy = turns.iter().any(|&t| t > 0.0) && turns.iter().any(|&t| t < 0.0);
    let mut nodes = Vec::with_capacity(3 * n + 1);
    for i in 0..n {
        let (before, here, after, beyond) = (at(i + n - 1), at(i), at(i + 1), at(i + 2));
        nodes.push(here);
        nodes.push(add(here, scale(sub(after, before), 1.0 / 6.0)));
        nodes.push(sub(after, scale(sub(beyond, here), 1.0 / 6.0)));
    }
    nodes.push(at(0));
    Some((nodes, wavy))
}

/// Fit smooth cubic spans that stay close to the *drawn* stroke. Simply connecting a few
/// Ramer–Douglas–Peucker corners with Catmull–Rom handles changes the path between those corners:
/// it can cut across a wave and even invent a bulge. Instead, measure each proposed cubic's
/// distance from the original samples and insert another anchor where the deviation is greatest.
/// The two controls at a shared anchor use the same tangent, so joins stay smooth.
const MAX_OPEN_SPANS: usize = 96;

fn cubic_at(a: P, b: P, c: P, d: P, t: f32) -> P {
    let u = 1.0 - t;
    add(add(scale(a, u * u * u), scale(b, 3.0 * u * u * t)), add(scale(c, 3.0 * u * t * t), scale(d, t * t * t)))
}

fn tangent(pts: &[P], at: usize) -> P {
    let before = pts[at.saturating_sub(3)];
    let after = pts[(at + 3).min(pts.len() - 1)];
    let direction = sub(after, before);
    let len = length(direction);
    if len > 1e-6 { scale(direction, 1.0 / len) } else { (0.0, 0.0) }
}

fn cubic_span(pts: &[P], a: usize, b: usize) -> [P; 4] {
    let start = pts[a];
    let end = pts[b];
    let reach = length(sub(end, start)) / 3.0;
    [start, add(start, scale(tangent(pts, a), reach)), sub(end, scale(tangent(pts, b), reach)), end]
}

/// Distance to the sampled cubic instead of matching parameter t: pointer samples have varying
/// speed, and an unequal speed must not count as a geometric error.
fn cubic_deviation(pts: &[P], a: usize, b: usize, nodes: [P; 4]) -> (usize, f32) {
    let steps = 10;
    let mut last = nodes[0];
    let mut outline = Vec::with_capacity(steps);
    for i in 1..=steps {
        let now = cubic_at(nodes[0], nodes[1], nodes[2], nodes[3], i as f32 / steps as f32);
        outline.push((last, now));
        last = now;
    }
    let mut worst = (a, 0.0);
    for (i, &p) in pts.iter().enumerate().take(b).skip(a + 1) {
        let distance = outline.iter().map(|&(x, y)| distance_to_segment(p, x, y)).fold(f32::INFINITY, f32::min);
        if distance > worst.1 {
            worst = (i, distance);
        }
    }
    worst
}

/// Light, spatial Gaussian filtering before fitting: remove small hand jitter without
/// tracing each wiggle with another Bezier anchor. The radius is measured in pixels along
/// the stroke, not samples, because a slow pen can produce far more points than a fast one.
/// Endpoints stay exactly where the user placed them; short strokes are not altered.
fn smooth_stroke(pts: &[P]) -> Vec<P> {
    if pts.len() < 4 {
        return pts.to_vec();
    }
    const SIGMA: f32 = 4.0;
    const RADIUS: f32 = SIGMA * 2.5;
    let mut along = Vec::with_capacity(pts.len());
    along.push(0.0f32);
    for pair in pts.windows(2) {
        along.push(along.last().copied().unwrap_or(0.0) + length(sub(pair[1], pair[0])));
    }
    if along.last().copied().unwrap_or(0.0) < RADIUS * 2.0 {
        return pts.to_vec();
    }
    let mut out = pts.to_vec();
    let mut left = 0;
    let mut right = 0;
    for i in 1..pts.len() - 1 {
        while along[i] - along[left] > RADIUS {
            left += 1;
        }
        while right + 1 < pts.len() && along[right + 1] - along[i] <= RADIUS {
            right += 1;
        }
        let mut total = 0.0;
        let mut sum = (0.0, 0.0);
        for j in left..=right {
            let delta = (along[j] - along[i]) / SIGMA;
            let weight = (-0.5 * delta * delta).exp();
            sum = add(sum, scale(pts[j], weight));
            total += weight;
        }
        if total > 0.0 {
            out[i] = scale(sum, 1.0 / total);
        }
    }
    out
}

fn fit(pts: &[P]) -> Vec<P> {
    let pts: Vec<P> = pts.iter().copied().filter(|p| p.0.is_finite() && p.1.is_finite()).collect();
    if pts.len() < 2 {
        return Vec::new();
    }
    let pts = smooth_stroke(&pts);
    // A bounded canvas-pixel error preserves the shape even for long multi-wave strokes.
    // A small allowance avoids treating every tiny hand tremor as an intentional bend.
    let tolerance = (size(&pts) * 0.006).clamp(1.5, 3.0);
    let mut anchors = vec![0, pts.len() - 1];
    loop {
        if anchors.len() > MAX_OPEN_SPANS {
            break;
        }
        let mut worst = (0, 0.0);
        for pair in anchors.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if b - a < 2 {
                continue;
            }
            let (at, error) = cubic_deviation(&pts, a, b, cubic_span(&pts, a, b));
            if error > worst.1 {
                worst = (at, error);
            }
        }
        if worst.1 <= tolerance || worst.0 == 0 {
            break;
        }
        match anchors.binary_search(&worst.0) {
            Err(at) => {
                anchors.insert(at, worst.0);
            }
            Ok(_) => break,
        }
    }
    let mut nodes = Vec::with_capacity(anchors.len() * 3);
    for pair in anchors.windows(2) {
        let [a, b, c, _] = cubic_span(&pts, pair[0], pair[1]);
        nodes.extend([a, b, c]);
    }
    nodes.push(pts[*anchors.last().unwrap_or(&0)]);
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stroke across, back and across again.
    fn zigzag() -> Vec<P> {
        let corners = [(0.0, 0.0), (200.0, 10.0), (10.0, 90.0), (200.0, 160.0)];
        corners
            .windows(2)
            .flat_map(|w| (0..25).map(move |i| add(w[0], scale(sub(w[1], w[0]), i as f32 / 25.0))))
            .collect()
    }

    #[test]
    fn a_stroke_bending_twice_gets_a_curve_through_its_bends() {
        let nodes = fit(&zigzag());
        // anchors at 0, 3, 6 ...: each span's two controls between them; one at each corner
        assert_eq!(nodes.len() % 3, 1, "{nodes:?}");
        assert!(nodes.len() >= 10, "{nodes:?}");
        assert_eq!(nodes[0], (0.0, 0.0));
        // each anchor's controls either side on one line (smooth), the first's toward the next
        let (before, at, after) = (nodes[2], nodes[3], nodes[4]);
        let (u, v) = (sub(at, before), sub(after, at));
        assert!((u.0 * v.1 - u.1 * v.0).abs() < 1e-2 * length(u) * length(v), "{u:?} {v:?}");
    }

    #[test]
    fn a_long_wavy_stroke_keeps_its_smaller_bends() {
        // A long stroke whose box is much wider than each individual wave must not have its
        // intermediate turns flattened by the global simplification threshold.
        let stroke: Vec<P> = (0..=240)
            .map(|i| {
                let x = i as f32 * 2.0;
                (x, 100.0 + 18.0 * (x * std::f32::consts::TAU / 120.0).sin())
            })
            .collect();
        let nodes = fit(&stroke);
        let anchors: Vec<P> = nodes.iter().step_by(3).copied().collect();
        assert!(anchors.len() >= 9, "small waves were lost: {anchors:?}");
        for x in [30.0, 90.0, 150.0, 210.0, 270.0, 330.0, 390.0, 450.0] {
            assert!(anchors.iter().any(|&(ax, _)| (ax - x).abs() < 20.0), "missing the bend near {x}: {anchors:?}");
        }
    }

    #[test]
    fn fitted_curve_preserves_a_wavy_strokes_outline_without_new_bumps() {
        let stroke: Vec<P> = (0..=300)
            .map(|i| {
                let x = i as f32 * 2.0;
                (x, 100.0 + 26.0 * (x * std::f32::consts::TAU / 150.0).sin())
            })
            .collect();
        let nodes = fit(&stroke);
        assert_eq!(nodes.len() % 3, 1);
        // The entire fitted outline stays close to what was drawn, not just its anchors.
        let mut samples = Vec::new();
        for base in (0..nodes.len() - 1).step_by(3) {
            for i in 0..=32 {
                samples.push(cubic_at(nodes[base], nodes[base + 1], nodes[base + 2], nodes[base + 3], i as f32 / 32.0));
            }
        }
        let deviation = stroke
            .iter()
            .map(|&p| samples.windows(2).map(|w| distance_to_segment(p, w[0], w[1])).fold(f32::INFINITY, f32::min))
            .fold(0.0f32, f32::max);
        assert!(deviation < 4.0, "fitted path drifted {deviation} px");
        for &(_, y) in &samples {
            assert!((73.0..=127.0).contains(&y), "invented a bulge at y={y}");
        }
        // Both sides of every interior anchor have the same direction (smooth join).
        for k in (3..nodes.len() - 1).step_by(3) {
            let into = sub(nodes[k], nodes[k - 1]);
            let out = sub(nodes[k + 1], nodes[k]);
            assert!((into.0 * out.1 - into.1 * out.0).abs() <= 1e-3 * length(into) * length(out) + 1e-3);
            assert!(into.0 * out.0 + into.1 * out.1 >= -1e-3);
        }
    }

    #[test]
    fn spatial_smoothing_removes_jitter_but_preserves_curve_position() {
        let clean = |x: f32| 100.0 + 30.0 * (x * std::f32::consts::TAU / 200.0).sin();
        let noisy: Vec<P> = (0..=200)
            .map(|i| {
                let x = i as f32 * 2.0;
                let tremor = if i == 0 || i == 200 {
                    0.0
                } else if i % 2 == 0 {
                    2.8
                } else {
                    -2.8
                };
                (x, clean(x) + tremor)
            })
            .collect();
        let smooth = smooth_stroke(&noisy);
        assert_eq!(smooth.first(), noisy.first());
        assert_eq!(smooth.last(), noisy.last());
        let mean_error = |pts: &[P]| pts.iter().map(|&(x, y)| (y - clean(x)).abs()).sum::<f32>() / pts.len() as f32;
        assert!(
            mean_error(&smooth) < mean_error(&noisy) * 0.40,
            "jitter was not reduced: {} -> {}",
            mean_error(&noisy),
            mean_error(&smooth)
        );
        assert!(smooth.iter().zip(&noisy).all(|(&a, &b)| length(sub(a, b)) <= 4.0));
        let nodes = fit(&noisy);
        let mut fitted = Vec::new();
        for base in (0..nodes.len() - 1).step_by(3) {
            for i in 0..=20 {
                fitted.push(cubic_at(nodes[base], nodes[base + 1], nodes[base + 2], nodes[base + 3], i as f32 / 20.0));
            }
        }
        let deviation = (0..=200)
            .map(|i| {
                let p = (i as f32 * 2.0, clean(i as f32 * 2.0));
                fitted.windows(2).map(|w| distance_to_segment(p, w[0], w[1])).fold(f32::INFINITY, f32::min)
            })
            .fold(0.0f32, f32::max);
        assert!(deviation < 4.0, "fit drifted {deviation} pixels from intended curve");
    }

    #[test]
    fn a_curve_is_offered_for_a_polyline_or_when_asked_and_not_otherwise() {
        let stroke = zigzag();
        assert!(offer(&stroke, Some(kit::ShapeKind::Polyline)).is_some_and(|n| n.len() >= 10));
        assert!(offer(&stroke, None).is_some_and(|n| n.len() >= 10));
        assert_eq!(offer(&stroke, Some(kit::ShapeKind::Line)), None);
        assert_eq!(offer(&stroke, Some(kit::ShapeKind::Ellipse)), None);
    }

    /// Once round a loop `r` from (300, 300) at each turn, starting on the left, in `n` steps, and
    /// on a further `over` of the way round (past its start, or short of it if less than 0).
    fn loop_of(r: impl Fn(f32) -> f32, n: usize, over: f32) -> Vec<P> {
        let steps = ((1.0 + over) * n as f32) as usize;
        (0..=steps)
            .map(|i| {
                let t = std::f32::consts::PI + i as f32 / n as f32 * std::f32::consts::TAU;
                (300.0 + r(t) * t.cos(), 300.0 + 0.7 * r(t) * t.sin())
            })
            .collect()
    }

    /// A cloud: a loop bulging out and dipping in five times round.
    fn cloud(over: f32) -> Vec<P> {
        loop_of(|t| 150.0 * (1.0 + 0.25 * (5.0 * t).cos()), 200, over)
    }

    /// Whether `nodes` close: the last anchor the first, the controls either side of it on a line.
    fn closes_smoothly(nodes: &[P]) -> bool {
        let n = nodes.len();
        let (before, at, after) = (nodes[n - 2], nodes[0], nodes[1]);
        let (u, v) = (sub(at, before), sub(after, at));
        n % 3 == 1 && nodes[n - 1] == nodes[0] && (u.0 * v.1 - u.1 * v.0).abs() < 1e-2 * length(u) * length(v)
    }

    #[test]
    fn a_loop_that_bends_in_and_out_closes_as_a_curve_not_an_ellipse() {
        for over in [0.0, 0.1, -0.08] {
            let stroke = cloud(over);
            let nodes = offer(&stroke, Some(kit::ShapeKind::Ellipse)).expect("a closed curve");
            assert!(closes_smoothly(&nodes), "{over}: {nodes:?}");
            // a bend at each bulge and dip at least, and it starts where the stroke did
            assert!(nodes.len() / 3 >= 10, "{over}: {} anchors", nodes.len() / 3);
            assert!(length(sub(nodes[0], stroke[0])) < 0.1 * size(&stroke), "{over}: {:?}", nodes[0]);
            assert!(offer(&stroke, Some(kit::ShapeKind::Circle)).is_some());
        }
        // as it was drawn when reported: a zigzag down and up, round to the right and back along the top
        let corners = [
            (20.0, 0.0),
            (0.0, 60.0),
            (40.0, 40.0),
            (30.0, 100.0),
            (60.0, 60.0),
            (90.0, 90.0),
            (120.0, 30.0),
            (60.0, 0.0),
            (28.0, 4.0),
        ];
        let stroke: Vec<P> = corners
            .windows(2)
            .flat_map(|w| (0..20).map(move |i| add(w[0], scale(sub(w[1], w[0]), i as f32 / 20.0))))
            .collect();
        assert!(offer(&stroke, Some(kit::ShapeKind::Ellipse)).is_some_and(|n| closes_smoothly(&n)));
    }

    #[test]
    fn a_loop_that_only_bends_one_way_is_left_the_ellipse_it_is() {
        let wobbly = loop_of(|t| 150.0 + 4.0 * (9.0 * t).sin(), 200, 0.05);
        assert_eq!(offer(&wobbly, Some(kit::ShapeKind::Ellipse)), None);
        assert_eq!(offer(&wobbly, Some(kit::ShapeKind::Circle)), None);
        // asked for by name, it closes as a curve all the same
        assert!(offer(&wobbly, None).is_some_and(|n| closes_smoothly(&n)));
        // and polygons stay QuickShape's
        assert_eq!(offer(&cloud(0.0), Some(kit::ShapeKind::Quadrilateral)), None);
    }
}
