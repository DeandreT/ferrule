//! Internal Basic rendering for explicit-policy emitters and private tests.
//! Caller-prepared declarations must match every Program source name and original
//! ordinal; acquisition/supplied-input normalization belongs to #527/#519.
//! Five limits are required and cover qualified runtime primitive metadata, not
//! output trees, caller query literals or other externally held metadata.

use crate::{EmitError, literal, mapping};
use ::mapping::{NodeId, SourceOwner, SourceScopeLocation, SourceScopeStep, SourceScopeTarget};
use codegen::{
    Expression, IterationOutput, IterationSource, OwnedSourceCapabilityOrigin as Origin,
    OwnedSourceCapabilityReason as Reason, OwnedSourceDiagnostic as Diagnostic,
    OwnedSourceDiagnosticLocation as Location, OwnedSourceDiagnosticRequest as Request,
    OwnedSourceNamespace as Namespace, Program, ProgramValidationError, TargetConstruction,
    TargetScope,
};
use std::collections::BTreeSet;

mod aggregate;
mod row_failures;
mod search;
pub(crate) use aggregate::render_owned_aggregate;
pub(crate) use search::render_owned_search;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SequenceItemAtPolicy {
    Closed,
    BasicScalarProducers,
    /// The four scalar producers plus the owned recursive collection walk.
    BasicScalarAndOwnedRecursiveProducers,
    /// Also admits SequenceExists over the same pure producers.
    BasicScalarProducersWithExists,
    /// Also admits SequenceAggregate over the same pure producers.
    BasicScalarProducersWithExistsAndAggregate,
}

#[derive(Clone, Copy)]
pub(super) enum TargetControls {
    Closed,
    By,
    ByAndAdjacent,
    ByAndAdjacentAndStarting,
    ByAndAdjacentAndStartingAndEnding,
    ByAndAdjacentAndStartingAndEndingAndBlocks,
}

impl TargetControls {
    fn has_scoped_groups(self) -> bool {
        matches!(
            self,
            Self::By
                | Self::ByAndAdjacent
                | Self::ByAndAdjacentAndStarting
                | Self::ByAndAdjacentAndStartingAndEnding
                | Self::ByAndAdjacentAndStartingAndEndingAndBlocks
        )
    }

    pub(super) fn permits_grouping(self, grouping: Option<codegen::GroupingPlan>) -> bool {
        match grouping {
            Some(codegen::GroupingPlan::By { .. }) => self.has_scoped_groups(),
            Some(codegen::GroupingPlan::AdjacentBy { .. }) => matches!(
                self,
                Self::ByAndAdjacent
                    | Self::ByAndAdjacentAndStarting
                    | Self::ByAndAdjacentAndStartingAndEnding
                    | Self::ByAndAdjacentAndStartingAndEndingAndBlocks
            ),
            Some(codegen::GroupingPlan::StartingWith { .. }) => matches!(
                self,
                Self::ByAndAdjacentAndStarting
                    | Self::ByAndAdjacentAndStartingAndEnding
                    | Self::ByAndAdjacentAndStartingAndEndingAndBlocks
            ),
            Some(codegen::GroupingPlan::EndingWith { .. }) => matches!(
                self,
                Self::ByAndAdjacentAndStartingAndEnding
                    | Self::ByAndAdjacentAndStartingAndEndingAndBlocks
            ),
            Some(codegen::GroupingPlan::IntoBlocks { .. }) => {
                matches!(self, Self::ByAndAdjacentAndStartingAndEndingAndBlocks)
            }
            None => false,
        }
    }
}

#[derive(Clone, Copy)]
struct TargetExtension {
    controls: TargetControls,
    sequence_item_at: SequenceItemAtPolicy,
    /// Explicitly owned failure walks, beside the primary-row permission.
    failure_sources: bool,
    /// Owned path-hierarchy construction is an orthogonal explicit permission.
    path_hierarchy: bool,
    /// Owned adjacency-tree construction is an orthogonal explicit permission.
    adjacency_tree: bool,
    /// Recursive filtering of an owned selector is an orthogonal permission.
    recursive_filter: bool,
    /// Explicitly owned Lookup and CollectionFind, optionally with owned
    /// collection aggregates.
    owned_search: OwnedSearch,
    /// Owned dynamic source-field reads are an orthogonal explicit permission.
    dynamic_field: bool,
    /// Owned XML operators are one nested explicit expression permission.
    owned_xml: OwnedXml,
    /// Owned inner joins and their tuple reads.
    owned_joins: bool,
}

/// The owned XML companions nest: each admits the operators it composes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum OwnedXml {
    Closed,
    Serialize,
    MixedContent,
    MixedConstruction,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OwnedSearch {
    Closed,
    LookupFind,
    /// LookupFind plus owned collection aggregates.
    CollectionAggregates,
}

impl TargetExtension {
    const CLOSED: Self = Self {
        controls: TargetControls::Closed,
        sequence_item_at: SequenceItemAtPolicy::Closed,
        failure_sources: false,
        path_hierarchy: false,
        adjacency_tree: false,
        recursive_filter: false,
        owned_search: OwnedSearch::Closed,
        dynamic_field: false,
        owned_xml: OwnedXml::Closed,
        owned_joins: false,
    };
    const GROUP_BY: Self = Self {
        controls: TargetControls::By,
        sequence_item_at: SequenceItemAtPolicy::Closed,
        failure_sources: false,
        path_hierarchy: false,
        adjacency_tree: false,
        recursive_filter: false,
        owned_search: OwnedSearch::Closed,
        dynamic_field: false,
        owned_xml: OwnedXml::Closed,
        owned_joins: false,
    };
    const SEQUENCE_ITEM_AT: Self = Self {
        controls: TargetControls::By,
        sequence_item_at: SequenceItemAtPolicy::BasicScalarProducers,
        failure_sources: false,
        path_hierarchy: false,
        adjacency_tree: false,
        recursive_filter: false,
        owned_search: OwnedSearch::Closed,
        dynamic_field: false,
        owned_xml: OwnedXml::Closed,
        owned_joins: false,
    };

    const GROUP_ADJACENT_BY: Self = Self {
        controls: TargetControls::ByAndAdjacent,
        sequence_item_at: SequenceItemAtPolicy::BasicScalarProducers,
        failure_sources: false,
        path_hierarchy: false,
        adjacency_tree: false,
        recursive_filter: false,
        owned_search: OwnedSearch::Closed,
        dynamic_field: false,
        owned_xml: OwnedXml::Closed,
        owned_joins: false,
    };

    const SEQUENCE_EXISTS: Self = Self {
        controls: TargetControls::ByAndAdjacentAndStartingAndEnding,
        sequence_item_at: SequenceItemAtPolicy::BasicScalarProducersWithExists,
        failure_sources: false,
        path_hierarchy: false,
        adjacency_tree: false,
        recursive_filter: false,
        owned_search: OwnedSearch::Closed,
        dynamic_field: false,
        owned_xml: OwnedXml::Closed,
        owned_joins: false,
    };

    const SEQUENCE_AGGREGATE: Self = Self {
        controls: TargetControls::ByAndAdjacentAndStartingAndEnding,
        sequence_item_at: SequenceItemAtPolicy::BasicScalarProducersWithExistsAndAggregate,
        failure_sources: false,
        path_hierarchy: false,
        adjacency_tree: false,
        recursive_filter: false,
        owned_search: OwnedSearch::Closed,
        dynamic_field: false,
        owned_xml: OwnedXml::Closed,
        owned_joins: false,
    };

    const GROUP_STARTING_WITH: Self = Self {
        controls: TargetControls::ByAndAdjacentAndStarting,
        sequence_item_at: SequenceItemAtPolicy::BasicScalarProducers,
        failure_sources: false,
        path_hierarchy: false,
        adjacency_tree: false,
        recursive_filter: false,
        owned_search: OwnedSearch::Closed,
        dynamic_field: false,
        owned_xml: OwnedXml::Closed,
        owned_joins: false,
    };

    const FAILURE_SOURCES: Self = Self {
        controls: TargetControls::ByAndAdjacentAndStartingAndEnding,
        sequence_item_at: SequenceItemAtPolicy::BasicScalarProducers,
        failure_sources: true,
        path_hierarchy: false,
        adjacency_tree: false,
        recursive_filter: false,
        owned_search: OwnedSearch::Closed,
        dynamic_field: false,
        owned_xml: OwnedXml::Closed,
        owned_joins: false,
    };

    const GROUP_ENDING_WITH: Self = Self {
        controls: TargetControls::ByAndAdjacentAndStartingAndEnding,
        sequence_item_at: SequenceItemAtPolicy::BasicScalarProducers,
        failure_sources: false,
        path_hierarchy: false,
        adjacency_tree: false,
        recursive_filter: false,
        owned_search: OwnedSearch::Closed,
        dynamic_field: false,
        owned_xml: OwnedXml::Closed,
        owned_joins: false,
    };

    /// The newest companion: FailureSources plus owned hierarchy construction.
    const PATH_HIERARCHY: Self = Self {
        controls: TargetControls::ByAndAdjacentAndStartingAndEnding,
        sequence_item_at: SequenceItemAtPolicy::BasicScalarProducers,
        failure_sources: true,
        path_hierarchy: true,
        adjacency_tree: false,
        recursive_filter: false,
        owned_search: OwnedSearch::Closed,
        dynamic_field: false,
        owned_xml: OwnedXml::Closed,
        owned_joins: false,
    };

    /// PATH_HIERARCHY plus owned adjacency-tree construction.
    const ADJACENCY_TREE: Self = Self {
        controls: TargetControls::ByAndAdjacentAndStartingAndEnding,
        sequence_item_at: SequenceItemAtPolicy::BasicScalarProducers,
        failure_sources: true,
        path_hierarchy: true,
        adjacency_tree: true,
        recursive_filter: false,
        owned_search: OwnedSearch::Closed,
        dynamic_field: false,
        owned_xml: OwnedXml::Closed,
        owned_joins: false,
    };

    /// ADJACENCY_TREE plus ItemAt over owned recursive collection walks.
    const RECURSIVE_COLLECT: Self = Self {
        controls: TargetControls::ByAndAdjacentAndStartingAndEnding,
        sequence_item_at: SequenceItemAtPolicy::BasicScalarAndOwnedRecursiveProducers,
        failure_sources: true,
        path_hierarchy: true,
        adjacency_tree: true,
        recursive_filter: false,
        owned_search: OwnedSearch::Closed,
        dynamic_field: false,
        owned_xml: OwnedXml::Closed,
        owned_joins: false,
    };

    /// RECURSIVE_COLLECT plus recursive filtering of owned selectors.
    const RECURSIVE_FILTER: Self = Self {
        controls: TargetControls::ByAndAdjacentAndStartingAndEnding,
        sequence_item_at: SequenceItemAtPolicy::BasicScalarAndOwnedRecursiveProducers,
        failure_sources: true,
        path_hierarchy: true,
        adjacency_tree: true,
        recursive_filter: true,
        owned_search: OwnedSearch::Closed,
        dynamic_field: false,
        owned_xml: OwnedXml::Closed,
        owned_joins: false,
    };

    /// RECURSIVE_FILTER and IntoBlocks controls plus owned Lookup and
    /// CollectionFind.
    const LOOKUP_FIND: Self = Self {
        controls: TargetControls::ByAndAdjacentAndStartingAndEndingAndBlocks,
        sequence_item_at: SequenceItemAtPolicy::BasicScalarAndOwnedRecursiveProducers,
        failure_sources: true,
        path_hierarchy: true,
        adjacency_tree: true,
        recursive_filter: true,
        owned_search: OwnedSearch::LookupFind,
        dynamic_field: false,
        owned_xml: OwnedXml::Closed,
        owned_joins: false,
    };

    /// LOOKUP_FIND plus owned dynamic source-field reads.
    const DYNAMIC_FIELD: Self = Self {
        controls: TargetControls::ByAndAdjacentAndStartingAndEndingAndBlocks,
        sequence_item_at: SequenceItemAtPolicy::BasicScalarAndOwnedRecursiveProducers,
        failure_sources: true,
        path_hierarchy: true,
        adjacency_tree: true,
        recursive_filter: true,
        owned_search: OwnedSearch::LookupFind,
        dynamic_field: true,
        owned_xml: OwnedXml::Closed,
        owned_joins: false,
    };

    /// DYNAMIC_FIELD plus owned XML serialization.
    const XML_SERIALIZE: Self = Self {
        controls: TargetControls::ByAndAdjacentAndStartingAndEndingAndBlocks,
        sequence_item_at: SequenceItemAtPolicy::BasicScalarAndOwnedRecursiveProducers,
        failure_sources: true,
        path_hierarchy: true,
        adjacency_tree: true,
        recursive_filter: true,
        owned_search: OwnedSearch::LookupFind,
        dynamic_field: true,
        owned_xml: OwnedXml::Serialize,
        owned_joins: false,
    };

    /// LOOKUP_FIND plus owned inner joins and their tuple reads.
    const JOINS: Self = Self {
        controls: TargetControls::ByAndAdjacentAndStartingAndEndingAndBlocks,
        sequence_item_at: SequenceItemAtPolicy::BasicScalarAndOwnedRecursiveProducers,
        failure_sources: true,
        path_hierarchy: true,
        adjacency_tree: true,
        recursive_filter: true,
        owned_search: OwnedSearch::LookupFind,
        dynamic_field: false,
        owned_xml: OwnedXml::Closed,
        owned_joins: true,
    };

    /// LOOKUP_FIND plus owned collection aggregates.
    const COLLECTION_AGGREGATES: Self = Self {
        owned_search: OwnedSearch::CollectionAggregates,
        ..Self::LOOKUP_FIND
    };

    fn permits_path_hierarchy(self) -> bool {
        self.path_hierarchy
    }

    const GROUP_INTO_BLOCKS: Self = Self {
        controls: TargetControls::ByAndAdjacentAndStartingAndEndingAndBlocks,
        sequence_item_at: SequenceItemAtPolicy::BasicScalarProducers,
        failure_sources: false,
        path_hierarchy: false,
        adjacency_tree: false,
        recursive_filter: false,
        owned_search: OwnedSearch::Closed,
        dynamic_field: false,
        owned_xml: OwnedXml::Closed,
        owned_joins: false,
    };

    /// XML_SERIALIZE plus owned XML mixed-content atomization.
    const XML_MIXED_CONTENT: Self = Self {
        owned_xml: OwnedXml::MixedContent,
        ..Self::XML_SERIALIZE
    };

    /// XML_MIXED_CONTENT plus mixed-content construction by an owned selector.
    const XML_MIXED_CONSTRUCTION: Self = Self {
        owned_xml: OwnedXml::MixedConstruction,
        ..Self::XML_SERIALIZE
    };

    fn permits_xml_serialize(self) -> bool {
        self.owned_xml >= OwnedXml::Serialize
    }

    fn permits_xml_mixed_content(self) -> bool {
        self.owned_xml >= OwnedXml::MixedContent
    }

    fn permits_xml_mixed_construction(self) -> bool {
        self.owned_xml >= OwnedXml::MixedConstruction
    }

    fn permits_into_blocks(self) -> bool {
        matches!(
            self.controls,
            TargetControls::ByAndAdjacentAndStartingAndEndingAndBlocks
        )
    }

    fn permits_ending_with(self) -> bool {
        matches!(
            self.controls,
            TargetControls::ByAndAdjacentAndStartingAndEnding
        )
    }

    fn permits_starting_with(self) -> bool {
        matches!(self.controls, TargetControls::ByAndAdjacentAndStarting)
    }

    fn permits_adjacent_by(self) -> bool {
        matches!(self.controls, TargetControls::ByAndAdjacent)
    }

    fn permits_sequence_item_at(self) -> bool {
        self.sequence_item_at != SequenceItemAtPolicy::Closed
    }

    fn permits_sequence_exists(self) -> bool {
        matches!(
            self.sequence_item_at,
            SequenceItemAtPolicy::BasicScalarProducersWithExists
                | SequenceItemAtPolicy::BasicScalarProducersWithExistsAndAggregate
        )
    }

    fn permits_sequence_aggregate(self) -> bool {
        self.sequence_item_at == SequenceItemAtPolicy::BasicScalarProducersWithExistsAndAggregate
    }

    fn permits_owned_recursive_collect(self) -> bool {
        self.sequence_item_at == SequenceItemAtPolicy::BasicScalarAndOwnedRecursiveProducers
    }

    /// Whether this profile renders ItemAt over `sequence` in the paired context.
    fn item_at_sequence(self, sequence: &codegen::GeneratedSequence) -> bool {
        matches!(
            sequence,
            codegen::GeneratedSequence::Range { .. }
                | codegen::GeneratedSequence::Tokenize { .. }
                | codegen::GeneratedSequence::TokenizeByLength { .. }
                | codegen::GeneratedSequence::TokenizeRegex { .. }
        ) || self.permits_owned_recursive_collect()
            && matches!(
                sequence,
                codegen::GeneratedSequence::OwnedRecursiveCollect { .. }
            )
    }
}

pub(crate) fn render_basic_owned_mapping(program: &Program) -> Result<String, EmitError> {
    render_basic_owned_mapping_profile(
        program,
        false,
        false,
        false,
        false,
        false,
        TargetExtension::CLOSED,
    )
}

fn render_basic_owned_mapping_profile(
    program: &Program,
    public_owned_nodes: bool,
    allow_filters: bool,
    allow_sort: bool,
    allow_windows: bool,
    allow_row_failures: bool,
    extension: TargetExtension,
) -> Result<String, EmitError> {
    let allow_scoped_groups = extension.controls.has_scoped_groups();
    // Select the control/consumer pair before the older ItemAt-only companion.
    if extension.permits_xml_mixed_construction() {
        codegen::validate_program_with_basic_owned_xml_mixed_construction(program)?;
    } else if extension.permits_xml_mixed_content() {
        codegen::validate_program_with_basic_owned_xml_mixed_content(program)?;
    } else if extension.permits_xml_serialize() {
        codegen::validate_program_with_basic_owned_xml_serialize(program)?;
    } else if extension.dynamic_field {
        codegen::validate_program_with_basic_owned_dynamic_field(program)?;
    } else if extension.owned_search == OwnedSearch::CollectionAggregates {
        codegen::validate_program_with_basic_owned_collection_aggregates(program)?;
    } else if extension.owned_joins {
        codegen::validate_program_with_basic_owned_joins(program)?;
    } else if extension.owned_search == OwnedSearch::LookupFind {
        codegen::validate_program_with_basic_owned_lookup_find(program)?;
    } else if extension.recursive_filter {
        codegen::validate_program_with_basic_owned_recursive_filter(program)?;
    } else if extension.permits_sequence_aggregate() {
        codegen::validate_program_with_basic_owned_sequence_aggregate(program)?;
    } else if extension.permits_sequence_exists() {
        codegen::validate_program_with_basic_owned_sequence_exists(program)?;
    } else if extension.permits_owned_recursive_collect() {
        codegen::validate_program_with_basic_owned_recursive_collect(program)?;
    } else if extension.adjacency_tree {
        codegen::validate_program_with_basic_owned_adjacency_tree(program)?;
    } else if extension.permits_path_hierarchy() {
        codegen::validate_program_with_basic_owned_path_hierarchy(program)?;
    } else if extension.failure_sources {
        codegen::validate_program_with_basic_owned_failure_sources(program)?;
    } else if extension.permits_into_blocks() {
        codegen::validate_program_with_basic_owned_group_into_blocks(program)?;
    } else if extension.permits_ending_with() {
        codegen::validate_program_with_basic_owned_group_ending_with(program)?;
    } else if extension.permits_starting_with() {
        codegen::validate_program_with_basic_owned_group_starting_with(program)?;
    } else if extension.permits_adjacent_by() {
        codegen::validate_program_with_basic_owned_group_adjacent_by(program)?;
    } else if extension.permits_sequence_item_at() {
        codegen::validate_program_with_basic_owned_sequence_item_at(program)?;
    } else if allow_scoped_groups {
        codegen::validate_program_with_basic_owned_group_by(program)?;
    } else if allow_row_failures {
        codegen::validate_program_with_basic_owned_row_failures(program)?;
    } else if allow_windows {
        codegen::validate_program_with_basic_owned_windows(program)?;
    } else if allow_sort {
        codegen::validate_program_with_basic_owned_sort(program)?;
    } else if allow_filters {
        codegen::validate_program_with_basic_owned_filters(program)?;
    } else {
        codegen::validate_program(program)?;
    }
    let retained = retained_expressions(program);
    validate_private_inventory(
        program,
        &retained,
        allow_filters,
        allow_sort,
        allow_windows,
        allow_row_failures,
        extension,
    )?;
    let mut body = String::new();
    let (primary, extras) = if extension.permits_into_blocks() {
        mapping::render_basic_scopes_with_group_into_blocks(program, &mut body)?
    } else if extension.permits_ending_with() {
        mapping::render_basic_scopes_with_group_ending_with(program, &mut body)?
    } else if extension.permits_starting_with() {
        mapping::render_basic_scopes_with_group_starting_with(program, &mut body)?
    } else if extension.permits_adjacent_by() {
        mapping::render_basic_scopes_with_group_adjacent_by(program, &mut body)?
    } else if allow_scoped_groups {
        mapping::render_basic_scopes_with_group_by(program, &mut body)?
    } else if allow_windows {
        mapping::render_basic_scopes_with_windows(program, &mut body)?
    } else {
        mapping::render_basic_scopes(program, &mut body)?
    };
    let mut output = String::from(
        "namespace Ferrule.Generated;\n\npublic sealed record NamedOutput(string Name, global::Ferrule.Runtime.FerruleInstance Instance);\npublic sealed record ExecutionOutputs(global::Ferrule.Runtime.FerruleInstance Primary, global::System.Collections.Generic.IReadOnlyList<NamedOutput> Extras);\n\n",
    );
    output.push_str(mapping::basic_selected_output_types());
    output.push_str("public static partial class GeneratedMapping\n{\n    // No implementation in the prototype: C# erases calls outside the private test host.\n    static partial void ObserveOwnedBasicNode(uint node, global::Ferrule.Runtime.FerruleBasicScopeContext context);\n");
    if !program.failure_rules.is_empty() {
        output.push_str("    static partial void ObserveBasicFailureCohort(int rule, global::System.Collections.Generic.IReadOnlyList<string> path, global::Ferrule.Runtime.FerruleRetainedSourceCohort cohort);\n");
    }
    output.push_str("    // Private prototype: exact prepared declarations, no input normalization or policy defaults.\n    private static ExecutionOutputs ExecuteOwnedBasic(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.FerruleDeclaredNamedSourceRoot> declared,\n        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,\n        global::Ferrule.Runtime.FerruleExecutionContext? execution = null)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(source);\n        var roots = global::Ferrule.Runtime.FerruleBasicSourceRoots.Create(source, declared, limits);\n        var context = roots.RootContext(execution);\n");
    if !program.failure_rules.is_empty() {
        output.push_str("        EvaluateBasicFailureRules(context);\n");
    }
    output.push_str(&format!(
        "        var primary = Scope_{primary}(context);\n"
    ));
    for (index, (_, scope)) in extras.iter().enumerate() {
        output.push_str(&format!(
            "        var extra_{index} = Scope_{scope}(context);\n"
        ));
    }
    output.push_str(
        "        var result = new ExecutionOutputs(primary, new NamedOutput[]\n        {\n",
    );
    for (index, (name, _)) in extras.iter().enumerate() {
        output.push_str(&format!(
            "            new({}, extra_{index}),\n",
            literal::string(name)
        ));
    }
    output.push_str(
        "        });\n        global::System.GC.KeepAlive(roots);\n        return result;\n    }\n",
    );
    output.push_str("\n    private static SelectedTargetOutput ExecuteSelectedOwnedBasic(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::Ferrule.Runtime.FerruleTargetSelection selection,\n        global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.FerruleDeclaredNamedSourceRoot> declared,\n        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,\n        global::Ferrule.Runtime.FerruleExecutionContext? execution = null)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(source);\n        global::System.ArgumentNullException.ThrowIfNull(selection);\n        var selected = ResolveOwnedBasicTarget(selection);\n        var roots = global::Ferrule.Runtime.FerruleBasicSourceRoots.Create(source, declared, limits);\n        var context = roots.RootContext(execution);\n");
    if !program.failure_rules.is_empty() {
        output.push_str("        EvaluateBasicFailureRules(context);\n");
    }
    output.push_str("        SelectedTargetOutput result;\n        switch (selected)\n        {\n");
    output.push_str(&format!("            case 0: result = new SelectedTargetOutput.Primary(Scope_{primary}(context)); break;\n"));
    for (index, (name, scope)) in extras.iter().enumerate() {
        output.push_str(&format!("            case {}: result = new SelectedTargetOutput.Named(new NamedOutput({}, Scope_{scope}(context))); break;\n", index + 1, literal::string(name)));
    }
    output.push_str("            default: throw new global::System.InvalidOperationException(\"Resolved target is invalid.\");\n        }\n        global::System.GC.KeepAlive(roots);\n        return result;\n    }\n\n    private static int ResolveOwnedBasicTarget(global::Ferrule.Runtime.FerruleTargetSelection selection)\n    {\n        if (selection.IsPrimary) return 0;\n        var name = selection.Name!;\n");
    for (index, (name, _)) in extras.iter().enumerate() {
        output.push_str(&format!("        if (global::System.String.Equals(name, {}, global::System.StringComparison.Ordinal)) return {};\n", literal::string(name), index + 1));
    }
    output.push_str("        throw new global::Ferrule.Runtime.FerruleRuntimeException(\n            global::Ferrule.Runtime.FerruleRuntimeError.UnknownTarget,\n            $\"project has no named target `{name}`\",\n            detail: name);\n    }\n");
    render_basic_failures(
        program,
        &mut output,
        allow_row_failures,
        extension.failure_sources,
    )?;
    if program.expressions.iter().any(|node| {
        retained.contains(&node.id)
            && match &node.expression {
                Expression::OwnedSourceField(field) => {
                    field.frame.as_ref().is_some_and(Vec::is_empty)
                }
                Expression::OwnedDynamicSourceField(field) => {
                    field.frame.as_ref().is_some_and(Vec::is_empty)
                }
                Expression::OwnedXmlSerialize(serializer) => {
                    serializer.frame.as_ref().is_some_and(Vec::is_empty)
                }
                Expression::OwnedXmlMixedContent(mixed) => {
                    mixed.frame.as_ref().is_some_and(Vec::is_empty)
                }
                _ => false,
            }
    }) {
        render_empty_root_pin_declarations(program, &mut output);
    }
    let mut nodes = program.expressions.iter().collect::<Vec<_>>();
    nodes.sort_by_key(|node| node.id);
    for node in nodes {
        if retained.contains(&node.id) {
            if allow_windows && !expression_needs_owned_sources(program, node.id) {
                mapping::render_legacy_basic_expression(node.id, &node.expression, &mut output)?;
            }
            if public_owned_nodes {
                mapping::render_public_basic_expression(node.id, &node.expression, &mut output)?;
            } else {
                mapping::render_basic_expression(node.id, &node.expression, &mut output)?;
            }
        }
    }
    output.push_str(&body);
    output.push_str("}\n");
    Ok(output)
}

fn capability(
    location: Location,
    request: Option<Request>,
    reason: Reason,
    origin: Origin,
) -> EmitError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(Diagnostic::Capability {
            location,
            request,
            reason,
            origin,
        }),
    }
    .into()
}

fn expression_refusal(node: NodeId, expression: &Expression, reason: Reason) -> EmitError {
    let request = match expression {
        Expression::OwnedSourceField(field) => Some(Request::Field {
            owner: (&field.source.owner).into(),
            path: field.source.path.clone(),
            frame: field.frame.clone(),
        }),
        Expression::OwnedPosition(position) => Some(Request::Position {
            owner: (&position.owner).into(),
            collection: position.collection.clone(),
        }),
        Expression::OwnedRowDocumentPath(path) => Some(Request::RowDocumentPath {
            owner: (&path.owner).into(),
            collection: path.collection.clone(),
        }),
        Expression::OwnedXmlSerialize(serializer) => Some(Request::XmlElement {
            owner: (&serializer.source.owner).into(),
            path: serializer.source.path.clone(),
            frame: serializer.frame.clone(),
        }),
        Expression::OwnedXmlMixedContent(mixed) => Some(Request::XmlElement {
            owner: (&mixed.source.owner).into(),
            path: mixed.source.path.clone(),
            frame: mixed.frame.clone(),
        }),
        _ => None,
    };
    capability(
        Location::Expression {
            namespace: Namespace::Main,
            node,
            scope: None,
        },
        request,
        reason,
        Origin::RetainedExpression {
            namespace: Namespace::Main,
            node,
        },
    )
}

fn scope_error(scope: &TargetScope, route: SourceScopeLocation, reason: Reason) -> EmitError {
    let request = scope
        .iteration
        .as_ref()
        .and_then(|iteration| iteration.owned_source_iteration())
        .map(|source| Request::Iteration {
            owner: (&source.source().owner).into(),
            path: source.source().path.clone(),
        });
    capability(
        Location::Iteration {
            scope: route.clone(),
        },
        request,
        reason,
        Origin::Scope { scope: route },
    )
}

#[derive(Clone, Copy)]
struct ScopeControls {
    owned_joins: bool,
    filters: bool,
    sort: bool,
    windows: bool,
    grouping: TargetControls,
    path_hierarchy: bool,
    adjacency_tree: bool,
    recursive_filter: bool,
    /// Mixed-content construction by an ungrouped scope's own owned selector.
    mixed_construction: bool,
}

fn check_scope(
    program: &Program,
    scope: &TargetScope,
    route: SourceScopeLocation,
    controls: ScopeControls,
    inherited_owned: bool,
) -> Result<(), EmitError> {
    let paired = inherited_owned || scope_needs_owned_sources(program, scope);
    if let Some(iteration) = &scope.iteration {
        // Codegen admission validated the complete owned join subtree.
        if controls.owned_joins && iteration.owned_inner_join().is_some() {
            return Ok(());
        }
        let reason = match iteration.input() {
            IterationSource::OwnedSource(_) | IterationSource::Source(_) => None,
            IterationSource::Generated(_) => Some(Reason::GeneratedItem),
            IterationSource::InnerJoin(_) | IterationSource::OwnedInnerJoin(_) => {
                Some(Reason::Join)
            }
            IterationSource::Concatenate(_) => Some(Reason::Concatenate),
            IterationSource::DynamicDocuments(_) => Some(Reason::Document),
        };
        if let Some(reason) = reason {
            return Err(scope_error(scope, route, reason));
        }
        if (!controls.filters && iteration.filter().is_some())
            || (!controls.sort && iteration.sort().is_some())
            || (iteration.grouping().is_some()
                && !(controls.grouping.has_scoped_groups()
                    && (!paired || controls.grouping.permits_grouping(iteration.grouping()))))
            || (iteration.post_group_filter().is_some() && !controls.grouping.has_scoped_groups())
            || (!controls.windows && !iteration.windows().is_empty())
            || (iteration.output() != IterationOutput::Repeated
                && !(controls.windows && iteration.output() == IterationOutput::First))
        {
            return Err(scope_error(scope, route, Reason::ScopeControl));
        }
    }
    let admitted_construction = controls.path_hierarchy
        && matches!(
            scope.construction,
            TargetConstruction::OwnedPathHierarchy { .. }
        )
        || controls.adjacency_tree
            && matches!(
                scope.construction,
                TargetConstruction::OwnedAdjacencyTree { .. }
            )
        || controls.recursive_filter && owned_recursive_filter_predicate(scope).is_some()
        || controls.mixed_construction
            && matches!(
                scope.construction,
                TargetConstruction::XmlMixedContent { .. }
            )
            && scope.iteration.as_ref().is_some_and(|iteration| {
                iteration.owned_source_iteration().is_some()
                    && iteration.grouping().is_none()
                    && iteration.post_group_filter().is_none()
            });
    if !admitted_construction
        && !matches!(
            scope.construction,
            TargetConstruction::Group | TargetConstruction::Scalar { .. }
        )
    {
        let reason = match scope.construction {
            TargetConstruction::XmlMixedContent { .. } => Reason::XmlOperator,
            TargetConstruction::DynamicGroup { .. } => Reason::DynamicTarget,
            TargetConstruction::RecursiveFilter { .. }
            | TargetConstruction::PathHierarchy { .. }
            | TargetConstruction::OwnedPathHierarchy { .. }
            | TargetConstruction::AdjacencyTree { .. }
            | TargetConstruction::OwnedAdjacencyTree { .. } => Reason::TreeOperator,
            _ => Reason::SpecialConstruction,
        };
        return Err(scope_error(scope, route, reason));
    }
    if let Some(iteration) = &scope.iteration
        && let IterationSource::Source(source) = iteration.input()
        && dynamic_legacy_path(program, source.path())
    {
        return Err(scope_error(scope, route, Reason::DynamicLoader));
    }
    let inherited_owned = inherited_owned
        || scope.iteration.as_ref().is_some_and(|iteration| {
            matches!(iteration.input(), IterationSource::OwnedSource(_))
                || paired && controls.grouping.permits_grouping(iteration.grouping())
        });
    for (index, child) in scope.children.iter().enumerate() {
        check_scope(
            program,
            child,
            route.clone().descendant(SourceScopeStep::Child(index)),
            controls,
            inherited_owned,
        )?;
    }
    Ok(())
}

// A legacy frame replaces its path, including an explicitly empty frame.
// Metadata matching is exact and does not infer runtime source ownership.
fn dynamic_legacy_path(program: &Program, path: &[String]) -> bool {
    path.first().is_some_and(|name| {
        program
            .extra_sources
            .iter()
            .any(|source| source.name == *name && source.dynamic.is_some())
    })
}

// Rendered targets, ordinary failures and every stored user-function call
// retain their potential dependencies. Both If arms and parameter defaults
// remain demands; unused ordinary definitions and dead nodes are omitted.
// Stored ownership still receives unconditional structural/context validation;
// unused declarations never activate a dynamic source.
fn retained_expressions(program: &Program) -> BTreeSet<NodeId> {
    fn roots(scope: &TargetScope, pending: &mut Vec<NodeId>) {
        if let TargetConstruction::Scalar { expression, .. } = &scope.construction {
            pending.push(*expression);
        }
        // The owned tree root is a parent-context target expression.
        if let TargetConstruction::OwnedAdjacencyTree {
            root: Some(root), ..
        } = &scope.construction
        {
            pending.push(*root);
        }
        // An owned recursive filter's predicate reads each item of its owner.
        pending.extend(owned_recursive_filter_predicate(scope));
        pending.extend(scope.bindings.iter().map(|binding| binding.expression));
        if let Some(iteration) = &scope.iteration {
            pending.extend(iteration.filter());
            if let Some(grouping) = iteration.grouping() {
                pending.push(match grouping {
                    codegen::GroupingPlan::By { key }
                    | codegen::GroupingPlan::AdjacentBy { key } => key,
                    codegen::GroupingPlan::StartingWith { predicate }
                    | codegen::GroupingPlan::EndingWith { predicate } => predicate,
                    codegen::GroupingPlan::IntoBlocks { size } => size,
                });
            }
            pending.extend(iteration.post_group_filter());
            if let Some(sort) = iteration.sort() {
                pending.extend(sort.keys().map(|key| key.expression));
            }
            for window in iteration.windows() {
                match *window {
                    codegen::SequenceWindow::SkipFirst { count }
                    | codegen::SequenceWindow::First { count }
                    | codegen::SequenceWindow::Last { count } => pending.push(count),
                    codegen::SequenceWindow::From { position } => pending.push(position),
                    codegen::SequenceWindow::FromTo { first, last } => {
                        pending.push(first);
                        pending.push(last);
                    }
                }
            }
        }
        for child in &scope.children {
            roots(child, pending);
        }
    }
    let mut pending = Vec::new();
    roots(&program.root, &mut pending);
    for target in &program.extra_targets {
        roots(&target.root, &mut pending);
    }
    for rule in &program.failure_rules {
        pending.extend(rule.selection.predicate());
        pending.extend(rule.message);
        if let codegen::FailureIteration::Generated(sequence) = &rule.iteration {
            pending.extend(sequence.roots());
        }
    }
    // Keep the same stored-call policy as the ordinary Rust inventory.
    pending.extend(program.expressions.iter().filter_map(|node| {
        matches!(&node.expression, Expression::UserFunctionCall { .. }).then_some(node.id)
    }));
    let mut retained = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !retained.insert(id) {
            continue;
        }
        let Some(node) = program.expressions.iter().find(|node| node.id == id) else {
            continue;
        };
        pending.extend(retained_expression_inputs(&node.expression));
    }
    retained
}

fn retained_expression_inputs(expression: &Expression) -> Vec<NodeId> {
    match expression {
        Expression::DynamicSourceField { key, .. } => vec![*key],
        Expression::OwnedDynamicSourceField(field) => vec![field.key],
        Expression::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Expression::OwnedXmlMixedContent(mixed) => mixed
            .replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Expression::RuntimeParameterDefault { default, .. } => vec![*default],
        Expression::InvocationCounter {
            start,
            step,
            restart,
            ..
        } => [Some(*start), Some(*step), *restart]
            .into_iter()
            .flatten()
            .collect(),
        Expression::Call { args, .. } | Expression::UserFunctionCall { args, .. } => args.clone(),
        Expression::DelimitedTextField { input, .. } | Expression::ValueMap { input, .. } => {
            vec![*input]
        }
        Expression::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Expression::Raise { message } => message.iter().copied().collect(),
        Expression::Lookup { matches, .. } => vec![*matches],
        Expression::OwnedLookup(lookup) => vec![lookup.matches],
        Expression::OwnedCollectionFind(find) => vec![find.predicate, find.value],
        Expression::OwnedAggregate(aggregate) => aggregate.inputs(),
        Expression::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Expression::Aggregate { value, arg, .. } => {
            value.expression().into_iter().chain(*arg).collect()
        }
        Expression::JoinAggregate {
            expression, arg, ..
        } => expression.iter().copied().chain(*arg).collect(),
        Expression::SequenceExists {
            sequence,
            predicate,
        } => sequence.roots().chain([*predicate]).collect(),
        Expression::SequenceItemAt { sequence, index } => {
            sequence.roots().chain([*index]).collect()
        }
        Expression::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => sequence
            .roots()
            .chain(*predicate)
            .chain(*expression)
            .chain(*arg)
            .collect(),
        Expression::OwnedSourceField(_)
        | Expression::OwnedPosition(_)
        | Expression::OwnedRowDocumentPath(_)
        | Expression::OwnedXmlSerialize(_)
        | Expression::OwnedJoinField(_)
        | Expression::OwnedJoinSourcePosition(_)
        | Expression::SourceField { .. }
        | Expression::SourceRootXmlTypeEquals { .. }
        | Expression::SourceRootField { .. }
        | Expression::XmlSerialize { .. }
        | Expression::SourceDocumentPath
        | Expression::Position { .. }
        | Expression::JoinField { .. }
        | Expression::JoinPosition { .. }
        | Expression::Const { .. }
        | Expression::FunctionParameter { .. }
        | Expression::RuntimeValue { .. }
        | Expression::RuntimeParameter { .. } => Vec::new(),
    }
}

fn validate_private_inventory(
    program: &Program,
    retained: &BTreeSet<NodeId>,
    allow_filters: bool,
    allow_sort: bool,
    allow_windows: bool,
    allow_row_failures: bool,
    extension: TargetExtension,
) -> Result<(), EmitError> {
    // General schema/structure and existing validation precedence already ran.
    let controls = ScopeControls {
        owned_joins: extension.owned_joins,
        filters: allow_filters,
        sort: allow_sort,
        windows: allow_windows,
        grouping: extension.controls,
        path_hierarchy: extension.permits_path_hierarchy(),
        adjacency_tree: extension.adjacency_tree,
        recursive_filter: extension.recursive_filter,
        mixed_construction: extension.permits_xml_mixed_construction(),
    };
    check_scope(
        program,
        &program.root,
        SourceScopeLocation::root(SourceScopeTarget::Primary),
        controls,
        false,
    )?;
    for (index, target) in program.extra_targets.iter().enumerate() {
        check_scope(
            program,
            &target.root,
            SourceScopeLocation::root(SourceScopeTarget::Named {
                index,
                name: target.name.clone(),
            }),
            controls,
            false,
        )?;
    }
    if extension.permits_xml_mixed_construction() {
        codegen::validate_basic_owned_xml_mixed_construction_emission(program)?;
    } else if extension.permits_xml_mixed_content() {
        codegen::validate_basic_owned_xml_mixed_content_emission(program)?;
    } else if extension.permits_xml_serialize() {
        codegen::validate_basic_owned_xml_serialize_emission(program)?;
    } else if extension.dynamic_field {
        codegen::validate_basic_owned_dynamic_field_emission(program)?;
    } else if extension.owned_search == OwnedSearch::CollectionAggregates {
        codegen::validate_basic_owned_collection_aggregates_emission(program)?;
    } else if extension.owned_joins {
        codegen::validate_basic_owned_joins_emission(program)?;
    } else if extension.owned_search == OwnedSearch::LookupFind {
        codegen::validate_basic_owned_lookup_find_emission(program)?;
    } else if extension.recursive_filter {
        codegen::validate_basic_owned_recursive_filter_emission(program)?;
    } else if extension.permits_sequence_aggregate() {
        codegen::validate_basic_owned_sequence_aggregate_emission(program)?;
    } else if extension.permits_sequence_exists() {
        codegen::validate_basic_owned_sequence_exists_emission(program)?;
    } else if extension.permits_owned_recursive_collect() {
        codegen::validate_basic_owned_recursive_collect_emission(program)?;
    } else if extension.adjacency_tree {
        codegen::validate_basic_owned_adjacency_tree_emission(program)?;
    } else if extension.permits_path_hierarchy() {
        codegen::validate_basic_owned_path_hierarchy_emission(program)?;
    } else if extension.failure_sources {
        codegen::validate_basic_owned_failure_source_emission(program)?;
    } else if extension.permits_into_blocks() {
        codegen::validate_basic_owned_group_into_blocks_emission(program)?;
    } else if extension.permits_ending_with() {
        codegen::validate_basic_owned_group_ending_with_emission(program)?;
    } else if extension.permits_starting_with() {
        codegen::validate_basic_owned_group_starting_with_emission(program)?;
    } else if extension.permits_adjacent_by() {
        codegen::validate_basic_owned_group_adjacent_by_emission(program)?;
    } else if extension.permits_sequence_item_at() {
        codegen::validate_basic_owned_sequence_item_at_emission(program)?;
    } else if extension.controls.has_scoped_groups() {
        codegen::validate_basic_owned_group_by_emission(program)?;
    } else if allow_row_failures {
        codegen::validate_basic_owned_row_failure_emission(program)?;
    } else {
        validate_basic_failures(program)?;
    }
    for node in &program.expressions {
        if !retained.contains(&node.id) {
            continue;
        }
        if let Expression::UserFunctionCall { function, .. } = &node.expression
            && let Some(body) = program
                .user_functions
                .iter()
                .find(|body| body.id == *function)
        {
            return Err(capability(
                Location::Expression {
                    namespace: Namespace::Function(*function),
                    node: body.output,
                    scope: None,
                },
                None,
                Reason::IsolatedFunction,
                Origin::FunctionBody {
                    function: *function,
                },
            ));
        }
        if let Expression::SourceField { path, frame } = &node.expression
            && dynamic_legacy_path(program, frame.as_ref().unwrap_or(path))
        {
            return Err(codegen::owned_expression_emission_error(
                Namespace::Main,
                node.id,
                &node.expression,
            )
            .into());
        }
        let reason = match &node.expression {
            Expression::SequenceItemAt { sequence, .. }
                if extension.permits_sequence_item_at() && extension.item_at_sequence(sequence) =>
            {
                continue;
            }
            Expression::OwnedLookup(_) | Expression::OwnedCollectionFind(_)
                if extension.owned_search != OwnedSearch::Closed =>
            {
                continue;
            }
            Expression::OwnedAggregate(_)
                if extension.owned_search == OwnedSearch::CollectionAggregates =>
            {
                continue;
            }
            // Codegen bound each read to its dominating owned join tuple.
            Expression::OwnedJoinField(_)
            | Expression::OwnedJoinSourcePosition(_)
            | Expression::JoinPosition { .. }
                if extension.owned_joins =>
            {
                continue;
            }
            Expression::SequenceExists { sequence, .. }
                if extension.permits_sequence_exists() && extension.item_at_sequence(sequence) =>
            {
                continue;
            }
            Expression::OwnedDynamicSourceField(_) if extension.dynamic_field => continue,
            Expression::SequenceAggregate { sequence, .. }
                if extension.permits_sequence_aggregate()
                    && extension.item_at_sequence(sequence) =>
            {
                continue;
            }
            Expression::OwnedXmlSerialize(_) if extension.permits_xml_serialize() => continue,
            Expression::OwnedXmlMixedContent(_) if extension.permits_xml_mixed_content() => {
                continue;
            }
            Expression::OwnedXmlSerialize(_) | Expression::OwnedXmlMixedContent(_) => {
                Reason::XmlOperator
            }
            Expression::OwnedSourceField(_)
            | Expression::OwnedPosition(_)
            | Expression::OwnedRowDocumentPath(_)
            | Expression::SourceField { .. }
            | Expression::Position { .. }
            | Expression::Const { .. }
            | Expression::Call { .. }
            | Expression::If { .. }
            | Expression::RuntimeValue { .. }
            | Expression::RuntimeParameter { .. }
            | Expression::RuntimeParameterDefault { .. }
            | Expression::ValueMap { .. }
            | Expression::Raise { .. } => continue,
            Expression::SourceRootXmlTypeEquals { .. }
            | Expression::XmlSerialize { .. }
            | Expression::XmlMixedContent { .. } => Reason::XmlOperator,
            Expression::SourceDocumentPath => Reason::Document,
            Expression::DynamicSourceField { .. } => Reason::DynamicLoader,
            Expression::JoinField { .. }
            | Expression::JoinPosition { .. }
            | Expression::OwnedJoinField(_)
            | Expression::OwnedJoinSourcePosition(_) => Reason::Join,
            Expression::Lookup { .. }
            | Expression::CollectionFind { .. }
            | Expression::OwnedLookup(_)
            | Expression::OwnedCollectionFind(_)
            | Expression::OwnedAggregate(_)
            | Expression::Aggregate { .. }
            | Expression::JoinAggregate { .. } => Reason::AggregateLookup,
            Expression::SequenceExists { .. }
            | Expression::SequenceItemAt { .. }
            | Expression::SequenceAggregate { .. } => Reason::GeneratedItem,
            Expression::UserFunctionCall { .. } | Expression::FunctionParameter { .. } => {
                Reason::IsolatedFunction
            }
            _ => Reason::UnsupportedBasicContext,
        };
        return Err(expression_refusal(node.id, &node.expression, reason));
    }
    Ok(())
}

pub(super) fn scope_refusal(program: &Program, scope_index: usize) -> EmitError {
    fn visit<'a>(
        scope: &'a TargetScope,
        route: SourceScopeLocation,
        all: &mut Vec<(&'a TargetScope, SourceScopeLocation)>,
    ) {
        all.push((scope, route.clone()));
        for (index, child) in scope.children.iter().enumerate() {
            visit(
                child,
                route.clone().descendant(SourceScopeStep::Child(index)),
                all,
            );
        }
    }
    let mut all = Vec::new();
    visit(
        &program.root,
        SourceScopeLocation::root(SourceScopeTarget::Primary),
        &mut all,
    );
    for (index, target) in program.extra_targets.iter().enumerate() {
        visit(
            &target.root,
            SourceScopeLocation::root(SourceScopeTarget::Named {
                index,
                name: target.name.clone(),
            }),
            &mut all,
        );
    }
    let (scope, route) = &all[scope_index];
    scope_error(scope, route.clone(), Reason::UnsupportedBasicContext)
}

pub(super) fn render_owner(owner: &SourceOwner, output: &mut String) {
    match owner {
        SourceOwner::Primary => {
            output.push_str("global::Ferrule.Runtime.FerruleOwnedSourceOwner.Primary")
        }
        SourceOwner::Named { name } => output.push_str(&format!(
            "global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named({})",
            literal::string(name)
        )),
    }
}

// This is immutable program metadata, not an occurrence or a run-wide policy.
// It is emitted only when a retained owned field has an explicit empty pin.
fn render_empty_root_pin_declarations(program: &Program, output: &mut String) {
    output.push_str("\n    private static bool OwnedRootIsDeclaredNonrepeating(global::Ferrule.Runtime.FerruleOwnedSourceOwner owner)\n    {\n");
    output.push_str(&format!(
        "        if (owner.IsPrimary) return {};\n",
        !program.source.repeating
    ));
    for source in &program.extra_sources {
        output.push_str(&format!(
            "        if (global::System.String.Equals(owner.Name, {}, global::System.StringComparison.Ordinal)) return {};\n",
            literal::string(&source.name),
            !source.source.repeating
        ));
    }
    output.push_str("        return false;\n    }\n");
}

pub(super) fn render_owned_leaf(
    node: NodeId,
    expression: &Expression,
    output: &mut String,
) -> Result<(), EmitError> {
    match expression {
        Expression::OwnedSourceField(field) => {
            let empty_root_pin = field.frame.as_ref().is_some_and(Vec::is_empty);
            output.push_str(if empty_root_pin {
                " =>\n        context.ResolveOwnedFieldWithRootPin("
            } else {
                " =>\n        context.ResolveOwnedField("
            });
            render_owner(&field.source.owner, output);
            output.push_str(", ");
            mapping::basic_path(&field.source.path, output);
            output.push_str(", ");
            mapping::render_optional_path(field.frame.as_deref(), output);
            if empty_root_pin {
                output.push_str(", OwnedRootIsDeclaredNonrepeating(");
                render_owner(&field.source.owner, output);
                output.push(')');
            }
            output.push_str(");\n");
        }
        Expression::OwnedPosition(position) => {
            // This private entry accepts no contexts or phase indices. All owned
            // frames come from int-sized actual walks; no arbitrary index can enter.
            output.push_str(" =>\n        global::Ferrule.Runtime.FerruleValue.FromInt64(checked((long)context.OwnedPosition(");
            render_owner(&position.owner, output);
            output.push_str(", ");
            mapping::render_optional_path(position.collection.as_deref(), output);
            output.push_str(")));\n");
        }
        Expression::OwnedRowDocumentPath(path) => {
            output.push_str(" =>\n        context.OwnedRowDocumentPath(");
            render_owner(&path.owner, output);
            output.push_str(", ");
            mapping::render_optional_path(path.collection.as_deref(), output);
            output.push_str(");\n");
        }
        // The receiving-context key is evaluated before the owned read.
        Expression::OwnedDynamicSourceField(field) => {
            output.push_str(&format!(
                "\n    {{\n        var key = Node_{}(context);\n        return context.ResolveOwnedDynamicField(",
                field.key
            ));
            render_owner(&field.object.owner, output);
            output.push_str(", ");
            mapping::basic_path(&field.object.path, output);
            output.push_str(", ");
            mapping::render_optional_path(field.frame.as_deref(), output);
            output.push_str(", key, ");
            if field.frame.as_ref().is_some_and(Vec::is_empty) {
                output.push_str("OwnedRootIsDeclaredNonrepeating(");
                render_owner(&field.object.owner, output);
                output.push(')');
            } else {
                output.push_str("false");
            }
            output.push_str(");\n    }\n");
        }
        Expression::OwnedXmlSerialize(serializer) => {
            // The element read precedes schema unwrapping and writing, like the
            // legacy serializer; only the read's owned failures gain the node.
            let schema = codegen::serialize_embedded_schema(
                &serializer.schema,
                codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
            )?;
            output.push_str(&format!(
                " =>\n        global::Ferrule.Runtime.FerruleXml.SerializeEmbedded(\n            {node}U,\n            {},\n            context.ResolveOwnedXmlInstance(",
                literal::string(&schema)
            ));
            render_owner(&serializer.source.owner, output);
            output.push_str(", ");
            mapping::basic_path(&serializer.source.path, output);
            output.push_str(", ");
            mapping::render_optional_path(serializer.frame.as_deref(), output);
            if serializer.frame.as_ref().is_some_and(Vec::is_empty) {
                output.push_str(", OwnedRootIsDeclaredNonrepeating(");
                render_owner(&serializer.source.owner, output);
                output.push(')');
            } else {
                output.push_str(", false");
            }
            output.push_str(&format!(
                "),\n            {},\n            {},\n            {});\n",
                serializer.declaration,
                serializer.indent,
                serializer
                    .namespace
                    .as_deref()
                    .map_or_else(|| "null".to_string(), literal::string),
            ));
        }
        Expression::OwnedJoinField(field) => {
            output.push_str(&format!(
                " =>\n        context.OwnedJoinField({}UL, ",
                field.join.get()
            ));
            render_owner(&field.collection.owner, output);
            output.push_str(", ");
            mapping::basic_path(&field.collection.path, output);
            output.push_str(", ");
            mapping::basic_path(&field.path, output);
            output.push_str(");\n");
        }
        Expression::OwnedJoinSourcePosition(position) => {
            output.push_str(&format!(
                " =>\n        global::Ferrule.Runtime.FerruleValue.FromInt64(checked((long)context.OwnedJoinSourcePosition({}UL, ",
                position.join.get()
            ));
            render_owner(&position.collection.owner, output);
            output.push_str(", ");
            mapping::basic_path(&position.collection.path, output);
            output.push_str(")));\n");
        }
        _ => unreachable!("shared expression mode dispatches only owned leaves"),
    }
    Ok(())
}

// Failure traversal remains the ordinary legacy walk inside a paired view. It
// acquires one complete bounded source cohort before selecting its first item;
// neither a second source walk nor an inferred owner is introduced.
pub(crate) fn failure_refusal(index: usize, reason: Reason) -> EmitError {
    capability(
        Location::Iteration {
            scope: SourceScopeLocation::root(SourceScopeTarget::Primary),
        },
        None,
        reason,
        Origin::Failure { index },
    )
}

fn failure_expression_nodes(program: &Program, root: NodeId) -> BTreeSet<NodeId> {
    let mut pending = vec![root];
    let mut found = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !found.insert(id) {
            continue;
        }
        let Some(node) = program.expressions.iter().find(|node| node.id == id) else {
            continue;
        };
        match &node.expression {
            Expression::Call { args, .. } => pending.extend(args.iter().copied()),
            Expression::If {
                condition,
                then,
                else_,
            } => pending.extend([*condition, *then, *else_]),
            Expression::RuntimeParameterDefault { default, .. } => pending.push(*default),
            Expression::Raise { message } => pending.extend(message.iter().copied()),
            _ => {}
        }
    }
    found
}

fn validate_basic_failures(program: &Program) -> Result<(), EmitError> {
    for (index, rule) in program.failure_rules.iter().enumerate() {
        let codegen::FailureIteration::Source(source) = &rule.iteration else {
            return Err(failure_refusal(index, Reason::FailureRule));
        };
        // The bridge's new admission order is iteration, predicate, then optional
        // message, per ordered rule. Common structural/owned-context validation
        // and the unchanged target/UDF preflight have already run. Both potential
        // If branches are inspected here; ordinary evaluation stays lazy.
        if dynamic_legacy_path(program, source.path()) {
            return Err(failure_refusal(index, Reason::DynamicLoader));
        }
        for root in rule.selection.predicate().into_iter().chain(rule.message) {
            let retained = failure_expression_nodes(program, root);
            for node in &program.expressions {
                if !retained.contains(&node.id) {
                    continue;
                }
                let dynamic = match &node.expression {
                    Expression::SourceField { path, frame } => {
                        dynamic_legacy_path(program, frame.as_ref().unwrap_or(path))
                    }
                    Expression::DynamicSourceField { .. } => true,
                    _ => false,
                };
                if dynamic {
                    return Err(capability(
                        Location::Expression {
                            namespace: Namespace::Main,
                            node: node.id,
                            scope: None,
                        },
                        None,
                        Reason::DynamicLoader,
                        Origin::Failure { index },
                    ));
                }
                if !matches!(
                    &node.expression,
                    Expression::SourceField { .. }
                        | Expression::Position { .. }
                        | Expression::Const { .. }
                        | Expression::Call { .. }
                        | Expression::If { .. }
                        | Expression::RuntimeValue { .. }
                        | Expression::RuntimeParameter { .. }
                        | Expression::RuntimeParameterDefault { .. }
                        | Expression::Raise { .. }
                        | Expression::ValueMap { .. }
                ) {
                    // Owned requests are refused by common validation first.
                    // Every other specialized failure domain stays closed with
                    // the pre-existing FailureRule envelope, not a new runtime.
                    return Err(failure_refusal(index, Reason::FailureRule));
                }
            }
        }
    }
    Ok(())
}

fn render_basic_failures(
    program: &Program,
    output: &mut String,
    allow_row_failures: bool,
    allow_failure_sources: bool,
) -> Result<(), EmitError> {
    use codegen::{FailureIteration, FailureSelection};
    if program.failure_rules.is_empty() {
        return Ok(());
    }
    output.push_str("\n    private static void EvaluateBasicFailureRules(global::Ferrule.Runtime.FerruleBasicScopeContext context)\n    {\n");
    for index in 0..program.failure_rules.len() {
        output.push_str(&format!("        BasicFailureRule_{index}(context);\n"));
    }
    output.push_str("    }\n");
    for (index, rule) in program.failure_rules.iter().enumerate() {
        if let (true, FailureIteration::OwnedSource(source)) =
            (allow_failure_sources, &rule.iteration)
        {
            row_failures::render_owned_source_rule(index, rule, source.source(), output);
            continue;
        }
        if allow_row_failures && row_failures::needs_owned_rows(program, rule) {
            row_failures::render_rule(index, rule, output)?;
            continue;
        }
        let FailureIteration::Source(source) = &rule.iteration else {
            return Err(failure_refusal(index, Reason::FailureRule));
        };
        let number = index + 1;
        output.push_str(&format!("\n    private static void BasicFailureRule_{index}(global::Ferrule.Runtime.FerruleBasicScopeContext context)\n    {{\n        var path_failure_{index} = "));
        mapping::basic_path(source.path(), output);
        output.push_str(&format!(";\n        var cohort_failure_{index} = context.WalkSourceCohort(path_failure_{index});\n        ObserveBasicFailureCohort({number}, path_failure_{index}, cohort_failure_{index});\n        try\n        {{\n            foreach (var candidate_failure_{index} in cohort_failure_{index})\n            {{\n                var item_context_failure_{index} = candidate_failure_{index}.Context;\n"));
        let indent = match rule.selection {
            FailureSelection::All => 4,
            FailureSelection::WhenTrue(predicate) | FailureSelection::WhenFalse(predicate) => {
                output.push_str(&format!("                var selection_failure_{index} = Node_{predicate}(item_context_failure_{index});\n                if ("));
                if matches!(rule.selection, FailureSelection::WhenFalse(_)) {
                    output.push('!');
                }
                output.push_str(&format!("global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(selection_failure_{index}, {predicate}U))\n                {{\n"));
                5
            }
        };
        let pad = "    ".repeat(indent);
        if let Some(message) = rule.message {
            output.push_str(&format!("{pad}var message_failure_{index} = Node_{message}(item_context_failure_{index});\n{pad}throw global::Ferrule.Runtime.FerruleFailures.MappingFailure({number}, message_failure_{index});\n"));
        } else {
            output.push_str(&format!("{pad}throw global::Ferrule.Runtime.FerruleFailures.MappingFailure({number}, null);\n"));
        }
        if !matches!(rule.selection, FailureSelection::All) {
            output.push_str("                }\n");
        }
        output.push_str(&format!("            }}\n        }}\n        finally {{ global::System.GC.KeepAlive(cohort_failure_{index}); }}\n    }}\n"));
    }
    Ok(())
}

// Separate checked prototype; the prepared mapping above remains byte-exact.
#[cfg(test)]
pub(crate) fn render_checked_basic_owned_mapping(program: &Program) -> Result<String, EmitError> {
    render_checked_basic_owned_mapping_profile(
        program,
        false,
        false,
        false,
        false,
        false,
        TargetExtension::CLOSED,
    )
}

fn render_checked_basic_owned_mapping_profile(
    program: &Program,
    public_owned_nodes: bool,
    allow_filters: bool,
    allow_sort: bool,
    allow_windows: bool,
    allow_row_failures: bool,
    extension: TargetExtension,
) -> Result<String, EmitError> {
    let mut source = if public_owned_nodes {
        render_basic_owned_mapping_profile(
            program,
            true,
            allow_filters,
            allow_sort,
            allow_windows,
            allow_row_failures,
            extension,
        )?
    } else {
        render_basic_owned_mapping(program)?
    };
    let plan = if extension.permits_xml_mixed_construction() {
        codegen::legacy_named_input_admission_plan_with_basic_owned_xml_mixed_construction(program)?
    } else if extension.permits_xml_mixed_content() {
        codegen::legacy_named_input_admission_plan_with_basic_owned_xml_mixed_content(program)?
    } else if extension.permits_xml_serialize() {
        codegen::legacy_named_input_admission_plan_with_basic_owned_xml_serialize(program)?
    } else if extension.dynamic_field {
        codegen::legacy_named_input_admission_plan_with_basic_owned_dynamic_field(program)?
    } else if extension.owned_search == OwnedSearch::CollectionAggregates {
        codegen::legacy_named_input_admission_plan_with_basic_owned_collection_aggregates(program)?
    } else if extension.owned_joins {
        codegen::legacy_named_input_admission_plan_with_basic_owned_joins(program)?
    } else if extension.owned_search == OwnedSearch::LookupFind {
        codegen::legacy_named_input_admission_plan_with_basic_owned_lookup_find(program)?
    } else if extension.recursive_filter {
        codegen::legacy_named_input_admission_plan_with_basic_owned_recursive_filter(program)?
    } else if extension.permits_sequence_aggregate() {
        codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_aggregate(program)?
    } else if extension.permits_sequence_exists() {
        codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_exists(program)?
    } else if extension.permits_owned_recursive_collect() {
        codegen::legacy_named_input_admission_plan_with_basic_owned_recursive_collect(program)?
    } else if extension.adjacency_tree {
        codegen::legacy_named_input_admission_plan_with_basic_owned_adjacency_tree(program)?
    } else if extension.permits_path_hierarchy() {
        codegen::legacy_named_input_admission_plan_with_basic_owned_path_hierarchy(program)?
    } else if extension.failure_sources {
        codegen::legacy_named_input_admission_plan_with_basic_owned_failure_sources(program)?
    } else if extension.permits_into_blocks() {
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_into_blocks(program)?
    } else if extension.permits_ending_with() {
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_ending_with(program)?
    } else if extension.permits_starting_with() {
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_starting_with(program)?
    } else if extension.permits_adjacent_by() {
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(program)?
    } else if extension.permits_sequence_item_at() {
        codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(program)?
    } else if extension.controls.has_scoped_groups() {
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_by(program)?
    } else if allow_row_failures {
        codegen::legacy_named_input_admission_plan_with_basic_owned_row_failures(program)?
    } else if allow_windows {
        codegen::legacy_named_input_admission_plan_with_basic_owned_windows(program)?
    } else if allow_sort {
        codegen::legacy_named_input_admission_plan_with_basic_owned_sort(program)?
    } else if allow_filters {
        codegen::legacy_named_input_admission_plan_with_basic_owned_filters(program)?
    } else {
        codegen::legacy_named_input_admission_plan(program)?
    };
    let (primary, extras) = mapping::render_basic_scopes(program, &mut String::new())?;
    // Reuse the ordinary NamedInput spelling and exact record declaration.
    source.push_str("\npublic sealed record NamedInput(\n    string Name,\n    global::Ferrule.Runtime.FerruleInstance Instance);\n\npublic static partial class GeneratedMapping\n{\n");
    render_checked_catalogue("CheckedAllInputs", &plan.all_outputs, &mut source);
    render_checked_catalogue("CheckedPrimaryInputs", &plan.primary, &mut source);
    for target in &plan.named_targets {
        render_checked_catalogue(
            &format!("CheckedNamedInputs_{}", target.target_index),
            &target.catalogue,
            &mut source,
        );
    }
    source.push_str(CHECKED_INPUT_ACCESSOR);
    source.push_str(
        r#"
    private static ExecutionOutputs ExecuteCheckedOwnedBasic(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> supplied,
        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,
        global::Ferrule.Runtime.FerruleExecutionContext? execution = null)
    {
        global::System.ArgumentNullException.ThrowIfNull(source);
        var accessor = new CheckedNamedInputAccessor(supplied);
        ObserveCheckedInputBoundary(true);
        var roots = global::Ferrule.Runtime.FerruleBasicSourceRoots.CreateFromNamedInputs(
            source, CheckedAllInputs, accessor.Supplied, accessor, limits);
        var context = roots.RootContext(execution);
        var result = (ExecutionOutputs)ExecuteCheckedBasicContext(context, null);
        global::System.GC.KeepAlive(roots);
        return result;
    }

    private static SelectedTargetOutput ExecuteSelectedCheckedOwnedBasic(
        global::Ferrule.Runtime.FerruleInstance source,
        global::Ferrule.Runtime.FerruleTargetSelection selection,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> supplied,
        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,
        global::Ferrule.Runtime.FerruleExecutionContext? execution = null)
    {
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(selection);
        var selected = ResolveOwnedBasicTarget(selection);
        var catalogue = selected switch
        {
            0 => CheckedPrimaryInputs,
"#,
    );
    for (index, _) in extras.iter().enumerate() {
        source.push_str(&format!(
            "            {} => CheckedNamedInputs_{index},\n",
            index + 1
        ));
    }
    source.push_str(r#"            _ => throw new global::System.InvalidOperationException("Resolved checked target is invalid."),
        };
        var accessor = new CheckedNamedInputAccessor(supplied);
        ObserveCheckedInputBoundary(true);
        var roots = global::Ferrule.Runtime.FerruleBasicSourceRoots.CreateFromNamedInputs(
            source, catalogue, accessor.Supplied, accessor, limits);
        var context = roots.RootContext(execution);
        var result = (SelectedTargetOutput)ExecuteCheckedBasicContext(context, selected);
        global::System.GC.KeepAlive(roots);
        return result;
    }

    // One checked registry context is reused for ordinary global failures and
    // selected/all output. Specialized failure domains remain typed-closed.
    // Both return shapes are existing reference types; no wrapper is allocated.
    private static object ExecuteCheckedBasicContext(
        global::Ferrule.Runtime.FerruleBasicScopeContext context, int? selected)
    {
"#);
    if !program.failure_rules.is_empty() {
        source.push_str("        EvaluateBasicFailureRules(context);\n");
    }
    source.push_str(
        r#"        if (selected.HasValue)
        {
            switch (selected.Value)
            {
"#,
    );
    source.push_str(&format!("                case 0: return new SelectedTargetOutput.Primary(Scope_{primary}(context));\n"));
    for (index, (name, scope)) in extras.iter().enumerate() {
        source.push_str(&format!("                case {}: return new SelectedTargetOutput.Named(new NamedOutput({}, Scope_{scope}(context)));\n",index+1,literal::string(name)));
    }
    source.push_str("                default: throw new global::System.InvalidOperationException(\"Resolved checked target is invalid.\");\n            }\n        }\n");
    source.push_str(&format!(
        "        var primary = Scope_{primary}(context);\n"
    ));
    for (index, (_, scope)) in extras.iter().enumerate() {
        source.push_str(&format!(
            "        var extra_{index} = Scope_{scope}(context);\n"
        ));
    }
    source.push_str("        return new ExecutionOutputs(primary, new NamedOutput[]\n        {\n");
    for (index, (name, _)) in extras.iter().enumerate() {
        source.push_str(&format!(
            "            new({}, extra_{index}),\n",
            literal::string(name)
        ));
    }
    source.push_str("        });\n    }\n}\n");
    Ok(source)
}

fn render_checked_catalogue(
    name: &str,
    catalogue: &codegen::NamedInputCatalogue<'_>,
    source: &mut String,
) {
    source.push_str(&format!("    private static readonly global::Ferrule.Runtime.FerruleBasicNamedSourceDeclaration[] {name} = new global::Ferrule.Runtime.FerruleBasicNamedSourceDeclaration[]\n    {{\n"));
    for entry in &catalogue.entries {
        let mode = match entry.availability {
            codegen::NamedInputAvailability::Optional => "Optional",
            codegen::NamedInputAvailability::RequiredLegacy => "RequiredLegacy",
            codegen::NamedInputAvailability::ReservedUnavailable => "ReservedUnavailable",
        };
        source.push_str(&format!("        new({}, global::Ferrule.Runtime.FerruleBasicNamedSourceMode.{mode}), // declaration {}\n",literal::string(entry.name),entry.declaration_index));
    }
    source.push_str("    };\n\n");
}

const CHECKED_INPUT_ACCESSOR: &str = r#"
    // Calls are erased unless the private host supplies this partial observer.
    static partial void ObserveCheckedInputBoundary(bool factory);
    private readonly struct CheckedNamedInputAccessor : global::Ferrule.Runtime.IFerruleNamedSourceInputAccessor<NamedInput>
    {
        private readonly global::System.Collections.Generic.IReadOnlyList<NamedInput> _supplied;
        internal CheckedNamedInputAccessor(global::System.Collections.Generic.IReadOnlyList<NamedInput> supplied) { _supplied = supplied; }
        internal global::System.Collections.Generic.IReadOnlyList<NamedInput> Supplied => _supplied;
        public global::Ferrule.Runtime.FerruleNamedSourceInput Read(NamedInput item)
        {
            ObserveCheckedInputBoundary(false);
            return item is null ? default : new(item.Name, item.Instance);
        }
    }
"#;

pub(crate) fn render_public_basic_owned_mapping(program: &Program) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        false,
        false,
        false,
        false,
        TargetExtension::CLOSED,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_filter_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        false,
        false,
        false,
        TargetExtension::CLOSED,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_sort_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        false,
        false,
        TargetExtension::CLOSED,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

const PUBLIC_BASIC_CHECKED_ENTRIES: &str = r#"
public static partial class GeneratedMapping
{
    public static ExecutionOutputs ExecuteWithBasicOwnedSources(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> supplied,
        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,
        global::Ferrule.Runtime.FerruleExecutionContext? execution)
        => ExecuteCheckedOwnedBasic(source, supplied, limits, execution);

    public static SelectedTargetOutput ExecuteSelectedWithBasicOwnedSources(
        global::Ferrule.Runtime.FerruleInstance source,
        global::Ferrule.Runtime.FerruleTargetSelection selection,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> supplied,
        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,
        global::Ferrule.Runtime.FerruleExecutionContext? execution)
        => ExecuteSelectedCheckedOwnedBasic(source, selection, supplied, limits, execution);
}
"#;

// Owned demand is per scope, including potential branches and descendants. An
// unrelated legacy subtree keeps the existing legacy renderer and context rules.
fn expression_needs_owned_sources(program: &Program, root: NodeId) -> bool {
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(node) = program.expressions.iter().find(|node| node.id == id) {
            if match &node.expression {
                Expression::OwnedSourceField(_)
                | Expression::OwnedPosition(_)
                | Expression::OwnedRowDocumentPath(_)
                | Expression::OwnedLookup(_)
                | Expression::OwnedCollectionFind(_)
                | Expression::OwnedDynamicSourceField(_)
                | Expression::OwnedXmlSerialize(_)
                | Expression::OwnedAggregate(_)
                | Expression::OwnedJoinField(_)
                | Expression::OwnedJoinSourcePosition(_)
                | Expression::JoinPosition { .. }
                | Expression::OwnedXmlMixedContent(_) => true,
                // An owned producer reads its root through the paired context.
                Expression::SequenceItemAt { sequence, .. } => sequence.owned_source().is_some(),
                _ => false,
            } {
                return true;
            }
            pending.extend(retained_expression_inputs(&node.expression));
        }
    }
    false
}

pub(super) fn scope_needs_owned_sources(program: &Program, scope: &TargetScope) -> bool {
    if matches!(
        scope.construction,
        TargetConstruction::OwnedPathHierarchy { .. }
            | TargetConstruction::OwnedAdjacencyTree { .. }
    ) || scope.iteration.as_ref().is_some_and(|iteration| {
        matches!(
            iteration.input(),
            IterationSource::OwnedSource(_) | IterationSource::OwnedInnerJoin(_)
        )
    }) {
        return true;
    }
    let mut roots = scope
        .bindings
        .iter()
        .map(|binding| binding.expression)
        .collect::<Vec<_>>();
    if let TargetConstruction::Scalar { expression, .. } = scope.construction {
        roots.push(expression);
    }
    if let Some(iteration) = &scope.iteration {
        roots.extend(iteration.filter());
        if let Some(grouping) = iteration.grouping() {
            roots.push(match grouping {
                codegen::GroupingPlan::By { key } | codegen::GroupingPlan::AdjacentBy { key } => {
                    key
                }
                codegen::GroupingPlan::StartingWith { predicate }
                | codegen::GroupingPlan::EndingWith { predicate } => predicate,
                codegen::GroupingPlan::IntoBlocks { size } => size,
            });
        }
        roots.extend(iteration.post_group_filter());
        if let Some(sort) = iteration.sort() {
            roots.extend(sort.keys().map(|key| key.expression));
        }
        for window in iteration.windows() {
            match *window {
                codegen::SequenceWindow::SkipFirst { count }
                | codegen::SequenceWindow::First { count }
                | codegen::SequenceWindow::Last { count } => roots.push(count),
                codegen::SequenceWindow::From { position } => roots.push(position),
                codegen::SequenceWindow::FromTo { first, last } => {
                    roots.push(first);
                    roots.push(last);
                }
            }
        }
    }
    roots
        .into_iter()
        .any(|root| expression_needs_owned_sources(program, root))
        || scope
            .children
            .iter()
            .any(|child| scope_needs_owned_sources(program, child))
}

pub(crate) fn render_public_basic_owned_window_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        false,
        TargetExtension::CLOSED,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_row_failure_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::CLOSED,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_group_by_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::GROUP_BY,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_sequence_item_at_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::SEQUENCE_ITEM_AT,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_group_adjacent_by_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::GROUP_ADJACENT_BY,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_group_starting_with_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::GROUP_STARTING_WITH,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_group_ending_with_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::GROUP_ENDING_WITH,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_failure_source_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::FAILURE_SOURCES,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_path_hierarchy_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::PATH_HIERARCHY,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_adjacency_tree_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::ADJACENCY_TREE,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_group_into_blocks_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::GROUP_INTO_BLOCKS,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_recursive_collect_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::RECURSIVE_COLLECT,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_sequence_exists_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::SEQUENCE_EXISTS,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

/// The predicate of a recursive filter whose scope iterates an owned source.
pub(crate) fn owned_recursive_filter_predicate(scope: &TargetScope) -> Option<NodeId> {
    match (&scope.construction, &scope.iteration) {
        (TargetConstruction::RecursiveFilter { predicate, .. }, Some(iteration))
            if iteration.owned_source_iteration().is_some() =>
        {
            Some(*predicate)
        }
        _ => None,
    }
}

pub(crate) fn render_public_basic_owned_recursive_filter_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::RECURSIVE_FILTER,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_lookup_find_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::LOOKUP_FIND,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_dynamic_field_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::DYNAMIC_FIELD,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_sequence_aggregate_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::SEQUENCE_AGGREGATE,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_xml_serialize_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::XML_SERIALIZE,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_collection_aggregates_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::COLLECTION_AGGREGATES,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_joins_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::JOINS,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_xml_mixed_content_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::XML_MIXED_CONTENT,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

pub(crate) fn render_public_basic_owned_xml_mixed_construction_mapping(
    program: &Program,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(
        program,
        true,
        true,
        true,
        true,
        true,
        TargetExtension::XML_MIXED_CONSTRUCTION,
    )?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    Ok(source)
}

/// Owned mixed-content atomization; the runtime attributes only its own group
/// read and occurrence frames to this node.
pub(crate) fn render_owned_xml_mixed_content(
    node: NodeId,
    mixed: &::mapping::OwnedXmlMixedContent,
    output: &mut String,
) {
    output.push_str(&format!(
        " =>\n        context.OwnedXmlMixedContent(\n            {node}U,\n            "
    ));
    render_owner(&mixed.source.owner, output);
    output.push_str(",\n            ");
    mapping::basic_path(&mixed.source.path, output);
    output.push_str(",\n            ");
    mapping::render_optional_path(mixed.frame.as_deref(), output);
    output.push_str(",\n            ");
    if mixed.frame.as_ref().is_some_and(Vec::is_empty) {
        output.push_str("OwnedRootIsDeclaredNonrepeating(");
        render_owner(&mixed.source.owner, output);
        output.push(')');
    } else {
        output.push_str("false");
    }
    output.push_str(",\n            new global::Ferrule.Runtime.FerruleOwnedXmlMixedContentReplacement[]\n            {\n");
    for replacement in &mixed.replacements {
        output.push_str(&format!(
            "                new({}, {}, Node_{}),\n",
            literal::string(&replacement.element),
            replacement.context == ::mapping::OwnedXmlMixedContentContext::Occurrence,
            replacement.expression,
        ));
    }
    output.push_str("            });\n");
}
