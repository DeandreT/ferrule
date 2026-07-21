//! Small, allocation-free catalog of common ANSI X12 segment identifiers.
//!
//! Transaction-set implementation guides remain the authority for a
//! segment's permitted position and elements. These descriptions are only
//! concise UI labels and deliberately do not encode guide-specific rules.

const MAX_SEGMENT_ID_BYTES: usize = 3;

/// Returns a concise description for a common canonical X12 segment ID.
///
/// Lookup accepts only two- or three-byte uppercase ASCII identifiers and is
/// bounded by the fixed catalog below. Aliases and descriptive schema node
/// names intentionally return `None`.
pub fn segment_description(id: &str) -> Option<&'static str> {
    if !(2..=MAX_SEGMENT_ID_BYTES).contains(&id.len())
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        return None;
    }
    SEGMENTS
        .binary_search_by_key(&id, |(segment, _)| *segment)
        .ok()
        .map(|index| SEGMENTS[index].1)
}

// Keep this sorted by segment ID; the test below enforces order and uniqueness.
const SEGMENTS: &[(&str, &str)] = &[
    ("AAA", "Request Validation"),
    ("ACK", "Line Item Acknowledgment"),
    ("AK1", "Functional Group Response Header"),
    ("AK2", "Transaction Set Response Header"),
    ("AK3", "Data Segment Note"),
    ("AK4", "Data Element Note"),
    ("AK5", "Transaction Set Response Trailer"),
    ("AK9", "Functional Group Response Trailer"),
    ("AMT", "Monetary Amount"),
    ("AT7", "Shipment Status Details"),
    (
        "B10",
        "Beginning Segment for Transportation Carrier Shipment Status",
    ),
    ("BEG", "Beginning Segment for Purchase Order"),
    ("BFR", "Beginning Segment for Planning Schedule"),
    ("BIG", "Beginning Segment for Invoice"),
    ("BPR", "Financial Information"),
    ("BSN", "Beginning Segment for Ship Notice"),
    ("CAS", "Claims Adjustment"),
    ("CLM", "Health Claim"),
    ("CN1", "Contract Information"),
    ("COB", "Coordination of Benefits"),
    ("CRC", "Conditions Indicator"),
    ("CRD", "Ambulance Certification"),
    ("CRF", "Chiropractic Certification"),
    ("CTT", "Transaction Totals"),
    ("CUR", "Currency"),
    ("DMG", "Demographic Information"),
    ("DTM", "Date/Time Reference"),
    ("DTP", "Date or Time or Period"),
    ("EB", "Eligibility or Benefit Information"),
    ("ENT", "Entity"),
    ("EQ", "Eligibility or Benefit Inquiry Information"),
    ("GE", "Functional Group Trailer"),
    ("GS", "Functional Group Header"),
    ("HCP", "Health Care Pricing"),
    ("HI", "Health Care Information Codes"),
    ("HL", "Hierarchical Level"),
    ("HLH", "Health Related Information"),
    ("IEA", "Interchange Control Trailer"),
    ("ISA", "Interchange Control Header"),
    ("IT1", "Baseline Item Data for Invoice"),
    ("ITD", "Terms of Sale or Deferred Terms of Sale"),
    ("L11", "Business Instructions and Reference Number"),
    ("LE", "Loop Trailer"),
    ("LIN", "Item Identification"),
    ("LS", "Loop Header"),
    ("LX", "Assigned Number"),
    ("MEA", "Measurements"),
    ("MSG", "Message Text"),
    ("N1", "Party Identification"),
    ("N2", "Additional Name Information"),
    ("N3", "Party Location"),
    ("N4", "Geographic Location"),
    ("NM1", "Individual or Organizational Name"),
    ("NTE", "Note or Special Instruction"),
    ("PER", "Administrative Communications Contact"),
    ("PID", "Product or Item Description"),
    ("PO1", "Baseline Item Data"),
    ("PRV", "Provider Information"),
    ("PWK", "Paperwork"),
    ("QTY", "Quantity Information"),
    ("REF", "Reference Information"),
    ("S5", "Stop-off Details"),
    (
        "SAC",
        "Service, Promotion, Allowance, or Charge Information",
    ),
    ("SBR", "Subscriber Information"),
    ("SE", "Transaction Set Trailer"),
    ("SLN", "Subline Item Detail"),
    ("ST", "Transaction Set Header"),
    ("SV1", "Professional Service"),
    ("SV2", "Institutional Service"),
    ("TA1", "Interchange Acknowledgment"),
    ("TD1", "Carrier Details for Quantity and Weight"),
    (
        "TD5",
        "Carrier Details for Routing Sequence or Transit Time",
    ),
    ("TOO", "Tooth Identification"),
    ("TRN", "Trace"),
    ("TXI", "Tax Information"),
    ("W01", "Warehouse Shipping Order Identification"),
    ("W12", "Warehouse Item Detail"),
    ("W17", "Warehouse Receipt Identification"),
    ("W20", "Warehouse Adjustment Identification"),
    ("W27", "Carrier Detail"),
    ("W66", "Warehouse Carrier Information"),
    ("W76", "Total Shipping Order"),
    ("W77", "Total Received Quantity"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_common_envelope_transaction_and_business_segments() {
        assert_eq!(
            segment_description("ISA"),
            Some("Interchange Control Header")
        );
        assert_eq!(segment_description("ST"), Some("Transaction Set Header"));
        assert_eq!(
            segment_description("BEG"),
            Some("Beginning Segment for Purchase Order")
        );
    }

    #[test]
    fn rejects_noncanonical_and_unknown_names() {
        assert_eq!(segment_description("isa"), None);
        assert_eq!(segment_description("ISA-loop"), None);
        assert_eq!(segment_description("XYZ"), None);
    }

    #[test]
    fn catalog_is_sorted_unique_and_bounded() {
        assert!(SEGMENTS.len() <= 128);
        assert!(SEGMENTS.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(SEGMENTS.iter().all(|(id, description)| {
            (2..=MAX_SEGMENT_ID_BYTES).contains(&id.len()) && !description.is_empty()
        }));
    }
}
