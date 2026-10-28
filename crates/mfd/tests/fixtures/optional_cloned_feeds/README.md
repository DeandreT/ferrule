# Optional cloned feeds

`cases.json` covers absent optional integer elements. `nil_cases.json` instead
covers explicitly nilled nillable integers, with the same independently declared
target occurrences.

The `input_typed` and `expected_typed` objects are typed-host fixture notation:
their JSON `null` markers construct `Value::XmlNil`, not `Value::Null` or JSON
null. These objects are never submitted to JSON mapping APIs. `input_xml` is the
separate XML-boundary input. A missing property represents an absent source field.
