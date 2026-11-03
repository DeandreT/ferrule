//! A builder-owned postcondition. Fingerprints detect changes, not ownership.
use super::*;
use mapping::{Project, Scope};
use sha2::{Digest, Sha256};

#[derive(Default)]
pub(in crate::import) struct CountSeal {
    pub(super) failure: Option<String>,
    earlier: BTreeSet<(usize, NodeId)>,
    pub(super) receipts: BTreeMap<UseId, Receipt>,
    pub(super) templates: BTreeSet<NodeId>,
    pub(super) budget: Option<CountConstruction>,
}
pub(super) struct Receipt {
    pub root: NodeId,
    pub facts: Vec<receiver::Fact>,
    pub nodes: BTreeMap<NodeId, [u8; 32]>,
    pub mode: Mode,
}

impl Clone for CountSeal {
    fn clone(&self) -> Self {
        count_visit(Counter::Speculative);
        let Some(budget) = &self.budget else {
            return Self {
                failure: self.failure.clone(),
                earlier: self.earlier.clone(),
                ..Self::default()
            };
        };
        let copy = (|| {
            budget.claim(
                self.earlier.len() + self.templates.len() + self.receipts.len(),
                self.failure.as_ref().map_or(0, String::len),
                0,
            )?;
            let mut receipts = BTreeMap::new();
            for (&id, receipt) in &self.receipts {
                budget.claim(receipt.nodes.len() + receipt.facts.len(), 0, 0)?;
                let mut facts = Vec::new();
                for fact in &receipt.facts {
                    facts.push(receiver::Fact {
                        path: copy_path(&fact.path, budget)?,
                        source: fact
                            .source
                            .as_ref()
                            .map(|p| copy_path(p, budget))
                            .transpose()?,
                        output: fact.output,
                        sort_descending: fact.sort_descending,
                        sort_filter_order: fact.sort_filter_order,
                    });
                }
                let mode = match &receipt.mode {
                    Mode::CurrentItem => Mode::CurrentItem,
                    Mode::Sum(path) => Mode::Sum(copy_path(path, budget)?),
                };
                receipts.insert(
                    id,
                    Receipt {
                        root: receipt.root,
                        nodes: receipt.nodes.clone(),
                        facts,
                        mode,
                    },
                );
            }
            Ok::<_, String>(Self {
                failure: self.failure.clone(),
                earlier: self.earlier.clone(),
                receipts,
                templates: self.templates.clone(),
                budget: Some(budget.clone()),
            })
        })();
        copy.unwrap_or_else(|reason| Self {
            failure: Some(reason),
            budget: Some(budget.clone()),
            ..Self::default()
        })
    }
}
impl CountSeal {
    pub(in crate::import) fn earlier(&mut self, index: usize, node: NodeId) {
        self.earlier.insert((index, node));
    }
}

pub(super) fn fingerprint(
    value: &impl serde::Serialize,
    budget: &CountConstruction,
) -> Result<[u8; 32], String> {
    struct Writer<'a> {
        hash: Sha256,
        budget: &'a CountConstruction,
        depth: usize,
        quoted: bool,
        escaped: bool,
    }
    impl std::io::Write for Writer<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.budget
                .claim(1, bytes.len(), 0)
                .map_err(std::io::Error::other)?;
            for &byte in bytes {
                if self.quoted {
                    if self.escaped {
                        self.escaped = false;
                    } else if byte == b'\\' {
                        self.escaped = true;
                    } else if byte == b'"' {
                        self.quoted = false;
                    }
                } else {
                    match byte {
                        b'"' => self.quoted = true,
                        b'{' | b'[' => {
                            self.depth += 1;
                            self.budget
                                .claim(0, 0, self.depth)
                                .map_err(std::io::Error::other)?;
                        }
                        b'}' | b']' => self.depth = self.depth.saturating_sub(1),
                        _ => {}
                    }
                }
            }
            self.hash.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Writer {
        hash: Sha256::new(),
        budget,
        depth: 0,
        quoted: false,
        escaped: false,
    };
    serde_json::to_writer(&mut writer, value)
        .map_err(|_| "Count structural seal quota exhausted")?;
    Ok(writer.hash.finalize().into())
}

impl CountProofTable {
    pub(in crate::import) fn audit(
        &self,
        seal: &CountSeal,
        project: &Project,
        descriptors: &[&SchemaComponent],
        declared_guard: bool,
    ) -> Result<(), String> {
        #[cfg(test)]
        if let Some(result) =
            super::tests::probe_audit(self, seal, project, descriptors, declared_guard)
        {
            return result;
        }
        self.audit_inner(seal, project, descriptors, declared_guard)
    }
    pub(super) fn audit_inner(
        &self,
        seal: &CountSeal,
        project: &Project,
        descriptors: &[&SchemaComponent],
        declared_guard: bool,
    ) -> Result<(), String> {
        if self.discovery_incomplete {
            return Err(self
                .failure
                .clone()
                .unwrap_or_else(|| "incomplete Count refusal discovery".into()));
        }
        if self.candidates.is_empty() {
            if let Some(reason) = &seal.failure {
                return Err(reason.clone());
            }
            return Ok(());
        }
        let budget = self
            .budget
            .as_ref()
            .ok_or("Count has no construction authority")?;
        if let Some(reason) = &seal.failure {
            return Err(format!("physical filtered count: {reason}"));
        }
        let earlier = |candidate: &Candidate| {
            candidate.registration.is_some_and(|i| {
                seal.earlier
                    .iter()
                    .any(|(index, node)| *index == i && project.graph.nodes.contains_key(node))
            })
        };
        if self.candidates.iter().all(|c| !c.connected || earlier(c)) {
            return Ok(());
        }
        budget.claim(self.candidates.len() + seal.earlier.len() + 1, 0, 0)?;
        let physical = self
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.connected
                    && !c.registration.is_some_and(|i| {
                        seal.earlier.iter().any(|(index, node)| {
                            *index == i && project.graph.nodes.contains_key(node)
                        })
                    })
            })
            .map(|(id, _)| id)
            .collect::<BTreeSet<_>>();
        if physical.is_empty() {
            return Ok(());
        }
        if declared_guard {
            return Err("physical filtered Count cannot compose with the declared-library execution profile".into());
        }
        if let Some(reason) = &self.failure {
            return Err(format!("physical filtered count: {reason}"));
        }
        for &id in &physical {
            self.candidates[id]
                .plan
                .as_ref()
                .map_err(|e| format!("physical filtered count: {e}"))?;
        }
        if descriptors.len() != project.extra_targets.len() + 1 || !project.extra_sources.is_empty()
        {
            return Err("physical filtered count final boundary set mismatch".into());
        }
        if self.source != Some(fingerprint(&project.source, budget)?) {
            return Err("physical filtered count final source changed".into());
        }
        // These fields are importer diagnostics, not reader policy. All actual
        // format options remain covered by the original reader fingerprint.
        fingerprint(&project.source_options, budget)?;
        let mut options = project.source_options.clone();
        options.mfd_decimal_input_names.clear();
        options.mfd_optional_parser_input_names.clear();
        options.mfd_json5_string_dependencies.clear();
        if self.source_options != Some(fingerprint(&options, budget)?) {
            return Err("physical filtered count reader policy changed".into());
        }
        let mut roots = BTreeMap::new();
        for (descriptor, (schema, root, options)) in descriptors.iter().zip(
            std::iter::once((&project.target, &project.root, &project.target_options)).chain(
                project
                    .extra_targets
                    .iter()
                    .map(|t| (&t.schema, &t.root, &t.options)),
            ),
        ) {
            budget.claim(1, 0, 0)?;
            if let Some(&slot) = self
                .targets
                .get(&(std::ptr::from_ref(*descriptor) as usize))
            {
                if self.target_schemas.get(&slot) != Some(&fingerprint(schema, budget)?) {
                    return Err("physical filtered count target schema changed".into());
                }
                if self.target_options.get(&slot) != Some(&fingerprint(options, budget)?) {
                    return Err("physical filtered count target options changed".into());
                }
                if roots.insert(slot, root).is_some() {
                    return Err("physical filtered count duplicate target slot".into());
                }
            }
        }
        let mut owned = BTreeMap::new();
        let mut allowed_roots = BTreeMap::new();
        for (&id, use_) in &self.uses {
            if !physical.contains(&use_.candidate) {
                continue;
            }
            count_visit(Counter::Audit);
            budget.claim(1, 0, 0)?;
            let receipt = seal
                .receipts
                .get(&id)
                .ok_or("physical filtered count missing authored use receipt")?;
            let root = *roots
                .get(&id.target)
                .ok_or("physical filtered count missing final target")?;
            let plan = self.candidates[use_.candidate]
                .plan
                .as_ref()
                .map_err(String::clone)?;
            let proof = receiver::prove(&project.source, plan, use_, root, None, budget)?;
            if proof.facts != receipt.facts || proof.mode != receipt.mode {
                return Err("physical filtered count receiving facts changed".into());
            }
            let mut scope = root;
            for segment in &use_.path[..use_.path.len() - 1] {
                budget.claim(scope.children.len(), segment.len(), 0)?;
                let mut matches = scope.children.iter().filter(|s| s.target_field == *segment);
                scope = matches.next().ok_or("missing final Count receiver")?;
                if matches.next().is_some() {
                    return Err("duplicate final Count receiver".into());
                }
            }
            if scope
                .bindings
                .iter()
                .filter(|b| b.target_field == *use_.path.last().unwrap_or(&String::new()))
                .count()
                != 1
                || !scope
                    .bindings
                    .iter()
                    .any(|b| b.node == receipt.root && Some(&b.target_field) == use_.path.last())
            {
                return Err(
                    "physical filtered count binding was removed, replaced or duplicated".into(),
                );
            }
            for (&node, expected) in &receipt.nodes {
                budget.claim(1, 0, 0)?;
                let actual = project
                    .graph
                    .nodes
                    .get(&node)
                    .ok_or("physical filtered count subgraph missing")?;
                if fingerprint(actual, budget)? != *expected {
                    return Err("physical filtered count subgraph changed".into());
                }
                if owned.insert(node, id).is_some() {
                    return Err("physical filtered count subgraphs were shared".into());
                }
            }
            allowed_roots.insert(receipt.root, 0usize);
        }
        if seal.receipts.keys().any(|id| !self.uses.contains_key(id)) {
            return Err("unexpected physical Count receipt".into());
        }
        // Count fresh graphs have no cross-use edges. No later wrapper/control
        // may acquire a reference to any internal Count node or root template.
        for (&id, node) in &project.graph.nodes {
            budget.claim(1, 0, 0)?;
            if seal.templates.contains(&id) {
                return Err("unclaimed eager Count template survived pruning".into());
            }
            let mut count = 0;
            node.visit_inputs(&mut |_| {
                count += 1;
            });
            budget.claim(count, 0, 0)?;
            for dependency in node.dependencies() {
                if let Some(owner) = owned.get(&dependency)
                    && owned.get(&id) != Some(owner)
                {
                    return Err("physical filtered count gained an indirect consumer".into());
                }
            }
        }
        fn scan(
            scope: &Scope,
            owned: &BTreeMap<NodeId, UseId>,
            allowed: &mut BTreeMap<NodeId, usize>,
            budget: &CountConstruction,
            depth: usize,
        ) -> Result<(), String> {
            budget.claim(1, 0, depth)?;
            let mut failure = None;
            scope.visit_roots(&mut |root| {
                if budget.claim(1, 0, depth).is_err() {
                    failure = Some("Count root scan quota exhausted");
                }
                if owned.contains_key(&root.node) {
                    if !matches!(root.slot, mapping::ScopeRootSlot::Binding(_))
                        || !allowed.contains_key(&root.node)
                    {
                        failure = Some("extra affected Count executable root");
                    } else if let Some(n) = allowed.get_mut(&root.node) {
                        *n += 1;
                    }
                }
            });
            if let Some(reason) = failure {
                return Err(reason.into());
            }
            for (_, child) in scope.nested_scopes() {
                scan(child, owned, allowed, budget, depth + 1)?;
            }
            Ok(())
        }
        for root in
            std::iter::once(&project.root).chain(project.extra_targets.iter().map(|t| &t.root))
        {
            scan(root, &owned, &mut allowed_roots, budget, 0)?;
        }
        for failure in &project.failure_rules {
            let mut invalid = false;
            failure.visit_roots(&mut |root| {
                if owned.contains_key(&root.node) {
                    invalid = true;
                }
            });
            if invalid {
                return Err("physical Count gained failure-rule consumer".into());
            }
        }
        if allowed_roots.values().any(|n| *n != 1) {
            return Err("physical Count final fan-out mismatch".into());
        }
        Ok(())
    }
}
