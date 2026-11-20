//! Lines and rectangles a view draws.

use rune::{El, LineCap, Shape, canvas};

use crate::lives::Shows;

use super::{Scope, ShapesNode};

pub(super) fn drawing(scope: &Scope, d: &ShapesNode) -> El {
    let theme = &scope.state.theme;
    let mut shape = Shape::new();
    for lines in &d.lines {
        let colour = theme.color(&lines.color);
        for run in runs(&lines.points) {
            // Round, so the joints of a run, such as an arc, show no seams; but round joints
            // overlap, which shows in a see-through colour.
            let cap = match run.len() > 2 && colour.a >= 1.0 {
                true => LineCap::Round,
                false => LineCap::Butt,
            };
            shape = shape.stroke_with(run, false, lines.width, cap, colour);
        }
    }
    for rects in &d.rects {
        let colour = theme.color(&rects.color);
        for rect in rects.rects.chunks_exact(4) {
            let [x, y, w, h] = [rect[0], rect[1], rect[2], rect[3]];
            shape = shape.fill(vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]], colour);
        }
    }
    if let Some([across, down]) = d.units.or_else(|| scope.place_units(d.place.as_ref())) {
        shape = shape.view_box(across, down);
    }
    let mut trim = None;
    match d.trim.as_ref().map(|[from, to]| (from.fixed(), to.fixed())) {
        Some((Some(from), Some(to))) => shape = shape.trimmed(from, to),
        Some(_) => trim = d.trim.clone(),
        None => {}
    }
    let mut e = canvas(shape).size(d.w, d.h);
    if let Some(trim) = trim {
        e = scope.bind(e, Shows::Trim(trim));
    }
    if let Some(key) = &d.key {
        e = e.key(scope.key(key));
    }
    scope.placed(e, d.at.as_ref(), (None, None), d.opacity.as_ref())
}

/// Line segments (x1 y1 x2 y2 each) as strokes: a segment that starts where the last ended goes
/// on with it, so an arc of short segments draws as one line, without seams.
fn runs(points: &[f32]) -> Vec<Vec<[f32; 2]>> {
    let mut runs: Vec<Vec<[f32; 2]>> = Vec::new();
    for segment in points.chunks_exact(4) {
        let (from, to) = ([segment[0], segment[1]], [segment[2], segment[3]]);
        match runs.last_mut() {
            Some(run) if run.last() == Some(&from) => run.push(to),
            _ => runs.push(vec![from, to]),
        }
    }
    runs
}
