use ir::{ScalarType, SchemaKind, Value};
use mapping::{Node, NodeId};

use super::GraphBuilder;
use super::function::{DbPredicateOperator, DbWhereComponent};
use super::source::SourcePath;

impl GraphBuilder<'_> {
    pub(super) fn warn_conflicting_db_sort(&mut self, target_path: &[String]) {
        self.warnings.push(format!(
            "iteration into `{}` combines database ORDER with another sort, which cannot be represented exactly; iteration skipped",
            target_path.join("/")
        ));
    }

    pub(super) fn apply_db_where(
        &mut self,
        index: Option<usize>,
        source_path: Option<&SourcePath>,
        existing_filter: Option<NodeId>,
    ) -> Result<(Option<NodeId>, Option<NodeId>, bool), String> {
        let Some(index) = index else {
            return Ok((existing_filter, None, false));
        };
        let (predicate, sort, descending) = self.db_where_nodes(index, source_path)?;
        let filter = Some(match existing_filter {
            Some(existing) => self.alloc(Node::Call {
                function: "and".to_string(),
                args: vec![existing, predicate],
            }),
            None => predicate,
        });
        Ok((filter, sort, descending))
    }

    fn db_where_nodes(
        &mut self,
        index: usize,
        source_path: Option<&SourcePath>,
    ) -> Result<(NodeId, Option<NodeId>, bool), String> {
        let source_path =
            source_path.ok_or_else(|| "collection input is unresolved".to_string())?;
        let control = match self.fn_components[index].db_where.clone() {
            Some(DbWhereComponent::Supported(control)) => control,
            Some(DbWhereComponent::Unsupported(reason)) => return Err(reason),
            None => return Err("where metadata is missing".to_string()),
        };
        let parameter_feed = self
            .input_feed(index, 1)
            .ok_or_else(|| "parameter input is not connected".to_string())?;
        let parameter = self
            .value_node(parameter_feed)
            .ok_or_else(|| "parameter expression is unsupported".to_string())?;
        if control.parameter_type != ScalarType::String {
            return Err("only string parameter expressions are supported".to_string());
        }
        let (value, value_type) = self.db_column_node(source_path, &control.predicate.column)?;
        if value_type != control.parameter_type {
            return Err(format!(
                "column type {value_type:?} does not match parameter type {:?}",
                control.parameter_type
            ));
        }
        let function = match control.predicate.operator {
            DbPredicateOperator::Equal => "equal",
            DbPredicateOperator::Like => "sql_like",
        };
        let comparison = self.alloc(Node::Call {
            function: function.to_string(),
            args: vec![value, parameter],
        });
        let value_exists = self.alloc(Node::Call {
            function: "exists".to_string(),
            args: vec![value],
        });
        let parameter_exists = self.alloc(Node::Call {
            function: "exists".to_string(),
            args: vec![parameter],
        });
        let both_exist = self.alloc(Node::Call {
            function: "and".to_string(),
            args: vec![value_exists, parameter_exists],
        });
        let false_value = self.alloc(Node::Const {
            value: Value::Bool(false),
        });
        let predicate = self.alloc(Node::If {
            condition: both_exist,
            then: comparison,
            else_: false_value,
        });
        let (sort, descending) = match control.order {
            Some(order) => (
                Some(self.db_column_node(source_path, &order.column)?.0),
                order.descending,
            ),
            None => (None, false),
        };
        Ok((predicate, sort, descending))
    }

    pub(super) fn db_column_node(
        &mut self,
        source_path: &SourcePath,
        identifier: &[String],
    ) -> Result<(NodeId, ScalarType), String> {
        let (column, qualifier) = match identifier {
            [column] => (column.as_str(), None),
            [qualifier, column] => (column.as_str(), Some(qualifier.as_str())),
            _ => return Err("column identifiers may contain at most one qualifier".to_string()),
        };
        let original_collection = self
            .schema_node(source_path)
            .ok_or_else(|| "collection schema is unresolved".to_string())?
            .name
            .clone();
        let mut candidate_path = source_path.path.clone();
        let mut matched_field = None;
        loop {
            let candidate = SourcePath {
                source: source_path.source,
                path: candidate_path.clone(),
            };
            let collection = self
                .schema_node(&candidate)
                .ok_or_else(|| "collection schema is unresolved".to_string())?;
            let physical_name = collection
                .name
                .split_once('|')
                .map_or(collection.name.as_str(), |(table, _)| table);
            if qualifier.is_none_or(|qualifier| physical_name.eq_ignore_ascii_case(qualifier)) {
                let SchemaKind::Group { children, .. } = &collection.kind else {
                    return Err(format!("collection `{}` is not a group", collection.name));
                };
                let matches = children
                    .iter()
                    .filter(|child| child.name.eq_ignore_ascii_case(column))
                    .collect::<Vec<_>>();
                if matches.len() > 1 {
                    return Err(format!(
                        "column `{column}` is ambiguous ignoring ASCII case"
                    ));
                }
                if let Some(matched) = matches.first()
                    && !matched.repeating
                    && let SchemaKind::Scalar { ty } = matched.kind
                {
                    let mut field = candidate;
                    field.path.push(matched.name.clone());
                    matched_field = Some((field, ty));
                    break;
                }
            }
            let Some((_, parent)) = candidate_path.split_last() else {
                break;
            };
            candidate_path = parent.to_vec();
        }
        let Some((field, column_type)) = matched_field else {
            if let Some(qualifier) = qualifier {
                return Err(format!(
                    "column qualifier `{qualifier}` does not match a scalar `{column}` field of collection `{original_collection}` or an ancestor"
                ));
            }
            return Err(format!(
                "column `{column}` is not a scalar field of collection `{original_collection}` or an ancestor"
            ));
        };
        self.source_field_at(&field)
            .map(|node| (node, column_type))
            .ok_or_else(|| format!("column `{column}` cannot be resolved"))
    }
}
