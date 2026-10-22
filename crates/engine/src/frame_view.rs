//! Prepared correspondence between borrowed source instances and legacy
//! position frames.
//!
//! Legacy evaluation keeps instances and position frames in parallel lists
//! whose correspondence is implicit: a receiving context pairs a position
//! with an instance by counting back from the context's end past synthetic
//! group wrappers, and a joined row pairs its two lists by index. A frame view
//! makes that pairing explicit once, so a lookup selects one frame by its
//! exact collection and reads the frame's instance, kind and origin together.
//!
//! The view is read-only and additive. Both lists stay authoritative, and the
//! view reproduces their legacy pairing exactly; each frame's endpoint names
//! which pairing rule produced its instance, including the legacy fallbacks.

use ir::Instance;
use mapping::JoinId;

use crate::source_iteration::PositionFrame;

/// Which instance, if any, corresponds to a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameEndpoint {
    /// The receiving context instance at this depth, outermost first.
    Context(usize),
    /// The joined row's own instance at this index. Every source of the row
    /// contributed exactly one instance per position.
    Row(usize),
    /// Legacy index pairing in a row whose sources did not all contribute one
    /// instance per position. The instance at this index may belong to
    /// another source, or may not exist.
    LegacyRow(usize),
    /// The receiving context is shorter than its positions and wrappers, so
    /// legacy lookup finds no instance for any of its positions.
    Unbound,
}

/// How a frame came to be active or joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameOrigin {
    /// An ordinary iteration frame of the receiving context.
    Iterated,
    /// A receiving frame contributed by a joined row of this join.
    Joined(JoinId),
    /// A join source that reuses the latest active receiving frame of its
    /// exact collection instead of walking it again.
    Correlated,
    /// A join source walked from a receiving-context base.
    Walked,
    /// A join source whose walk crossed no repetition, so its one position is
    /// synthesized rather than read from the source.
    Synthetic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameKind {
    /// One item of a repeating collection.
    Item,
    /// One member of a document set; the position retains its path.
    Document,
    /// The member collection of one group, behind its named wrapper.
    GroupMembers,
}

impl FrameKind {
    fn of(position: &PositionFrame) -> Self {
        if position.grouped {
            Self::GroupMembers
        } else if position.document_path.is_some() {
            Self::Document
        } else {
            Self::Item
        }
    }
}

pub(crate) struct Frame<'v> {
    pub endpoint: FrameEndpoint,
    /// The frame's exact collection identity.
    pub collection: &'v [String],
    /// The legacy frame itself. Its `index` is the raw structural source
    /// position of a walked or synthetic join frame, and the enclosing
    /// scope's selection or output position for an active receiving frame,
    /// which a correlated join frame keeps. `join_position` is the flattened
    /// tuple position once a joined row has one.
    pub position: &'v PositionFrame,
    pub origin: FrameOrigin,
    pub kind: FrameKind,
}

pub(crate) struct FrameView<'v, 'a> {
    instances: &'v [&'a Instance],
    frames: Vec<Frame<'v>>,
}

impl<'v, 'a> FrameView<'v, 'a> {
    /// Pairs each receiving position with its context instance, as legacy
    /// position lookup does: positions and group wrappers occupy the end of
    /// the context, and a grouped position's instance follows its wrapper.
    pub(crate) fn receiving(context: &'v [&'a Instance], positions: &'v [PositionFrame]) -> Self {
        let offset = receiving_offset(context, positions);
        let mut preceding_wrappers = 0;
        let frames = positions
            .iter()
            .enumerate()
            .map(|(index, position)| {
                preceding_wrappers += usize::from(position.grouped);
                receiving_frame(offset, index, preceding_wrappers, position)
            })
            .collect();
        Self {
            instances: context,
            frames,
        }
    }

    /// The latest receiving frame satisfying `select`, innermost first, with
    /// its instance, paired exactly as [`FrameView::receiving`] pairs it but
    /// without preparing the whole view. An unbound frame is still selected
    /// and has no instance; selection never falls back to an earlier frame.
    pub(crate) fn latest_receiving(
        context: &'v [&'a Instance],
        positions: &'v [PositionFrame],
        select: impl Fn(&Frame<'v>) -> bool,
    ) -> Option<(Frame<'v>, Option<&'a Instance>)> {
        let offset = receiving_offset(context, positions);
        // Wrappers at or before each position, counted down from the end.
        let mut preceding_wrappers = positions.iter().filter(|position| position.grouped).count();
        for (index, position) in positions.iter().enumerate().rev() {
            let frame = receiving_frame(offset, index, preceding_wrappers, position);
            preceding_wrappers -= usize::from(position.grouped);
            if select(&frame) {
                let instance = instance_at(context, frame.endpoint);
                return Some((frame, instance));
            }
        }
        None
    }

    /// The collection of the innermost receiving frame when that frame is a
    /// group's member collection and its instance equals `base`: an
    /// empty-path walk from `base` iterates those members and keeps the
    /// grouped collection's identity. An unbound frame never matches.
    pub(crate) fn grouped_members_collection(
        context: &'v [&'a Instance],
        positions: &'v [PositionFrame],
        base: &Instance,
    ) -> Option<&'v [String]> {
        let (frame, instance) = Self::latest_receiving(context, positions, |_| true)?;
        (frame.kind == FrameKind::GroupMembers && instance == Some(base))
            .then_some(frame.collection)
    }

    /// Where the receiving positions and their group wrappers start in the
    /// context, or `None` when the context is too short to bind them.
    pub(crate) fn receiving_start(
        context: &[&Instance],
        positions: &[PositionFrame],
    ) -> Option<usize> {
        receiving_offset(context, positions)
    }

    /// Pairs a joined row's positions with its instances by index. `exact`
    /// records whether every source contributed one instance per position;
    /// otherwise the endpoints name the legacy index pairing.
    pub(crate) fn row(
        instances: &'v [&'a Instance],
        positions: impl IntoIterator<Item = (&'v PositionFrame, FrameOrigin)>,
        exact: bool,
    ) -> Self {
        let frames = positions
            .into_iter()
            .enumerate()
            .map(|(index, (position, origin))| Frame {
                endpoint: if exact {
                    FrameEndpoint::Row(index)
                } else {
                    FrameEndpoint::LegacyRow(index)
                },
                collection: &position.collection,
                position,
                origin,
                kind: FrameKind::of(position),
            })
            .collect();
        Self { instances, frames }
    }

    #[cfg(test)]
    pub(crate) fn frames(&self) -> &[Frame<'v>] {
        &self.frames
    }

    /// The latest frame satisfying `select`, innermost first.
    pub(crate) fn latest(&self, select: impl Fn(&Frame<'v>) -> bool) -> Option<&Frame<'v>> {
        self.frames.iter().rev().find(|frame| select(frame))
    }

    /// The borrowed instance corresponding to `frame`.
    pub(crate) fn instance(&self, frame: &Frame<'v>) -> Option<&'a Instance> {
        instance_at(self.instances, frame.endpoint)
    }
}

/// Where the receiving positions and their wrappers start in the context, or
/// `None` when the context is too short to hold them.
fn receiving_offset(context: &[&Instance], positions: &[PositionFrame]) -> Option<usize> {
    let wrappers = positions.iter().filter(|position| position.grouped).count();
    context.len().checked_sub(positions.len() + wrappers)
}

/// The receiving frame at `index`, after `preceding_wrappers` group wrappers
/// at or before it.
fn receiving_frame(
    offset: Option<usize>,
    index: usize,
    preceding_wrappers: usize,
    position: &PositionFrame,
) -> Frame<'_> {
    Frame {
        endpoint: offset.map_or(FrameEndpoint::Unbound, |offset| {
            FrameEndpoint::Context(offset + index + preceding_wrappers)
        }),
        collection: &position.collection,
        position,
        origin: position
            .join
            .map_or(FrameOrigin::Iterated, FrameOrigin::Joined),
        kind: FrameKind::of(position),
    }
}

fn instance_at<'a>(instances: &[&'a Instance], endpoint: FrameEndpoint) -> Option<&'a Instance> {
    match endpoint {
        FrameEndpoint::Context(index)
        | FrameEndpoint::Row(index)
        | FrameEndpoint::LegacyRow(index) => instances.get(index).copied(),
        FrameEndpoint::Unbound => None,
    }
}
