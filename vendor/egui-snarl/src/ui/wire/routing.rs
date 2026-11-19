use crate::NodeId;
use egui::{Pos2, Rect, pos2};

pub(in crate::ui) struct Lane {
    bounds: Rect,
    points: Vec<Pos2>,
}

impl Lane {
    pub(super) fn new(points: &[Pos2]) -> Self {
        Self {
            bounds: Rect::from_points(points),
            points: points.to_vec(),
        }
    }
}

/// Clip against the open interior. Following the perimeter of an expanded
/// card is legal and keeps the visible stroke clear of the actual frame.
fn crosses(a: Pos2, b: Pos2, r: Rect) -> bool {
    let mut enter = 0.0_f32;
    let mut leave = 1.0_f32;
    for (start, end, lo, hi) in [
        (a.x, b.x, r.left(), r.right()),
        (a.y, b.y, r.top(), r.bottom()),
    ] {
        let delta = end - start;
        if delta == 0.0 {
            if start <= lo || start >= hi {
                return false;
            }
        } else {
            let t = (lo - start) / delta;
            let u = (hi - start) / delta;
            enter = enter.max(t.min(u));
            leave = leave.min(t.max(u));
        }
    }
    enter < leave
}

pub(super) fn hit(points: &[Pos2], pos: Pos2, threshold: f32) -> bool {
    points.windows(2).any(|s| {
        let edge = s[1] - s[0];
        let t = if edge.length_sq() > 0.0 {
            ((pos - s[0]).dot(edge) / edge.length_sq()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        pos.distance_sq(s[0] + t * edge) <= threshold * threshold
    })
}

// For shared pins, ignore just the short mandatory fan-out/approach section.
// Keep the rest of each segment: two long coincident edges still need lanes.
fn trimmed(points: &[Pos2], ends: [Pos2; 2], gap: f32) -> impl Iterator<Item = [Pos2; 2]> {
    let start = if ends.contains(&points[0]) { gap } else { 0.0 };
    let end = if ends.contains(&points[points.len() - 1]) {
        points.windows(2).map(|s| s[0].distance(s[1])).sum::<f32>() - gap
    } else {
        f32::INFINITY
    };
    let mut at = 0.0;
    points.windows(2).filter_map(move |s| {
        let len = s[0].distance(s[1]);
        let lo = (start - at).max(0.0);
        let hi = (end - at).min(len);
        at += len;
        (lo < hi).then(|| [s[0].lerp(s[1], lo / len), s[0].lerp(s[1], hi / len)])
    })
}

fn near(a: [Pos2; 2], b: [Pos2; 2], gap: f32) -> bool {
    if !Rect::from_two_pos(a[0], a[1])
        .expand(gap)
        .intersects(Rect::from_two_pos(b[0], b[1]))
    {
        return false;
    }
    let orient = |s: [Pos2; 2], p: Pos2| {
        let d = s[1] - s[0];
        let v = p - s[0];
        d.x * v.y - d.y * v.x
    };
    (orient(a, b[0]) * orient(a, b[1]) < 0.0 && orient(b, a[0]) * orient(b, a[1]) < 0.0)
        || a.iter().any(|&p| hit(&b, p, gap))
        || b.iter().any(|&p| hit(&a, p, gap))
}

fn conflicts(points: &[Pos2], lanes: &[Lane], gap: f32) -> (usize, Rect) {
    let mut count = 0;
    let mut occupied = Rect::NOTHING;
    let bounds = Rect::from_points(points).expand(gap);
    let ends = [points[0], points[points.len() - 1]];
    for lane in lanes.iter().filter(|lane| bounds.intersects(lane.bounds)) {
        let other = &lane.points;
        let other_ends = [other[0], other[other.len() - 1]];
        let previous: Vec<_> = trimmed(other, ends, 3.0 * gap).collect();
        if trimmed(points, other_ends, 3.0 * gap).any(|s| previous.iter().any(|&p| near(s, p, gap)))
        {
            count += 1;
            occupied = occupied.union(lane.bounds.expand(1.5 * gap));
        }
    }
    (count, occupied)
}

pub(super) fn detour(
    line: &[Pos2],
    cards: &[(NodeId, Rect)],
    source: NodeId,
    target: NodeId,
    clearance: f32,
    lanes: &[Lane],
) -> Option<Vec<Pos2>> {
    let (&from, &to) = (line.first()?, line.last()?);
    let mut blocked = Rect::NOTHING;
    let line_bounds = Rect::from_points(line);
    let mut exit = from.x + clearance;
    let mut entry = to.x - clearance;
    for &(id, rect) in cards {
        let r = rect.expand(clearance);
        if id == source {
            exit = exit.max(r.right());
        }
        if id == target {
            entry = entry.min(r.left());
        }
        if id != source
            && id != target
            && line_bounds.intersects(r)
            && line.windows(2).any(|s| crosses(s[0], s[1], r))
        {
            blocked = blocked.union(r);
        }
    }
    let has_node_blocker = blocked.is_positive();
    let (lane_conflicts, lane_bounds) = if lanes.is_empty() {
        (0, Rect::NOTHING)
    } else {
        conflicts(line, lanes, clearance)
    };
    blocked = blocked.union(lane_bounds);
    if !blocked.is_positive() {
        return None;
    }

    // Walk around the obstructing group on each side. Include any other card
    // met along a candidate, not just cards hit by the original curve. The
    // second pair of candidates can turn directly outside the endpoint cards.
    let mut best = None;
    let original_score = if has_node_blocker {
        usize::MAX
    } else {
        lane_conflicts
    };
    let mut best_score = original_score;
    let mut shortest = f32::INFINITY;
    for near_blocker in [true, false] {
        for above in [true, false] {
            let mut group = blocked;
            for _ in 0..8 {
                let (x1, x2) = if near_blocker && exit <= entry {
                    (
                        group.left().clamp(exit, entry),
                        group.right().clamp(exit, entry),
                    )
                } else {
                    (exit, entry)
                };
                let y = if above { group.top() } else { group.bottom() };
                let route = [
                    from,
                    pos2(x1, from.y),
                    pos2(x1, y),
                    pos2(x2, y),
                    pos2(x2, to.y),
                    to,
                ];
                let mut next = group;
                let mut clear = true;
                for &(id, rect) in cards {
                    let r = rect.expand(clearance);
                    if route.windows(2).enumerate().any(|(i, s)| {
                        !(i == 0 && id == source || i == 4 && id == target)
                            && crosses(s[0], s[1], r)
                    }) {
                        clear = false;
                        next = next.union(r);
                    }
                }
                if clear {
                    let (score, occupied) = conflicts(&route, lanes, clearance);
                    let length = route.windows(2).map(|s| s[0].distance(s[1])).sum::<f32>();
                    if score < original_score
                        && (score < best_score || score == best_score && length < shortest)
                    {
                        shortest = length;
                        best_score = score;
                        let mut points = route.to_vec();
                        points.dedup();
                        best = Some(points);
                    }
                    next = next.union(occupied);
                }
                if next == group {
                    break;
                }
                group = next;
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn turning_avoidance_on_and_off_rebuilds_paths_and_hit_targets() {
        use super::super::{WireId, WireStyle, connected_detour, hit_wire};
        use crate::{InPinId, OutPinId};
        let ctx = egui::Context::default();
        let a = pos2(0.0, 10.0);
        let b = pos2(160.0, 10.0);
        let id = |n| WireId::Connected {
            snarl_id: egui::Id::unique("routes"),
            out_pin: OutPinId {
                node: NodeId(0),
                output: 0,
            },
            in_pin: InPinId {
                node: NodeId(n),
                input: 0,
            },
        };
        for enabled in [false, true, false] {
            let mut lanes = enabled.then(Vec::new);
            for n in [1, 2] {
                let route = connected_detour(
                    &ctx,
                    id(n),
                    10.0,
                    false,
                    false,
                    a,
                    b,
                    1.0,
                    WireStyle::Line,
                    &[],
                    6.0,
                    lanes.as_mut(),
                );
                let at = |pos| {
                    hit_wire(
                        &ctx,
                        id(n),
                        10.0,
                        false,
                        false,
                        a,
                        b,
                        pos,
                        2.0,
                        WireStyle::Line,
                        route.as_deref(),
                    )
                };
                assert_eq!(at(pos2(80.0, 10.0)), !(enabled && n == 2));
                if enabled && n == 2 {
                    assert!(at(pos2(80.0, 1.0)));
                }
            }
        }
    }

    #[test]
    fn optional_lanes_split_a_long_shared_pin_run_but_keep_both_pins() {
        let line = [pos2(0.0, 0.0), pos2(200.0, 0.0)];
        let path = |lanes: &[Lane]| detour(&line, &[], NodeId(0), NodeId(1), 6.0, lanes);
        assert!(path(&[]).is_none());
        let lanes = [Lane::new(&line)];
        let split = path(&lanes).unwrap();
        assert_eq!(split.first(), line.first());
        assert_eq!(split.last(), line.last());
        assert!(!hit(&split, pos2(100.0, 0.0), 2.0));
        assert_eq!(path(&lanes).unwrap(), split);
        // The same pin alone is not an overlap: two edges immediately diverge.
        let divergent = [line[0], pos2(0.0, -200.0)];
        assert!(detour(&divergent, &[], NodeId(0), NodeId(2), 6.0, &lanes).is_none());
    }

    #[test]
    fn prefers_clear_crossing_detour_but_preserves_connection_when_enclosed() {
        let line = [pos2(0.0, 0.0), pos2(200.0, 0.0)];
        let cut = [Lane::new(&[pos2(100.0, -20.0), pos2(100.0, 20.0)])];
        let split = detour(&line, &[], NodeId(0), NodeId(1), 6.0, &cut).unwrap();
        assert!(!hit(&split, pos2(100.0, 0.0), 2.0));
        let walls = [
            (
                NodeId(2),
                Rect::from_min_max(pos2(-50.0, -100.0), pos2(260.0, -7.0)),
            ),
            (
                NodeId(3),
                Rect::from_min_max(pos2(-50.0, 7.0), pos2(260.0, 100.0)),
            ),
        ];
        // The free horizontal corridor cannot fit a lane around the vertical edge.
        assert!(detour(&line, &walls, NodeId(0), NodeId(1), 6.0, &cut).is_none());
        assert!(hit(&line, pos2(100.0, 0.0), 2.0));
    }

    #[test]
    fn blocker_move_resize_and_layout_use_current_pin_and_frame_geometry() {
        let a = NodeId(0);
        let b = NodeId(1);
        let c = NodeId(2);
        let line = [pos2(0.0, 0.0), pos2(200.0, 0.0)];
        let small = Rect::from_min_max(pos2(60.0, 20.0), pos2(100.0, 45.0));
        let moved = small.translate(vec2(0.0, -30.0));
        let resized = Rect::from_min_max(moved.min - vec2(10.0, 60.0), moved.max);
        let path = |line: &[Pos2], rect| detour(line, &[(c, rect)], a, b, 6.0, &[]);
        assert!(path(&line, small).is_none());
        let detour = path(&line, moved).unwrap();
        assert_eq!(detour.first(), line.first());
        assert_eq!(detour.last(), line.last());
        assert!(
            detour
                .windows(2)
                .all(|s| !crosses(s[0], s[1], moved.expand(6.0)))
        );
        assert!(hit(&detour, pos2(80.0, -16.0), 2.0));
        assert!(!hit(&detour, pos2(80.0, 0.0), 2.0));
        let tall = path(&line, resized).unwrap();
        assert!(hit(&tall, pos2(80.0, 21.0), 2.0));
        assert!(!hit(&tall, pos2(80.0, -16.0), 2.0));
        assert!(path(&line, small).is_none());
        let arranged = [pos2(-40.0, 90.0), pos2(210.0, 90.0)];
        assert!(path(&arranged, resized).is_none());
    }
}
