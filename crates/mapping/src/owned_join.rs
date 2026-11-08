//! Inner-join plans whose sources and left keys carry an explicit endpoint
//! owner beside their endpoint-relative collection.
//!
//! The legacy [`crate::JoinPlan`] identifies a source by collection text
//! alone, so a primary `Rows` and a named `Rows` are the same source there.
//! This plan compares the complete owner and collection identity instead;
//! owned join reads bind tuple members by that identity.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use crate::{JoinSourceCardinality, OwnedSourceRef, SourceOwner};

fn is_repeating(cardinality: &JoinSourceCardinality) -> bool {
    *cardinality == JoinSourceCardinality::Repeating
}

/// One owned source participating in an inner join.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedJoinSource {
    collection: OwnedSourceRef,
    #[serde(default, skip_serializing_if = "is_repeating")]
    cardinality: JoinSourceCardinality,
}

impl OwnedJoinSource {
    pub fn new(collection: OwnedSourceRef) -> Self {
        Self {
            collection,
            cardinality: JoinSourceCardinality::Repeating,
        }
    }

    pub fn singleton(path: OwnedSourceRef) -> Self {
        Self {
            collection: path,
            cardinality: JoinSourceCardinality::Singleton,
        }
    }

    pub fn collection(&self) -> &OwnedSourceRef {
        &self.collection
    }

    pub const fn cardinality(&self) -> JoinSourceCardinality {
        self.cardinality
    }
}

/// One equality condition between an owned source already in the left tuple
/// and the source introduced by the current stage. Key paths stay relative to
/// their selected rows and never select an endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedJoinKey {
    left_collection: OwnedSourceRef,
    left_path: Vec<String>,
    right_path: Vec<String>,
}

impl OwnedJoinKey {
    pub fn new(
        left_collection: OwnedSourceRef,
        left_path: Vec<String>,
        right_path: Vec<String>,
    ) -> Self {
        Self {
            left_collection,
            left_path,
            right_path,
        }
    }

    pub fn left_collection(&self) -> &OwnedSourceRef {
        &self.left_collection
    }

    pub fn left_path(&self) -> &[String] {
        &self.left_path
    }

    pub fn right_path(&self) -> &[String] {
        &self.right_path
    }
}

/// Non-empty set of equality conditions for one owned join stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedJoinConditions {
    first: OwnedJoinKey,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    rest: Vec<OwnedJoinKey>,
}

impl OwnedJoinConditions {
    pub fn new(first: OwnedJoinKey) -> Self {
        Self {
            first,
            rest: Vec::new(),
        }
    }

    pub fn and(mut self, condition: OwnedJoinKey) -> Self {
        self.rest.push(condition);
        self
    }

    pub fn iter(&self) -> impl Iterator<Item = &OwnedJoinKey> {
        std::iter::once(&self.first).chain(&self.rest)
    }
}

/// One stage key with the earlier source its left side reads, resolved once
/// when the plan is built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedJoinStageKey {
    left: usize,
    left_source: OwnedJoinSource,
    key: OwnedJoinKey,
}

impl OwnedJoinStageKey {
    /// Position of the left source in [`OwnedJoinPlan::sources`].
    pub const fn left(&self) -> usize {
        self.left
    }

    pub fn left_source(&self) -> &OwnedJoinSource {
        &self.left_source
    }

    pub fn key(&self) -> &OwnedJoinKey {
        &self.key
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedJoinStep {
    source: OwnedJoinSource,
    conditions: OwnedJoinConditions,
    /// `conditions` in order with their left sources; derived, never on the
    /// wire, and left out of `Debug` so recorded program text stays stable.
    #[serde(skip)]
    keys: Vec<OwnedJoinStageKey>,
}

impl fmt::Debug for OwnedJoinStep {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OwnedJoinStep")
            .field("source", &self.source)
            .field("conditions", &self.conditions)
            .finish()
    }
}

/// Left-deep owned inner-join plan with at least two sources. Duplicate and
/// left-stage checks compare the complete owner and collection identity; the
/// owning scope's `JoinId` stays independent of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OwnedJoinPlan {
    first: OwnedJoinSource,
    second: OwnedJoinStep,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    rest: Vec<OwnedJoinStep>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedJoinPlanWire {
    first: OwnedJoinSource,
    second: OwnedJoinStep,
    #[serde(default)]
    rest: Vec<OwnedJoinStep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedJoinPlanError {
    DuplicateSource(OwnedSourceRef),
    UnknownLeftSource(OwnedSourceRef),
}

/// `primary `Rows`` or `named `lookup` `Rows``, matching how diagnostics
/// name an owned collection.
pub fn describe_owned_collection(source: &OwnedSourceRef) -> String {
    let path = source.path.join("/");
    match &source.owner {
        SourceOwner::Primary => format!("primary `{path}`"),
        SourceOwner::Named { name } => format!("named `{name}` `{path}`"),
    }
}

impl fmt::Display for OwnedJoinPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateSource(source) => write!(
                formatter,
                "owned join collection {} is used more than once",
                describe_owned_collection(source)
            ),
            Self::UnknownLeftSource(source) => write!(
                formatter,
                "owned join condition references collection {} before it is joined",
                describe_owned_collection(source)
            ),
        }
    }
}

impl std::error::Error for OwnedJoinPlanError {}

impl OwnedJoinPlan {
    pub fn new(
        first: OwnedJoinSource,
        second: OwnedJoinSource,
        conditions: OwnedJoinConditions,
    ) -> Result<Self, OwnedJoinPlanError> {
        if first.collection == second.collection {
            return Err(OwnedJoinPlanError::DuplicateSource(second.collection));
        }
        let keys = resolve_left_sources(&conditions, std::slice::from_ref(&first))?;
        Ok(Self {
            first,
            second: OwnedJoinStep {
                source: second,
                conditions,
                keys,
            },
            rest: Vec::new(),
        })
    }

    pub fn then(
        mut self,
        source: OwnedJoinSource,
        conditions: OwnedJoinConditions,
    ) -> Result<Self, OwnedJoinPlanError> {
        if self
            .sources()
            .any(|existing| existing.collection == source.collection)
        {
            return Err(OwnedJoinPlanError::DuplicateSource(source.collection));
        }
        let keys = resolve_left_sources(&conditions, self.sources())?;
        self.rest.push(OwnedJoinStep {
            source,
            conditions,
            keys,
        });
        Ok(self)
    }

    pub fn first(&self) -> &OwnedJoinSource {
        &self.first
    }

    pub fn sources(&self) -> impl Iterator<Item = &OwnedJoinSource> {
        std::iter::once(&self.first)
            .chain(std::iter::once(&self.second.source))
            .chain(self.rest.iter().map(|step| &step.source))
    }

    pub fn stages(&self) -> impl Iterator<Item = (&OwnedJoinSource, &OwnedJoinConditions)> {
        std::iter::once((&self.second.source, &self.second.conditions)).chain(
            self.rest
                .iter()
                .map(|step| (&step.source, &step.conditions)),
        )
    }

    /// Each stage's source with its keys, every key carrying its left source.
    pub fn keyed_stages(&self) -> impl Iterator<Item = (&OwnedJoinSource, &[OwnedJoinStageKey])> {
        std::iter::once(&self.second)
            .chain(&self.rest)
            .map(|step| (&step.source, step.keys.as_slice()))
    }
}

impl<'de> Deserialize<'de> for OwnedJoinPlan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = OwnedJoinPlanWire::deserialize(deserializer)?;
        let mut plan = Self::new(wire.first, wire.second.source, wire.second.conditions)
            .map_err(serde::de::Error::custom)?;
        for step in wire.rest {
            plan = plan
                .then(step.source, step.conditions)
                .map_err(serde::de::Error::custom)?;
        }
        Ok(plan)
    }
}

fn resolve_left_sources<'a>(
    conditions: &OwnedJoinConditions,
    sources: impl IntoIterator<Item = &'a OwnedJoinSource>,
) -> Result<Vec<OwnedJoinStageKey>, OwnedJoinPlanError> {
    let sources: Vec<_> = sources.into_iter().collect();
    conditions
        .iter()
        .map(|condition| {
            sources
                .iter()
                .enumerate()
                .find(|(_, source)| source.collection == condition.left_collection)
                .map(|(left, source)| OwnedJoinStageKey {
                    left,
                    left_source: (*source).clone(),
                    key: condition.clone(),
                })
                .ok_or_else(|| {
                    OwnedJoinPlanError::UnknownLeftSource(condition.left_collection.clone())
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource, Project, Scope, ScopeIteration,
        project_file,
    };

    fn owned(owner: SourceOwner, path: &[&str]) -> OwnedSourceRef {
        OwnedSourceRef {
            owner,
            path: path.iter().map(|segment| (*segment).to_owned()).collect(),
        }
    }

    fn named(path: &[&str]) -> OwnedSourceRef {
        owned(
            SourceOwner::Named {
                name: "lookup".into(),
            },
            path,
        )
    }

    fn primary(path: &[&str]) -> OwnedSourceRef {
        owned(SourceOwner::Primary, path)
    }

    fn key(left: OwnedSourceRef) -> OwnedJoinConditions {
        OwnedJoinConditions::new(OwnedJoinKey::new(
            left,
            vec!["id".into()],
            vec!["id".into()],
        ))
    }

    fn plan() -> OwnedJoinPlan {
        OwnedJoinPlan::new(
            OwnedJoinSource::new(primary(&["Rows"])),
            OwnedJoinSource::new(named(&["Rows"])),
            key(primary(&["Rows"])),
        )
        .expect("primary and named Rows are distinct sources")
    }

    fn project(iteration: ScopeIteration) -> Project {
        Project {
            execution_contract: None,
            source: ir::SchemaNode::group("Input", vec![]),
            target: ir::SchemaNode::group("Output", vec![]),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![],
            failure_rules: vec![],
            user_functions: Default::default(),
            graph: Default::default(),
            root: Scope {
                children: vec![Scope {
                    target_field: "Row".into(),
                    iteration,
                    ..Scope::default()
                }],
                ..Scope::default()
            },
        }
    }

    #[test]
    fn same_collection_text_under_different_owners_is_two_sources() {
        let plan = plan();
        let sources: Vec<_> = plan.sources().map(OwnedJoinSource::collection).collect();
        assert_eq!(sources, [&primary(&["Rows"]), &named(&["Rows"])]);
        // The singleton third stage may key on the named source by identity.
        let plan = plan
            .then(
                OwnedJoinSource::singleton(named(&["Header", "id"])),
                key(named(&["Rows"])),
            )
            .unwrap();
        assert_eq!(plan.stages().count(), 2);
        let wire = serde_json::to_value(&plan).unwrap();
        assert_eq!(serde_json::from_value::<OwnedJoinPlan>(wire).unwrap(), plan);
    }

    #[test]
    fn stage_keys_carry_their_resolved_left_source() {
        let plan = plan()
            .then(
                OwnedJoinSource::singleton(named(&["Header", "id"])),
                key(named(&["Rows"])).and(OwnedJoinKey::new(
                    primary(&["Rows"]),
                    vec!["code".into()],
                    vec!["code".into()],
                )),
            )
            .unwrap();
        let stages: Vec<_> = plan
            .keyed_stages()
            .map(|(source, keys)| {
                let lefts: Vec<_> = keys
                    .iter()
                    .map(|key| (key.left(), key.left_source().collection().clone()))
                    .collect();
                (source.collection().clone(), lefts)
            })
            .collect();
        assert_eq!(
            stages,
            [
                (named(&["Rows"]), vec![(0, primary(&["Rows"]))]),
                (
                    named(&["Header", "id"]),
                    vec![(1, named(&["Rows"])), (0, primary(&["Rows"]))],
                ),
            ]
        );
        assert_eq!(plan.first().collection(), &primary(&["Rows"]));
        // Resolution is derived, not wire state: a round trip resolves again.
        let wire = serde_json::to_value(&plan).unwrap();
        assert!(!wire.to_string().contains("keys"));
        assert_eq!(serde_json::from_value::<OwnedJoinPlan>(wire).unwrap(), plan);
    }

    #[test]
    fn duplicate_and_unknown_identities_are_rejected() {
        assert_eq!(
            OwnedJoinPlan::new(
                OwnedJoinSource::new(named(&["Rows"])),
                OwnedJoinSource::singleton(named(&["Rows"])),
                key(named(&["Rows"])),
            ),
            Err(OwnedJoinPlanError::DuplicateSource(named(&["Rows"])))
        );
        // A left key naming the right collection's text under another owner
        // is not a joined source.
        assert_eq!(
            OwnedJoinPlan::new(
                OwnedJoinSource::new(primary(&["Rows"])),
                OwnedJoinSource::new(named(&["Lines"])),
                key(named(&["Rows"])),
            ),
            Err(OwnedJoinPlanError::UnknownLeftSource(named(&["Rows"])))
        );
        assert_eq!(
            plan().then(
                OwnedJoinSource::new(primary(&["Rows"])),
                key(named(&["Rows"])),
            ),
            Err(OwnedJoinPlanError::DuplicateSource(primary(&["Rows"])))
        );
        assert_eq!(
            OwnedJoinPlanError::UnknownLeftSource(named(&["Rows"])).to_string(),
            "owned join condition references collection named `lookup` `Rows` before it is joined"
        );
    }

    #[test]
    fn malformed_owned_plans_are_rejected() {
        let valid = r#"{"first":{"collection":{"owner":{"kind":"primary"},"path":["Rows"]}},"second":{"source":{"collection":{"owner":{"kind":"named","name":"lookup"},"path":["Rows"]},"cardinality":"singleton"},"conditions":{"first":{"left_collection":{"owner":{"kind":"primary"},"path":["Rows"]},"left_path":["id"],"right_path":["id"]}}}}"#;
        assert!(serde_json::from_str::<OwnedJoinPlan>(valid).is_ok());
        for (from, to) in [
            // Malformed cardinality and owners.
            (r#""singleton""#, r#""optional""#),
            (
                r#"{"kind":"named","name":"lookup"}"#,
                r#"{"kind":"named","name":""}"#,
            ),
            (
                r#"{"kind":"named","name":"lookup"}"#,
                r#"{"kind":"remote"}"#,
            ),
            // Mixed legacy and owned identities.
            (
                r#""left_collection":{"owner":{"kind":"primary"},"path":["Rows"]}"#,
                r#""left_collection":["Rows"]"#,
            ),
            (
                r#""collection":{"owner":{"kind":"named","name":"lookup"},"path":["Rows"]}"#,
                r#""collection":["Rows"]"#,
            ),
            // Unknown and duplicate fields.
            (
                r#""right_path":["id"]"#,
                r#""right_path":["id"],"owner":{"kind":"primary"}"#,
            ),
            (
                r#""cardinality":"singleton""#,
                r#""cardinality":"singleton","frame":[]"#,
            ),
            (
                r#""left_path":["id"]"#,
                r#""left_path":["id"],"left_path":["id"]"#,
            ),
            // Unknown left identity.
            (
                r#""left_collection":{"owner":{"kind":"primary"},"path":["Rows"]}"#,
                r#""left_collection":{"owner":{"kind":"named","name":"other"},"path":["Rows"]}"#,
            ),
        ] {
            assert_eq!(valid.matches(from).count(), 1, "{from}");
            let invalid = valid.replace(from, to);
            assert!(
                serde_json::from_str::<OwnedJoinPlan>(&invalid).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn legacy_join_bytes_are_unchanged_and_old_readers_refuse_owned_joins() {
        let legacy = JoinPlan::new(
            JoinSource::new(vec!["Rows".into()]),
            JoinSource::singleton(vec!["Header".into(), "id".into()]),
            JoinConditions::new(JoinKey::new(vec!["Rows".into()], vec!["id".into()], vec![])),
        )
        .unwrap();
        let legacy = project(ScopeIteration::InnerJoin {
            id: JoinId::new(7),
            plan: legacy,
        });
        let text = serde_json::to_string(&legacy.root.children[0]).unwrap();
        assert!(
            text.contains(r#""join":{"id":7,"plan":{"first":{"collection":["Rows"]},"second":{"source":{"collection":["Header","id"],"cardinality":"singleton"},"conditions":{"first":{"left_collection":["Rows"],"left_path":["id"],"right_path":[]}}}}}"#),
            "{text}"
        );
        assert_eq!(
            serde_json::from_str::<Scope>(&text)
                .unwrap()
                .iteration
                .join(),
            legacy.root.children[0].iteration.join()
        );
        assert_eq!(crate::first_project_owned_source_scope(&legacy), None);

        let owned = project(ScopeIteration::OwnedInnerJoin {
            id: JoinId::new(7),
            plan: plan(),
        });
        let text = project_file::encode_pretty(&owned).unwrap();
        let decoded = project_file::decode_str(&text).unwrap();
        assert_eq!(
            decoded.root.children[0].iteration.owned_join(),
            Some((JoinId::new(7), &plan()))
        );
        assert_eq!(decoded.root.children[0].iteration.join(), None);
        assert!(crate::first_project_owned_source_scope(&decoded).is_some());

        // A reader that only knows collection arrays refuses the same `join`
        // key instead of dropping the owner or the iteration.
        let scope = serde_json::to_value(&owned.root.children[0]).unwrap();
        assert!(serde_json::from_value::<JoinPlan>(scope["join"]["plan"].clone()).is_err());
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct OldJoin {
            id: JoinId,
            plan: JoinPlan,
        }
        assert!(serde_json::from_value::<OldJoin>(scope["join"].clone()).is_err());
    }
}
