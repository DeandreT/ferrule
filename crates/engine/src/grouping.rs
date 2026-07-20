use mapping::NodeId;

#[derive(Clone, Copy)]
pub(super) enum GroupingMode {
    By(NodeId),
    AdjacentBy(NodeId),
    StartingWith(NodeId),
    EndingWith(NodeId),
    IntoBlocks(usize),
}
