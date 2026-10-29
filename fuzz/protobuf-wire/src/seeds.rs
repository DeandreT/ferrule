//! Deterministic, self-authored seed inputs.
//!
//! `cargo run --example write_seeds` writes these to `seeds/`, and the seed
//! test fails if the checked-in files drift from this generator.

const VARINT: u64 = 0;
const FIXED64: u64 = 1;
const LEN: u64 = 2;
const START_GROUP: u64 = 3;
const END_GROUP: u64 = 4;
const FIXED32: u64 = 5;

/// Field numbers of `fuzz.wire.Root`.
mod root {
    pub const SCALARS: u64 = 1;
    pub const PACKED: u64 = 2;
    pub const LEAVES: u64 = 3;
    pub const NAME: u64 = 4;
    pub const CODE: u64 = 5;
    pub const LEAF: u64 = 6;
    pub const COUNTS: u64 = 7;
    pub const BY_ID: u64 = 8;
    pub const TOGGLES: u64 = 9;
    pub const TREE: u64 = 10;
    pub const LAST: u64 = 536_870_911;
}

/// Encodes `value` as a base-128 varint.
pub fn varint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

fn key(field: u64, wire: u64) -> Vec<u8> {
    varint((field << 3) | wire)
}

fn int(field: u64, value: u64) -> Vec<u8> {
    [key(field, VARINT), varint(value)].concat()
}

fn zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

fn fixed32(field: u64, value: [u8; 4]) -> Vec<u8> {
    [key(field, FIXED32), value.to_vec()].concat()
}

fn fixed64(field: u64, value: [u8; 8]) -> Vec<u8> {
    [key(field, FIXED64), value.to_vec()].concat()
}

fn bytes(field: u64, payload: &[u8]) -> Vec<u8> {
    [
        key(field, LEN),
        varint(payload.len() as u64),
        payload.to_vec(),
    ]
    .concat()
}

/// A length-delimited field whose declared length is `length` regardless of
/// how many payload bytes follow.
fn declared(field: u64, length: u64, payload: &[u8]) -> Vec<u8> {
    [key(field, LEN), varint(length), payload.to_vec()].concat()
}

fn leaf(label: &str, weight: u64) -> Vec<u8> {
    [bytes(1, label.as_bytes()), int(2, weight)].concat()
}

fn scalars_all() -> Vec<u8> {
    [
        int(1, -5i64 as u64),
        int(2, i64::MIN as u64),
        int(3, u64::from(u32::MAX)),
        int(4, i64::MAX as u64),
        int(5, zigzag(-3)),
        int(6, zigzag(i64::MIN)),
        int(7, 1),
        int(8, 2),
        fixed32(9, u32::MAX.to_le_bytes()),
        fixed64(10, (i64::MAX as u64).to_le_bytes()),
        fixed32(11, (-9i32).to_le_bytes()),
        fixed64(12, i64::MIN.to_le_bytes()),
        fixed32(13, 1.5f32.to_le_bytes()),
        fixed64(14, (-2.25f64).to_le_bytes()),
        bytes(15, "héllo".as_bytes()),
        bytes(16, b"blob"),
        int(17, 0),
    ]
    .concat()
}

fn packed_all() -> Vec<u8> {
    let i32s = [varint(1), varint(-1i64 as u64), varint(300)].concat();
    let s64s = [varint(zigzag(-1)), varint(zigzag(i64::MAX))].concat();
    let f32s = [7u32.to_le_bytes(), u32::MAX.to_le_bytes()].concat();
    let doubles = [0.5f64.to_le_bytes(), (-0.0f64).to_le_bytes()].concat();
    let colors = [varint(0), varint(1), varint(-1i64 as u64)].concat();
    [
        bytes(1, &i32s),
        int(1, 42),
        bytes(2, &s64s),
        bytes(3, &f32s),
        bytes(4, &doubles),
        bytes(5, &[0, 1, 2]),
        bytes(6, &colors),
        int(7, 9),
        bytes(7, &[varint(10), varint(11)].concat()),
        bytes(8, b"one"),
        bytes(8, b""),
    ]
    .concat()
}

fn nested_nodes(levels: usize) -> Vec<u8> {
    let mut node = int(2, 1);
    for _ in 1..levels {
        node = bytes(1, &node);
    }
    node
}

fn unknown_group(levels: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..levels {
        out.extend(key(1000, START_GROUP));
    }
    for _ in 0..levels {
        out.extend(key(1000, END_GROUP));
    }
    out
}

fn legacy_full() -> Vec<u8> {
    let next = [bytes(1, b"inner"), int(2, 1)].concat();
    let entry = |name: &[u8], kind: u64| [bytes(1, name), int(2, kind)].concat();
    [
        bytes(1, b"outer"),
        int(3, zigzag(-1)),
        bytes(4, &[varint(1), varint(2)].concat()),
        int(4, 3),
        int(5, 4),
        bytes(5, &[varint(5), varint(6)].concat()),
        bytes(6, &next),
        bytes(6, &int(3, zigzag(8))),
        bytes(7, b"text"),
        fixed64(8, 3.0f64.to_le_bytes()),
        bytes(9, &entry(b"a", 1)),
        bytes(9, &entry(b"a", 2)),
        bytes(9, &[]),
        bytes(10, b"payload"),
    ]
    .concat()
}

/// Every seed as `(file stem, bytes)`, in a stable order.
pub fn all() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("empty", Vec::new()),
        ("scalars_all", bytes(root::SCALARS, &scalars_all())),
        ("scalars_direct", scalars_all()),
        (
            "scalars_merge",
            [
                bytes(root::SCALARS, &int(1, 1)),
                bytes(root::SCALARS, &[int(1, 2), int(2, 3)].concat()),
            ]
            .concat(),
        ),
        (
            "varint_ten_bytes",
            [key(2, VARINT), vec![0xff; 9], vec![0x01]].concat(),
        ),
        (
            "varint_overflow",
            [key(2, VARINT), vec![0xff; 9], vec![0x02]].concat(),
        ),
        (
            "varint_eleven_bytes",
            [key(2, VARINT), vec![0x80; 10], vec![0x00]].concat(),
        ),
        ("varint_truncated", [key(2, VARINT), vec![0x80]].concat()),
        ("key_truncated", vec![0x80]),
        ("uint32_overflow", int(3, 1 << 32)),
        ("uint64_out_of_range", int(4, 1 << 63)),
        ("sint32_overflow", int(5, 1 << 32)),
        (
            "fixed64_out_of_range",
            fixed64(10, (1u64 << 63).to_le_bytes()),
        ),
        (
            "fixed32_truncated",
            [key(9, FIXED32), vec![1, 2, 3]].concat(),
        ),
        ("double_nan", fixed64(14, f64::NAN.to_le_bytes())),
        ("float_infinity", fixed32(13, f32::INFINITY.to_le_bytes())),
        ("string_invalid_utf8", bytes(15, &[0xc3, 0x28])),
        ("wire_type_mismatch", bytes(1, b"x")),
        ("packed_all", bytes(root::PACKED, &packed_all())),
        ("packed_direct", packed_all()),
        ("packed_truncated_fixed", bytes(3, &[1, 2, 3])),
        ("packed_truncated_varint", bytes(1, &[0x80])),
        ("packed_bad_enum", bytes(6, &varint(9))),
        ("texts_empty_string", bytes(8, &[])),
        (
            "length_exact",
            [declared(root::NAME, 3, b"abc"), int(root::CODE, 1)].concat(),
        ),
        ("length_short_by_one", declared(root::NAME, 4, b"abc")),
        (
            "length_truncated_message",
            declared(root::LEAVES, 5, &[0x08]),
        ),
        ("length_u64_max", declared(root::NAME, u64::MAX, b"x")),
        (
            "length_i64_max",
            declared(root::NAME, i64::MAX as u64, b"x"),
        ),
        (
            "length_u32_max",
            declared(root::NAME, u64::from(u32::MAX), b""),
        ),
        ("length_missing", key(root::NAME, LEN)),
        (
            "leaves_repeated",
            [
                bytes(root::LEAVES, &leaf("a", 1)),
                bytes(root::LEAVES, &[]),
                bytes(root::LEAVES, &leaf("b", 2)),
            ]
            .concat(),
        ),
        (
            "oneof_last_wins",
            [
                bytes(root::NAME, b"first"),
                int(root::CODE, 7),
                bytes(root::LEAF, &leaf("x", 1)),
                bytes(root::LEAF, &int(2, 9)),
            ]
            .concat(),
        ),
        (
            "oneof_back_to_name",
            [
                bytes(root::LEAF, &leaf("x", 1)),
                bytes(root::NAME, b"again"),
            ]
            .concat(),
        ),
        (
            "map_entries",
            [
                bytes(root::COUNTS, &[bytes(1, b"a"), int(2, 1)].concat()),
                bytes(root::COUNTS, &[]),
                bytes(root::COUNTS, &[bytes(1, b"a"), int(2, 2)].concat()),
                bytes(
                    root::COUNTS,
                    &[int(2, 3), bytes(1, b"b"), int(99, 0)].concat(),
                ),
                bytes(
                    root::BY_ID,
                    &[int(1, -1i64 as u64), bytes(2, &leaf("l", 4))].concat(),
                ),
                bytes(root::BY_ID, &int(1, 5)),
                bytes(root::TOGGLES, &[int(1, 1), int(2, 2)].concat()),
                bytes(root::TOGGLES, &[int(1, 7), int(2, 1)].concat()),
            ]
            .concat(),
        ),
        ("map_bad_enum", bytes(root::TOGGLES, &int(2, 5))),
        (
            "map_truncated_entry",
            declared(root::COUNTS, 4, &bytes(1, b"a")),
        ),
        (
            "unknown_fields",
            [
                int(100, u64::MAX),
                fixed64(101, [0; 8]),
                bytes(102, b"skip"),
                fixed32(103, [0; 4]),
                key(104, START_GROUP),
                int(1, 1),
                bytes(2, b"nested"),
                key(5, START_GROUP),
                key(5, END_GROUP),
                key(104, END_GROUP),
                bytes(root::NAME, b"kept"),
            ]
            .concat(),
        ),
        ("unknown_group_depth_ok", unknown_group(128)),
        ("unknown_group_too_deep", unknown_group(129)),
        (
            "unknown_group_mismatched_end",
            [key(104, START_GROUP), key(105, END_GROUP)].concat(),
        ),
        ("unknown_group_unterminated", key(104, START_GROUP)),
        ("stray_end_group", key(104, END_GROUP)),
        ("wire_type_six", key(100, 6)),
        ("wire_type_seven", key(100, 7)),
        ("field_number_zero", int(0, 1)),
        ("field_number_max", int(root::LAST, u64::from(u32::MAX))),
        ("field_number_too_large", int(root::LAST + 1, 1)),
        ("field_key_beyond_u32", int(1 << 32, 1)),
        ("tree_depth_limit", bytes(root::TREE, &nested_nodes(128))),
        ("tree_depth_exceeded", bytes(root::TREE, &nested_nodes(129))),
        ("node_depth_limit", nested_nodes(129)),
        ("node_depth_exceeded", nested_nodes(130)),
        (
            "node_children",
            [
                bytes(3, &int(2, 1)),
                bytes(3, &bytes(3, &int(2, 2))),
                bytes(1, &int(2, 3)),
                bytes(1, &bytes(3, &[])),
            ]
            .concat(),
        ),
        ("legacy_minimal", bytes(1, b"id")),
        ("legacy_full", legacy_full()),
        ("legacy_bad_enum", [bytes(1, b"id"), int(2, 3)].concat()),
        (
            "legacy_nested_missing_required",
            [bytes(1, b"id"), bytes(6, &[])].concat(),
        ),
        (
            "legacy_bytes_invalid_utf8",
            [bytes(1, b"id"), bytes(10, &[0xff])].concat(),
        ),
    ]
}
