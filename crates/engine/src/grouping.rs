use mapping::NodeId;

#[derive(Clone, Copy)]
pub(super) enum GroupingMode {
    By(NodeId),
    StartingWith(NodeId),
    IntoBlocks(usize),
}
