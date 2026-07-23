use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use ir::Value;
use mapping::{FunctionId, FunctionParameterId, Node, NodeId, Project, UserFunction};

use crate::MfdError;

use super::function::{
    constant_parts, function_library, scalar_type_name, unmap_function_name, value_scalar_type,
    value_text,
};
use super::schema::{KeyAlloc, xml_escape};

struct ParameterPort {
    id: FunctionParameterId,
    name: String,
    component_id: u32,
}

struct Interface {
    library: String,
    name: String,
    parameters: Vec<ParameterPort>,
    output_name: String,
    output_component_id: u32,
}

pub(super) struct Exports {
    interfaces: BTreeMap<FunctionId, Interface>,
    declarations: String,
}

impl Exports {
    pub(super) fn build(
        project: &Project,
        keys: &mut KeyAlloc,
        uid: &mut u32,
    ) -> Result<Self, MfdError> {
        validate_call_graph(project)?;
        let mut names = BTreeSet::new();
        let mut interfaces = BTreeMap::new();
        for (&id, function) in &project.user_functions {
            validate_function(id, function)?;
            if !names.insert((function.library.clone(), function.name.clone())) {
                return Err(unsupported(format!(
                    "user-defined function `{}` ({}) has a duplicate library/name pair",
                    function.name, function.library
                )));
            }
            let parameters = function
                .parameters
                .iter()
                .map(|parameter| {
                    *uid += 1;
                    ParameterPort {
                        id: parameter.id,
                        name: parameter.name.clone(),
                        component_id: *uid,
                    }
                })
                .collect();
            *uid += 1;
            interfaces.insert(
                id,
                Interface {
                    library: function.library.clone(),
                    name: function.name.clone(),
                    parameters,
                    output_name: function.output_name.clone(),
                    output_component_id: *uid,
                },
            );
        }

        for (&caller, function) in &project.user_functions {
            validate_calls(caller, &function.body.nodes, &interfaces)?;
        }
        validate_calls(FunctionId::new(0), &project.graph.nodes, &interfaces)?;

        let mut declarations = String::new();
        for (&id, function) in &project.user_functions {
            let interface = interfaces.get(&id).ok_or_else(|| {
                unsupported(format!(
                    "user-defined function `{}` has no export interface",
                    function.name
                ))
            })?;
            render_definition(
                function,
                interface,
                &interfaces,
                keys,
                uid,
                &mut declarations,
            )?;
        }
        Ok(Self {
            interfaces,
            declarations,
        })
    }

    pub(super) fn declarations(&self) -> &str {
        &self.declarations
    }

    pub(super) fn render_call(
        &self,
        function: FunctionId,
        keys: &mut KeyAlloc,
        uid: &mut u32,
        components: &mut String,
    ) -> Option<(Vec<u32>, u32)> {
        let interface = self.interfaces.get(&function)?;
        Some(render_call_component(
            interface, keys, uid, components, "\t\t\t\t",
        ))
    }
}

fn validate_function(id: FunctionId, function: &UserFunction) -> Result<(), MfdError> {
    if function.library.trim().is_empty()
        || function.name.trim().is_empty()
        || function.output_name.trim().is_empty()
    {
        return Err(unsupported(format!(
            "user-defined function {} has an empty library, name, or output name",
            id.get()
        )));
    }
    if !function.body.nodes.contains_key(&function.output) {
        return Err(unsupported(format!(
            "user-defined function `{}` references missing output node {}",
            function.name, function.output
        )));
    }
    let mut parameter_ids = BTreeSet::new();
    let mut parameter_names = BTreeSet::new();
    for parameter in &function.parameters {
        if parameter.name.trim().is_empty()
            || !parameter_ids.insert(parameter.id)
            || !parameter_names.insert(parameter.name.as_str())
        {
            return Err(unsupported(format!(
                "user-defined function `{}` has duplicate or empty parameters",
                function.name
            )));
        }
    }
    let reachable = reachable_nodes(function)?;
    for node_id in reachable {
        let node = function.body.nodes.get(&node_id).ok_or_else(|| {
            unsupported(format!(
                "user-defined function `{}` references missing node {node_id}",
                function.name
            ))
        })?;
        match node {
            Node::FunctionParameter { parameter } if parameter_ids.contains(parameter) => {}
            Node::Unconnected
            | Node::Const {
                value:
                    Value::Null
                    | Value::XmlNil(_)
                    | Value::Bool(_)
                    | Value::Int(_)
                    | Value::Float(_)
                    | Value::String(_),
            }
            | Node::Call { .. }
            | Node::UserFunctionCall { .. }
            | Node::If { .. }
            | Node::ValueMap { .. } => {}
            Node::FunctionParameter { parameter } => {
                return Err(unsupported(format!(
                    "user-defined function `{}` uses undeclared parameter {}",
                    function.name,
                    parameter.get()
                )));
            }
            _ => {
                return Err(unsupported(format!(
                    "user-defined function `{}` contains a context-sensitive or non-scalar node",
                    function.name
                )));
            }
        }
    }
    Ok(())
}

fn reachable_nodes(function: &UserFunction) -> Result<BTreeSet<NodeId>, MfdError> {
    let mut reachable = BTreeSet::new();
    let mut pending = vec![function.output];
    while let Some(id) = pending.pop() {
        if !reachable.insert(id) {
            continue;
        }
        let node = function.body.nodes.get(&id).ok_or_else(|| {
            unsupported(format!(
                "user-defined function `{}` references missing node {id}",
                function.name
            ))
        })?;
        pending.extend(node.dependencies());
    }
    Ok(reachable)
}

fn validate_calls(
    caller: FunctionId,
    nodes: &BTreeMap<NodeId, Node>,
    interfaces: &BTreeMap<FunctionId, Interface>,
) -> Result<(), MfdError> {
    for node in nodes.values() {
        let Node::UserFunctionCall { function, args } = node else {
            continue;
        };
        let interface = interfaces.get(function).ok_or_else(|| {
            unsupported(format!(
                "graph {} calls missing user-defined function {}",
                caller.get(),
                function.get()
            ))
        })?;
        if args.len() != interface.parameters.len() {
            return Err(unsupported(format!(
                "call to user-defined function `{}` has {} arguments, expected {}",
                interface.name,
                args.len(),
                interface.parameters.len()
            )));
        }
    }
    Ok(())
}

fn validate_call_graph(project: &Project) -> Result<(), MfdError> {
    fn visit(
        id: FunctionId,
        project: &Project,
        active: &mut BTreeSet<FunctionId>,
        complete: &mut BTreeSet<FunctionId>,
        depth: usize,
    ) -> Result<(), MfdError> {
        if complete.contains(&id) {
            return Ok(());
        }
        if depth >= 64 || !active.insert(id) {
            return Err(unsupported(format!(
                "user-defined function call graph is recursive or exceeds 64 levels at function {}",
                id.get()
            )));
        }
        let function = project
            .user_functions
            .get(&id)
            .ok_or_else(|| unsupported(format!("missing user-defined function {}", id.get())))?;
        for callee in function.body.nodes.values().filter_map(|node| match node {
            Node::UserFunctionCall { function, .. } => Some(*function),
            _ => None,
        }) {
            visit(callee, project, active, complete, depth + 1)?;
        }
        active.remove(&id);
        complete.insert(id);
        Ok(())
    }

    let mut complete = BTreeSet::new();
    for &id in project.user_functions.keys() {
        visit(id, project, &mut BTreeSet::new(), &mut complete, 0)?;
    }
    Ok(())
}

fn render_definition(
    function: &UserFunction,
    interface: &Interface,
    interfaces: &BTreeMap<FunctionId, Interface>,
    keys: &mut KeyAlloc,
    uid: &mut u32,
    declarations: &mut String,
) -> Result<(), MfdError> {
    let reachable = reachable_nodes(function)?;
    let mut components = String::new();
    let mut edges = Vec::new();
    let mut outputs = BTreeMap::new();
    let mut inputs: BTreeMap<NodeId, Vec<u32>> = BTreeMap::new();
    let mut parameter_outputs = BTreeMap::new();

    for parameter in &function.parameters {
        let port = interface
            .parameters
            .iter()
            .find(|port| port.id == parameter.id)
            .ok_or_else(|| unsupported("user-defined function parameter interface mismatch"))?;
        let output = keys.next();
        parameter_outputs.insert(parameter.id, output);
        let _ = write!(
            components,
            "\t\t\t\t<component name=\"{}\" library=\"core\" uid=\"{}\" kind=\"6\">\n\
             \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{output}\"/></targets>\n\
             \t\t\t\t\t<data><input datatype=\"{}\"/></data>\n\
             \t\t\t\t</component>\n",
            xml_escape(&parameter.name),
            port.component_id,
            scalar_type_name(parameter.ty)
        );
    }

    for &id in &reachable {
        let node = function.body.nodes.get(&id).ok_or_else(|| {
            unsupported(format!(
                "user-defined function `{}` references missing node {id}",
                function.name
            ))
        })?;
        match node {
            Node::FunctionParameter { parameter } => {
                if let Some(&output) = parameter_outputs.get(parameter) {
                    outputs.insert(id, output);
                }
            }
            Node::Unconnected => {
                let output = keys.next();
                outputs.insert(id, output);
                render_constant(&Value::Null, output, uid, &mut components, "\t\t\t\t");
            }
            Node::Const { value } => {
                let output = keys.next();
                outputs.insert(id, output);
                render_constant(value, output, uid, &mut components, "\t\t\t\t");
            }
            Node::Call { function, args } => {
                let pins = args.iter().map(|_| keys.next()).collect::<Vec<_>>();
                let output = keys.next();
                outputs.insert(id, output);
                inputs.insert(id, pins.clone());
                render_scalar_call(function, &pins, output, uid, &mut components, "\t\t\t\t");
            }
            Node::UserFunctionCall { function, .. } => {
                let callee = interfaces.get(function).ok_or_else(|| {
                    unsupported(format!("missing user-defined function {}", function.get()))
                })?;
                let (pins, output) =
                    render_call_component(callee, keys, uid, &mut components, "\t\t\t\t");
                outputs.insert(id, output);
                inputs.insert(id, pins);
            }
            Node::If { .. } => {
                let pins = (0..3).map(|_| keys.next()).collect::<Vec<_>>();
                let output = keys.next();
                outputs.insert(id, output);
                inputs.insert(id, pins.clone());
                render_if(&pins, output, uid, &mut components, "\t\t\t\t");
            }
            Node::ValueMap {
                input_type,
                table,
                default,
                ..
            } => {
                let input = keys.next();
                let output = keys.next();
                outputs.insert(id, output);
                inputs.insert(id, vec![input]);
                render_value_map(
                    *input_type,
                    table,
                    default,
                    input,
                    output,
                    uid,
                    &mut components,
                    "\t\t\t\t",
                );
            }
            _ => {
                return Err(unsupported(format!(
                    "user-defined function `{}` contains an unsupported node",
                    function.name
                )));
            }
        }
    }

    for (&id, pins) in &inputs {
        let node = function.body.nodes.get(&id).ok_or_else(|| {
            unsupported(format!("user-defined function body node {id} is missing"))
        })?;
        for (argument, input) in node.dependencies().into_iter().zip(pins) {
            let output = outputs.get(&argument).copied().ok_or_else(|| {
                unsupported(format!(
                    "user-defined function `{}` references unexported node {argument}",
                    function.name
                ))
            })?;
            edges.push((output, *input));
        }
    }
    let result_input = keys.next();
    let result_output = outputs.get(&function.output).copied().ok_or_else(|| {
        unsupported(format!(
            "user-defined function `{}` has no exported output node",
            function.name
        ))
    })?;
    edges.push((result_output, result_input));
    let _ = write!(
        components,
        "\t\t\t\t<component name=\"{}\" library=\"core\" uid=\"{}\" kind=\"7\">\n\
         \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{result_input}\"/></sources>\n\
         \t\t\t\t\t<data><output datatype=\"{}\"/></data>\n\
         \t\t\t\t</component>\n",
        xml_escape(&function.output_name),
        interface.output_component_id,
        scalar_type_name(function.output_type)
    );

    let _ = write!(
        declarations,
        "\t<component name=\"{}\" library=\"{}\">\n\
         \t\t<structure>\n\
         \t\t\t<children>\n{components}\t\t\t</children>\n\
         \t\t\t<graph directed=\"1\">\n\
         \t\t\t\t<vertices>\n",
        xml_escape(&function.name),
        xml_escape(&function.library)
    );
    render_edges(&edges, declarations, "\t\t\t\t\t");
    declarations
        .push_str("\t\t\t\t</vertices>\n\t\t\t</graph>\n\t\t</structure>\n\t</component>\n");
    Ok(())
}

fn render_call_component(
    interface: &Interface,
    keys: &mut KeyAlloc,
    uid: &mut u32,
    components: &mut String,
    indent: &str,
) -> (Vec<u32>, u32) {
    let inputs = interface
        .parameters
        .iter()
        .map(|_| keys.next())
        .collect::<Vec<_>>();
    let output = keys.next();
    *uid += 1;
    let mut input_entries = String::new();
    for (parameter, key) in interface.parameters.iter().zip(&inputs) {
        let _ = write!(
            input_entries,
            "<entry name=\"{}\" inpkey=\"{key}\" componentid=\"{}\"/>",
            xml_escape(&parameter.name),
            parameter.component_id
        );
    }
    let _ = write!(
        components,
        "{indent}<component name=\"{}\" library=\"{}\" uid=\"{uid}\" kind=\"19\">\n\
         {indent}\t<data><root>{input_entries}</root><root rootindex=\"1\"><entry name=\"{}\" outkey=\"{output}\" componentid=\"{}\"/></root></data>\n\
         {indent}\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
         {indent}</component>\n",
        xml_escape(&interface.name),
        xml_escape(&interface.library),
        xml_escape(&interface.output_name),
        interface.output_component_id
    );
    (inputs, output)
}

fn render_constant(
    value: &Value,
    output: u32,
    uid: &mut u32,
    components: &mut String,
    indent: &str,
) {
    *uid += 1;
    let (text, datatype) = constant_parts(value);
    let (name, kind) = match value {
        Value::Null => ("set-empty", 5),
        Value::XmlNil(_) => ("set-xsi-nil", 5),
        _ => ("constant", 2),
    };
    let data = matches!(
        value,
        Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::String(_)
    )
    .then(|| {
        format!(
            "{indent}\t<data><constant value=\"{}\" datatype=\"{datatype}\"/></data>\n",
            xml_escape(&text)
        )
    });
    let _ = write!(
        components,
        "{indent}<component name=\"{name}\" library=\"core\" uid=\"{uid}\" kind=\"{}\">\n\
         {indent}\t<targets><datapoint pos=\"0\" key=\"{output}\"/></targets>\n{}\
         {indent}</component>\n",
        kind,
        data.unwrap_or_default()
    );
}

fn render_scalar_call(
    function: &str,
    inputs: &[u32],
    output: u32,
    uid: &mut u32,
    components: &mut String,
    indent: &str,
) {
    *uid += 1;
    let mut pins = String::new();
    for (position, key) in inputs.iter().enumerate() {
        let _ = write!(pins, "<datapoint pos=\"{position}\" key=\"{key}\"/>");
    }
    let _ = write!(
        components,
        "{indent}<component name=\"{}\" library=\"{}\" uid=\"{uid}\" kind=\"5\">\n\
         {indent}\t<sources>{pins}</sources>\n\
         {indent}\t<targets><datapoint pos=\"0\" key=\"{output}\"/></targets>\n\
         {indent}</component>\n",
        xml_escape(&unmap_function_name(function)),
        function_library(function)
    );
}

fn render_if(inputs: &[u32], output: u32, uid: &mut u32, components: &mut String, indent: &str) {
    *uid += 1;
    let _ = write!(
        components,
        "{indent}<component name=\"if-else\" library=\"core\" uid=\"{uid}\" kind=\"4\">\n\
         {indent}\t<sources><datapoint pos=\"0\" key=\"{}\"/><datapoint pos=\"1\" key=\"{}\"/><datapoint pos=\"2\" key=\"{}\"/></sources>\n\
         {indent}\t<targets><datapoint pos=\"0\" key=\"{output}\"/></targets>\n\
         {indent}</component>\n",
        inputs[0], inputs[1], inputs[2]
    );
}

#[allow(clippy::too_many_arguments)]
fn render_value_map(
    input_type: Option<ir::ScalarType>,
    table: &[(Value, Value)],
    default: &Option<Value>,
    input: u32,
    output: u32,
    uid: &mut u32,
    components: &mut String,
    indent: &str,
) {
    *uid += 1;
    let rows = table
        .iter()
        .map(|(from, to)| {
            format!(
                "<entry from=\"{}\" to=\"{}\"/>",
                xml_escape(&value_text(from)),
                xml_escape(&value_text(to))
            )
        })
        .collect::<String>();
    let default_attr = default
        .as_ref()
        .map(|value| format!(" defaultValue=\"{}\"", xml_escape(&value_text(value))))
        .unwrap_or_default();
    let mode = if default.is_some() {
        " defaultValueMode=\"custom\""
    } else {
        ""
    };
    let result_type = table
        .iter()
        .find_map(|(_, value)| value_scalar_type(value))
        .or_else(|| default.as_ref().and_then(value_scalar_type))
        .map(scalar_type_name)
        .unwrap_or("string");
    let _ = write!(
        components,
        "{indent}<component name=\"value-map\" library=\"core\" uid=\"{uid}\" kind=\"23\">\n\
         {indent}\t<sources><datapoint pos=\"0\" key=\"{input}\"/></sources>\n\
         {indent}\t<targets><datapoint pos=\"0\" key=\"{output}\"/></targets>\n\
         {indent}\t<data><valuemap{mode}><valuemapTable>{rows}</valuemapTable><input name=\"input\" type=\"{}\"/><result name=\"result\" type=\"{result_type}\"{default_attr}/></valuemap></data>\n\
         {indent}</component>\n",
        scalar_type_name(input_type.unwrap_or(ir::ScalarType::String))
    );
}

fn render_edges(edges: &[(u32, u32)], output: &mut String, indent: &str) {
    let mut grouped: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for &(from, to) in edges {
        grouped.entry(from).or_default().push(to);
    }
    for (from, targets) in grouped {
        let _ = write!(output, "{indent}<vertex vertexkey=\"{from}\"><edges>");
        for target in targets {
            let _ = write!(output, "<edge vertexkey=\"{target}\"/>");
        }
        output.push_str("</edges></vertex>\n");
    }
}

fn unsupported(message: impl Into<String>) -> MfdError {
    MfdError::Unsupported(message.into())
}
