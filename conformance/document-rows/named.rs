pub fn project(mut project: mapping::Project) -> mapping::Project {
    project.extra_targets.push(mapping::NamedTarget {
        name: "selected".into(),
        path: None,
        schema: project.target.clone(),
        options: Default::default(),
        root: project.root.clone(),
    });
    // The primary has intentionally different content. A selected visitor must
    // not execute it or consume its counter calls.
    project.root.children[0].bindings[0].node = 6;
    project
}
