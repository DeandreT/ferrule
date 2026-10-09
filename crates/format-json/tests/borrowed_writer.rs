//! Literal writer controls shared by the original and borrowed write paths.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use format_json::{JsonFormatError, json_schema, to_string, to_value, write};
use ir::{Instance, SchemaNode, Value};
use serde::Deserialize;

// Complete expected bytes/error fields are independently authored literals.
// Neither writer supplies these expectations.
const CONTROLS: &str = r####"{
  "status": "MANUAL_LITERAL_CONTROLS_SOURCE_CORRECTED_AND_FROZEN_BEFORE_IMPLEMENTATION_ALL_EXECUTABLE_GATES_UNRUN",
  "issue": 184,
  "controls": [
    {
      "id": "closed-schema-order-escapes-and-extra",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "group",
          "children": [
            {
              "name": "Zulu",
              "kind": {
                "kind": "scalar",
                "ty": "string"
              }
            },
            {
              "name": "Alpha",
              "kind": {
                "kind": "scalar",
                "ty": "int"
              }
            }
          ]
        }
      },
      "input": {
        "Group": [
          [
            "Alpha",
            {
              "Scalar": "42"
            }
          ],
          [
            "Extra",
            {
              "Scalar": "ignored"
            }
          ],
          [
            "Zulu",
            {
              "Scalar": "é\t\"\\\n"
            }
          ]
        ]
      },
      "expected": {
        "bytes": "{\n  \"Zulu\": \"é\\t\\\"\\\\\\n\",\n  \"Alpha\": 42\n}\n"
      }
    },
    {
      "id": "nested-repeated-groups",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "group",
          "children": [
            {
              "name": "Rows",
              "kind": {
                "kind": "group",
                "children": [
                  {
                    "name": "Id",
                    "kind": {
                      "kind": "scalar",
                      "ty": "int"
                    }
                  },
                  {
                    "name": "Text",
                    "kind": {
                      "kind": "scalar",
                      "ty": "string"
                    }
                  }
                ]
              },
              "repeating": true
            }
          ]
        }
      },
      "input": {
        "Group": [
          [
            "Rows",
            {
              "Repeated": [
                {
                  "Group": [
                    [
                      "Text",
                      {
                        "Scalar": "first"
                      }
                    ],
                    [
                      "Id",
                      {
                        "Scalar": "1.000"
                      }
                    ]
                  ]
                },
                {
                  "Group": [
                    [
                      "Id",
                      {
                        "Scalar": 2
                      }
                    ],
                    [
                      "Text",
                      {
                        "Scalar": "😀"
                      }
                    ]
                  ]
                }
              ]
            }
          ]
        ]
      },
      "expected": {
        "bytes": "{\n  \"Rows\": [\n    {\n      \"Id\": 1,\n      \"Text\": \"first\"\n    },\n    {\n      \"Id\": 2,\n      \"Text\": \"😀\"\n    }\n  ]\n}\n"
      }
    },
    {
      "id": "root-flat-rows",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "group",
          "children": [
            {
              "name": "Text",
              "kind": {
                "kind": "scalar",
                "ty": "string"
              }
            }
          ]
        }
      },
      "input": {
        "Repeated": [
          {
            "Group": [
              [
                "Text",
                {
                  "Scalar": "a"
                }
              ]
            ]
          },
          {
            "Group": [
              [
                "Text",
                {
                  "Scalar": "b"
                }
              ]
            ]
          }
        ]
      },
      "expected": {
        "bytes": "[\n  {\n    \"Text\": \"a\"\n  },\n  {\n    \"Text\": \"b\"\n  }\n]\n"
      }
    },
    {
      "id": "mapped-singleton",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "string"
        }
      },
      "input": {
        "MappedSequence": [
          {
            "Scalar": "a"
          }
        ]
      },
      "expected": {
        "bytes": "\"a\"\n"
      }
    },
    {
      "id": "mapped-empty-root",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "string"
        }
      },
      "input": {
        "MappedSequence": []
      },
      "expected": {
        "error_debug": "Shape { name: \"Root\", expected: \"one mapped item\", got: \"mapped sequence\" }"
      }
    },
    {
      "id": "mapped-many-root",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "string"
        }
      },
      "input": {
        "MappedSequence": [
          {
            "Scalar": "a"
          },
          {
            "Scalar": "b"
          }
        ]
      },
      "expected": {
        "error_debug": "Shape { name: \"Root\", expected: \"one mapped item\", got: \"mapped sequence\" }"
      }
    },
    {
      "id": "mapped-repeating",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "int"
        },
        "repeating": true
      },
      "input": {
        "MappedSequence": [
          {
            "Scalar": "1.0"
          },
          {
            "Scalar": 2
          }
        ]
      },
      "expected": {
        "bytes": "[\n  1,\n  2\n]\n"
      }
    },
    {
      "id": "nullable-container",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "group",
          "children": []
        },
        "container_nullable": true
      },
      "input": {
        "Scalar": {
          "$json_null": true
        }
      },
      "expected": {
        "bytes": "null\n"
      }
    },
    {
      "id": "nullable-scalar",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "string"
        },
        "nullable": true
      },
      "input": {
        "Scalar": {
          "$json_null": true
        }
      },
      "expected": {
        "bytes": "null\n"
      }
    },
    {
      "id": "root-absence-is-not-null",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "string"
        }
      },
      "input": {
        "Scalar": null
      },
      "expected": {
        "error_debug": "Shape { name: \"Root\", expected: \"string\", got: \"null\" }"
      }
    },
    {
      "id": "group-absence-null-and-empty",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "group",
          "children": [
            {
              "name": "absent",
              "kind": {
                "kind": "scalar",
                "ty": "string"
              }
            },
            {
              "name": "nullable",
              "kind": {
                "kind": "scalar",
                "ty": "string"
              },
              "nullable": true
            },
            {
              "name": "empty",
              "kind": {
                "kind": "group",
                "children": []
              }
            },
            {
              "name": "nullGroup",
              "kind": {
                "kind": "group",
                "children": []
              },
              "container_nullable": true
            },
            {
              "name": "nullArray",
              "kind": {
                "kind": "scalar",
                "ty": "int"
              },
              "repeating": true,
              "container_nullable": true
            }
          ]
        }
      },
      "input": {
        "Group": [
          [
            "absent",
            {
              "Scalar": null
            }
          ],
          [
            "nullable",
            {
              "Scalar": {
                "$json_null": true
              }
            }
          ],
          [
            "empty",
            {
              "Group": []
            }
          ],
          [
            "nullGroup",
            {
              "Scalar": {
                "$json_null": true
              }
            }
          ],
          [
            "nullArray",
            {
              "Scalar": {
                "$json_null": true
              }
            }
          ]
        ]
      },
      "expected": {
        "bytes": "{\n  \"nullable\": null,\n  \"empty\": {},\n  \"nullGroup\": null,\n  \"nullArray\": null\n}\n"
      }
    },
    {
      "id": "string-from-bool",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "string"
        }
      },
      "input": {
        "Scalar": true
      },
      "expected": {
        "bytes": "\"true\"\n"
      }
    },
    {
      "id": "string-from-int",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "string"
        }
      },
      "input": {
        "Scalar": -42
      },
      "expected": {
        "bytes": "\"-42\"\n"
      }
    },
    {
      "id": "string-from-float",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "string"
        }
      },
      "input": {
        "Scalar": 2.5
      },
      "expected": {
        "bytes": "\"2.5\"\n"
      }
    },
    {
      "id": "exact-int-decimal",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "int"
        }
      },
      "input": {
        "Scalar": " 9007199254740993.000 "
      },
      "expected": {
        "bytes": "9007199254740993\n"
      }
    },
    {
      "id": "exact-int-min",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "int"
        }
      },
      "input": {
        "Scalar": "-9223372036854775808.000"
      },
      "expected": {
        "bytes": "-9223372036854775808\n"
      }
    },
    {
      "id": "float-from-exact-int",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "float"
        }
      },
      "input": {
        "Scalar": 42
      },
      "expected": {
        "bytes": "42\n"
      }
    },
    {
      "id": "float-from-string",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "float"
        }
      },
      "input": {
        "Scalar": " 2.5 "
      },
      "expected": {
        "bytes": "2.5\n"
      }
    },
    {
      "id": "float-negative-zero",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "float"
        }
      },
      "input": {
        "Scalar": 0
      },
      "expected": {
        "bytes": "-0.0\n"
      },
      "special_float": "negative-zero"
    },
    {
      "id": "bool-from-string",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "bool"
        }
      },
      "input": {
        "Scalar": " true "
      },
      "expected": {
        "bytes": "true\n"
      }
    },
    {
      "id": "union-retains-string",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar_union",
          "types": [
            "string",
            "int"
          ]
        },
        "numeric_range": {
          "kind": "integer",
          "bounds": {
            "minimum": 2,
            "maximum": 5
          }
        }
      },
      "input": {
        "Scalar": "100"
      },
      "expected": {
        "bytes": "\"100\"\n"
      }
    },
    {
      "id": "union-exact-int",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar_union",
          "types": [
            "string",
            "int"
          ]
        }
      },
      "input": {
        "Scalar": 5
      },
      "expected": {
        "bytes": "5\n"
      }
    },
    {
      "id": "union-float-fallback",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar_union",
          "types": [
            "string",
            "float"
          ]
        }
      },
      "input": {
        "Scalar": 42
      },
      "expected": {
        "bytes": "42\n"
      }
    },
    {
      "id": "union-decimal-selects-float",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar_union",
          "types": [
            "int",
            "float"
          ]
        }
      },
      "input": {
        "Scalar": "1.000"
      },
      "expected": {
        "bytes": "1.0\n"
      }
    },
    {
      "id": "union-ambiguous-string",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar_union",
          "types": [
            "int",
            "float"
          ]
        }
      },
      "input": {
        "Scalar": "1"
      },
      "expected": {
        "error_debug": "Shape { name: \"Root\", expected: \"unambiguous declared scalar union\", got: \"string\" }"
      }
    },
    {
      "id": "union-no-string-candidate",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar_union",
          "types": [
            "int",
            "bool"
          ]
        }
      },
      "input": {
        "Scalar": "no"
      },
      "expected": {
        "error_debug": "Shape { name: \"Root\", expected: \"declared scalar union\", got: \"string\" }"
      }
    },
    {
      "id": "union-bool-string-candidate",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar_union",
          "types": [
            "int",
            "bool"
          ]
        }
      },
      "input": {
        "Scalar": " true "
      },
      "expected": {
        "bytes": "true\n"
      }
    },
    {
      "id": "dynamic-instance-order-json-and-fallback",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "group",
          "children": [
            {
              "name": "known",
              "kind": {
                "kind": "scalar",
                "ty": "int"
              }
            }
          ],
          "dynamic": {
            "name": "*",
            "kind": {
              "kind": "scalar",
              "ty": "string"
            },
            "json_any": true
          }
        }
      },
      "input": {
        "Group": [
          [
            "z",
            {
              "Scalar": "{\"b\":2,\"a\":[true,null]}"
            }
          ],
          [
            "known",
            {
              "Scalar": "3"
            }
          ],
          [
            "a",
            {
              "Scalar": "unparsed"
            }
          ]
        ]
      },
      "expected": {
        "bytes": "{\n  \"z\": {\n    \"b\": 2,\n    \"a\": [\n      true,\n      null\n    ]\n  },\n  \"known\": 3,\n  \"a\": \"unparsed\"\n}\n"
      }
    },
    {
      "id": "dynamic-duplicate-after-emission",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "group",
          "children": [
            {
              "name": "known",
              "kind": {
                "kind": "scalar",
                "ty": "int"
              }
            }
          ],
          "dynamic": {
            "name": "*",
            "kind": {
              "kind": "scalar",
              "ty": "string"
            },
            "json_any": true
          }
        }
      },
      "input": {
        "Group": [
          [
            "a",
            {
              "Scalar": "x"
            }
          ],
          [
            "a",
            {
              "Scalar": "y"
            }
          ]
        ]
      },
      "expected": {
        "error_debug": "DuplicateProperty { object: \"Root\", property: \"a\" }"
      }
    },
    {
      "id": "dynamic-absent-first-is-not-duplicate",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "group",
          "children": [
            {
              "name": "known",
              "kind": {
                "kind": "scalar",
                "ty": "int"
              }
            }
          ],
          "dynamic": {
            "name": "*",
            "kind": {
              "kind": "scalar",
              "ty": "string"
            },
            "json_any": true
          }
        }
      },
      "input": {
        "Group": [
          [
            "known",
            {
              "Scalar": null
            }
          ],
          [
            "known",
            {
              "Scalar": "4"
            }
          ]
        ]
      },
      "expected": {
        "bytes": "{\n  \"known\": 4\n}\n"
      }
    },
    {
      "id": "duplicate-schema-replaces-slot",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "group",
          "children": [
            {
              "name": "x",
              "kind": {
                "kind": "scalar",
                "ty": "string"
              }
            },
            {
              "name": "a",
              "kind": {
                "kind": "scalar",
                "ty": "int"
              }
            },
            {
              "name": "x",
              "kind": {
                "kind": "scalar",
                "ty": "int"
              }
            }
          ]
        }
      },
      "input": {
        "Group": [
          [
            "x",
            {
              "Scalar": "42"
            }
          ],
          [
            "x",
            {
              "Scalar": "ignored"
            }
          ],
          [
            "a",
            {
              "Scalar": 1
            }
          ]
        ]
      },
      "expected": {
        "bytes": "{\n  \"x\": 42,\n  \"a\": 1\n}\n"
      }
    },
    {
      "id": "schema-error-order-before-required",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "group",
          "children": [
            {
              "name": "z",
              "kind": {
                "kind": "scalar",
                "ty": "int"
              }
            },
            {
              "name": "a",
              "kind": {
                "kind": "scalar",
                "ty": "int"
              }
            }
          ],
          "required": [
            "missing"
          ]
        }
      },
      "input": {
        "Group": [
          [
            "a",
            {
              "Scalar": "bad-a"
            }
          ],
          [
            "z",
            {
              "Scalar": "bad-z"
            }
          ]
        ]
      },
      "expected": {
        "error_debug": "Shape { name: \"z\", expected: \"integer\", got: \"string\" }"
      }
    },
    {
      "id": "missing-required-after-omission",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "group",
          "children": [
            {
              "name": "x",
              "kind": {
                "kind": "scalar",
                "ty": "int"
              }
            }
          ],
          "required": [
            "x"
          ]
        }
      },
      "input": {
        "Group": [
          [
            "x",
            {
              "Scalar": null
            }
          ]
        ]
      },
      "expected": {
        "error_debug": "MissingRequiredProperty { object: \"Root\", property: \"x\" }"
      }
    },
    {
      "id": "integer-fraction-refused",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "int"
        }
      },
      "input": {
        "Scalar": "1.001"
      },
      "expected": {
        "error_debug": "Shape { name: \"Root\", expected: \"integer\", got: \"string\" }"
      }
    },
    {
      "id": "float-inexact-int-refused",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "float"
        }
      },
      "input": {
        "Scalar": 9007199254740992
      },
      "expected": {
        "error_debug": "Shape { name: \"Root\", expected: \"number\", got: \"int outside the exact f64 range\" }"
      },
      "exact_i64": "9007199254740993"
    },
    {
      "id": "non-finite-nan",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "string"
        }
      },
      "input": {
        "Scalar": 0
      },
      "expected": {
        "error_debug": "Shape { name: \"Root\", expected: \"finite number\", got: \"non-finite float\" }"
      },
      "special_float": "nan"
    },
    {
      "id": "non-finite-positive-infinity",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "string"
        }
      },
      "input": {
        "Scalar": 0
      },
      "expected": {
        "error_debug": "Shape { name: \"Root\", expected: \"finite number\", got: \"non-finite float\" }"
      },
      "special_float": "positive-infinity"
    },
    {
      "id": "non-finite-negative-infinity",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "string"
        }
      },
      "input": {
        "Scalar": 0
      },
      "expected": {
        "error_debug": "Shape { name: \"Root\", expected: \"finite number\", got: \"non-finite float\" }"
      },
      "special_float": "negative-infinity"
    },
    {
      "id": "group-shape",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "group",
          "children": []
        }
      },
      "input": {
        "Scalar": "x"
      },
      "expected": {
        "error_debug": "Shape { name: \"Root\", expected: \"object\", got: \"string\" }"
      }
    },
    {
      "id": "array-shape",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "int"
        },
        "repeating": true
      },
      "input": {
        "Scalar": 1
      },
      "expected": {
        "error_debug": "Shape { name: \"Root\", expected: \"array\", got: \"int\" }"
      }
    },
    {
      "id": "recursive-occurrence-name",
      "schema_ir": {
        "name": "Node",
        "kind": {
          "kind": "group",
          "children": [
            {
              "name": "Text",
              "kind": {
                "kind": "scalar",
                "ty": "string"
              }
            },
            {
              "name": "next",
              "recursive_ref": "Node",
              "kind": {
                "kind": "group",
                "children": []
              }
            }
          ]
        }
      },
      "input": {
        "Group": [
          [
            "Text",
            {
              "Scalar": "a"
            }
          ],
          [
            "next",
            {
              "Group": [
                [
                  "Text",
                  {
                    "Scalar": "b"
                  }
                ]
              ]
            }
          ]
        ]
      },
      "expected": {
        "bytes": "{\n  \"Text\": \"a\",\n  \"next\": {\n    \"Text\": \"b\"\n  }\n}\n"
      }
    },
    {
      "id": "missing-recursive-anchor",
      "schema_ir": {
        "name": "Root",
        "recursive_ref": "Missing",
        "kind": {
          "kind": "group",
          "children": []
        }
      },
      "input": {
        "Group": []
      },
      "expected": {
        "error_debug": "MissingRecursiveAnchor { node: \"Root\", anchor: \"Missing\" }"
      }
    },
    {
      "id": "constant-before-enum",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "string"
        },
        "fixed": "a",
        "json_allowed_values": [
          {
            "type": "string",
            "value": "a"
          },
          {
            "type": "string",
            "value": "c"
          }
        ]
      },
      "input": {
        "Scalar": "b"
      },
      "expected": {
        "error_debug": "ConstantMismatch { name: \"Root\", expected: \"\\\"a\\\"\", got: \"\\\"b\\\"\" }"
      }
    },
    {
      "id": "enum-string-refusal",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar",
          "ty": "string"
        },
        "json_allowed_values": [
          {
            "type": "string",
            "value": "a"
          },
          {
            "type": "string",
            "value": "c"
          }
        ]
      },
      "input": {
        "Scalar": "b"
      },
      "expected": {
        "error_debug": "AllowedValueMismatch { name: \"Root\", got: \"String(\\\"b\\\")\" }"
      }
    },
    {
      "id": "union-range-refusal",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "scalar_union",
          "types": [
            "string",
            "int"
          ]
        },
        "numeric_range": {
          "kind": "integer",
          "bounds": {
            "minimum": 2,
            "maximum": 5
          }
        }
      },
      "input": {
        "Scalar": 6
      },
      "expected": {
        "error_debug": "RangeMismatch { name: \"Root\", range: \"[2, 5]\", got: \"6\" }"
      }
    },
    {
      "id": "multiple-coercion-accepted",
      "schema_json": {
        "title": "Root",
        "type": "integer",
        "minimum": 2,
        "maximum": 8,
        "multipleOf": 2
      },
      "input": {
        "Scalar": "4.000"
      },
      "expected": {
        "bytes": "4\n"
      }
    },
    {
      "id": "multiple-refusal",
      "schema_json": {
        "title": "Root",
        "type": "integer",
        "multipleOf": 2
      },
      "input": {
        "Scalar": "3.000"
      },
      "expected": {
        "error_debug": "MultipleOfMismatch { name: \"Root\", divisors: \"2\", got: \"3\" }"
      }
    },
    {
      "id": "string-pattern-and-length-accepted",
      "schema_json": {
        "title": "Root",
        "type": "string",
        "minLength": 2,
        "maxLength": 3,
        "pattern": "^x+$"
      },
      "input": {
        "Scalar": "xx"
      },
      "expected": {
        "bytes": "\"xx\"\n"
      }
    },
    {
      "id": "string-length-before-pattern",
      "schema_json": {
        "title": "Root",
        "type": "string",
        "minLength": 2,
        "pattern": "^x+$"
      },
      "input": {
        "Scalar": "y"
      },
      "expected": {
        "error_debug": "StringLengthMismatch { name: \"Root\", range: \"at least 2 Unicode scalar values\", got: 1 }"
      }
    },
    {
      "id": "string-pattern-refusal",
      "schema_json": {
        "title": "Root",
        "type": "string",
        "pattern": "^x+$"
      },
      "input": {
        "Scalar": "y"
      },
      "expected": {
        "error_debug": "PatternMismatch { name: \"Root\" }"
      }
    },
    {
      "id": "contains-unique-accepted",
      "schema_json": {
        "title": "Root",
        "type": "array",
        "items": {
          "type": "integer"
        },
        "contains": {
          "const": 2
        },
        "minContains": 1,
        "uniqueItems": true
      },
      "input": {
        "Repeated": [
          {
            "Scalar": "1.0"
          },
          {
            "Scalar": "2.0"
          }
        ]
      },
      "expected": {
        "bytes": "[\n  1,\n  2\n]\n"
      }
    },
    {
      "id": "contains-before-unique",
      "schema_json": {
        "title": "Root",
        "type": "array",
        "items": {
          "type": "integer"
        },
        "contains": {
          "const": 2
        },
        "uniqueItems": true
      },
      "input": {
        "Repeated": [
          {
            "Scalar": "1.0"
          },
          {
            "Scalar": 1
          }
        ]
      },
      "expected": {
        "error_debug": "ContainsCountMismatch { name: \"Root\", range: \"at least 1\", got: 0 }"
      }
    },
    {
      "id": "unique-normalized-refusal",
      "schema_json": {
        "title": "Root",
        "type": "array",
        "items": {
          "type": "integer"
        },
        "uniqueItems": true
      },
      "input": {
        "Repeated": [
          {
            "Scalar": "1.0"
          },
          {
            "Scalar": 1
          }
        ]
      },
      "expected": {
        "error_debug": "UniqueItemsMismatch { name: \"Root\", first_index: 1, duplicate_index: 2 }"
      }
    },
    {
      "id": "item-count-before-bad-item",
      "schema_json": {
        "title": "Root",
        "type": "array",
        "items": {
          "type": "integer"
        },
        "maxItems": 1
      },
      "input": {
        "Repeated": [
          {
            "Scalar": "bad"
          },
          {
            "Scalar": 1
          }
        ]
      },
      "expected": {
        "error_debug": "ItemCountMismatch { name: \"Root\", range: \"at most 1\", got: 2 }"
      }
    },
    {
      "id": "property-name-refusal",
      "schema_json": {
        "title": "Root",
        "type": "object",
        "properties": {
          "bad": {
            "type": "string"
          }
        },
        "propertyNames": {
          "pattern": "^good$"
        },
        "additionalProperties": false
      },
      "input": {
        "Group": [
          [
            "bad",
            {
              "Scalar": "x"
            }
          ]
        ]
      },
      "expected": {
        "error_debug": "InvalidPropertyName { object: \"Root\", property: \"bad\" }"
      }
    },
    {
      "id": "dependent-property-refusal",
      "schema_json": {
        "title": "Root",
        "type": "object",
        "properties": {
          "a": {
            "type": "integer"
          },
          "b": {
            "type": "integer"
          }
        },
        "dependentRequired": {
          "a": [
            "b"
          ]
        },
        "additionalProperties": false
      },
      "input": {
        "Group": [
          [
            "a",
            {
              "Scalar": 1
            }
          ]
        ]
      },
      "expected": {
        "error_debug": "MissingDependentProperty { object: \"Root\", trigger: \"a\", property: \"b\" }"
      }
    },
    {
      "id": "dependent-schema-accepted",
      "schema_json": {
        "title": "Root",
        "type": "object",
        "properties": {
          "a": {
            "type": "integer"
          },
          "b": {
            "type": "integer"
          }
        },
        "dependentSchemas": {
          "a": {
            "properties": {
              "b": {
                "const": 2
              }
            },
            "required": [
              "b"
            ]
          }
        },
        "additionalProperties": false
      },
      "input": {
        "Group": [
          [
            "b",
            {
              "Scalar": "2.0"
            }
          ],
          [
            "a",
            {
              "Scalar": 1
            }
          ]
        ]
      },
      "expected": {
        "bytes": "{\n  \"a\": 1,\n  \"b\": 2\n}\n"
      }
    },
    {
      "id": "dependent-schema-refusal",
      "schema_json": {
        "title": "Root",
        "type": "object",
        "properties": {
          "a": {
            "type": "integer"
          },
          "b": {
            "type": "integer"
          }
        },
        "dependentSchemas": {
          "a": {
            "properties": {
              "b": {
                "const": 2
              }
            },
            "required": [
              "b"
            ]
          }
        },
        "additionalProperties": false
      },
      "input": {
        "Group": [
          [
            "a",
            {
              "Scalar": 1
            }
          ],
          [
            "b",
            {
              "Scalar": 3
            }
          ]
        ]
      },
      "expected": {
        "error_debug": "DependentSchemaMismatch { object: \"Root\", trigger: \"a\" }"
      }
    },
    {
      "id": "property-count-refusal",
      "schema_json": {
        "title": "Root",
        "type": "object",
        "properties": {
          "a": {
            "type": "integer"
          },
          "b": {
            "type": "integer"
          }
        },
        "maxProperties": 1,
        "additionalProperties": false
      },
      "input": {
        "Group": [
          [
            "a",
            {
              "Scalar": 1
            }
          ],
          [
            "b",
            {
              "Scalar": 2
            }
          ]
        ]
      },
      "expected": {
        "error_debug": "PropertyCountMismatch { name: \"Root\", range: \"at most 1\", got: 2 }"
      }
    },
    {
      "id": "closed-alternative-accepted",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "group",
          "children": [
            {
              "name": "tag",
              "kind": {
                "kind": "scalar",
                "ty": "string"
              }
            },
            {
              "name": "value",
              "kind": {
                "kind": "scalar",
                "ty": "int"
              }
            }
          ],
          "alternatives": [
            {
              "name": "A",
              "members": [
                "tag",
                "value"
              ],
              "required": [
                "tag"
              ],
              "constraints": [
                {
                  "member": "tag",
                  "value": {
                    "type": "string",
                    "value": "a"
                  }
                }
              ]
            },
            {
              "name": "B",
              "members": [
                "tag"
              ],
              "required": [
                "tag"
              ],
              "constraints": [
                {
                  "member": "tag",
                  "value": {
                    "type": "string",
                    "value": "b"
                  }
                }
              ]
            }
          ]
        }
      },
      "input": {
        "Group": [
          [
            "value",
            {
              "Scalar": "2.0"
            }
          ],
          [
            "tag",
            {
              "Scalar": "a"
            }
          ]
        ]
      },
      "expected": {
        "bytes": "{\n  \"tag\": \"a\",\n  \"value\": 2\n}\n"
      }
    },
    {
      "id": "closed-alternative-refused",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "group",
          "children": [
            {
              "name": "tag",
              "kind": {
                "kind": "scalar",
                "ty": "string"
              }
            },
            {
              "name": "value",
              "kind": {
                "kind": "scalar",
                "ty": "int"
              }
            }
          ],
          "alternatives": [
            {
              "name": "A",
              "members": [
                "tag",
                "value"
              ],
              "required": [
                "tag"
              ],
              "constraints": [
                {
                  "member": "tag",
                  "value": {
                    "type": "string",
                    "value": "a"
                  }
                }
              ]
            },
            {
              "name": "B",
              "members": [
                "tag"
              ],
              "required": [
                "tag"
              ],
              "constraints": [
                {
                  "member": "tag",
                  "value": {
                    "type": "string",
                    "value": "b"
                  }
                }
              ]
            }
          ]
        }
      },
      "input": {
        "Group": [
          [
            "tag",
            {
              "Scalar": "b"
            }
          ],
          [
            "value",
            {
              "Scalar": 2
            }
          ]
        ]
      },
      "expected": {
        "error_debug": "NoMatchingAlternative { name: \"Root\" }"
      }
    },
    {
      "id": "closed-alternative-ambiguous",
      "schema_ir": {
        "name": "Root",
        "kind": {
          "kind": "group",
          "children": [
            {
              "name": "tag",
              "kind": {
                "kind": "scalar",
                "ty": "string"
              }
            }
          ],
          "alternatives": [
            {
              "name": "A",
              "members": [
                "tag"
              ],
              "required": [
                "tag"
              ]
            },
            {
              "name": "B",
              "members": [
                "tag"
              ],
              "required": [
                "tag"
              ]
            }
          ]
        }
      },
      "input": {
        "Group": [
          [
            "tag",
            {
              "Scalar": "a"
            }
          ]
        ]
      },
      "expected": {
        "error_debug": "AmbiguousAlternative { name: \"Root\" }"
      }
    }
  ],
  "notes": [
    "Expected bytes and complete error Debug fields are literal source-derived controls, not application outputs.",
    "Negative zero, non-finite float and exact i64 overrides construct the named typed value explicitly, independently of JSON wire numeric inference.",
    "Every to_string and to_value actual will be retained before comparisons; filesystem rejection uses a separate sentinel control.",
    "No fixture generator, benchmark, parser, compiler or runtime was executed while authoring these literals."
  ],
  "source_corrections_before_implementation": [
    {
      "id": "constant-before-enum",
      "witness": "crates/format-json/src/json_schema/constraints.rs:173-182",
      "correction": "expected and got retain JSON scalar quotation through Value::to_string"
    },
    {
      "id": "union-exact-int",
      "correction": "literal Int(5) now supplies the named positive Int tag control; the preceding control separately preserves String tag"
    }
  ]
}
"####;

#[derive(Deserialize)]
struct Catalog {
    controls: Vec<Control>,
}

#[derive(Deserialize)]
struct Control {
    id: String,
    schema_ir: Option<serde_json::Value>,
    schema_json: Option<serde_json::Value>,
    input: Instance,
    exact_i64: Option<String>,
    special_float: Option<String>,
    expected: Expected,
}

#[derive(Deserialize)]
struct Expected {
    bytes: Option<String>,
    error_debug: Option<String>,
}

fn retain(directory: Option<&Path>, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(directory) = directory {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(name))?;
        file.write_all(bytes)?;
        file.flush()
    } else {
        eprintln!("{name}: {}", String::from_utf8_lossy(bytes));
        Ok(())
    }
}

fn retain_debug(
    directory: Option<&Path>,
    name: &str,
    value: &impl std::fmt::Debug,
) -> std::io::Result<()> {
    retain(directory, name, format!("{value:#?}\n").as_bytes())
}

fn origin_text(instance: &Instance) -> String {
    fn visit(value: &Instance, path: &str, text: &mut String) {
        use std::fmt::Write as _;
        match value {
            Instance::Group(fields) => {
                writeln!(text, "path={path:?};origin={:?}", fields.xml_type_origin()).unwrap();
                for (index, (name, child)) in fields.iter().enumerate() {
                    visit(child, &format!("{path}/field[{index}]={name:?}"), text);
                }
            }
            Instance::Repeated(items) | Instance::MappedSequence(items) => {
                let kind = if matches!(value, Instance::Repeated(_)) {
                    "repeated"
                } else {
                    "mapped"
                };
                for (index, child) in items.iter().enumerate() {
                    visit(child, &format!("{path}/{kind}[{index}]"), text);
                }
            }
            Instance::DocumentSet(documents) => {
                for (index, document) in documents.iter().enumerate() {
                    visit(
                        document.value(),
                        &format!("{path}/document[{index}]={:?}", document.path()),
                        text,
                    );
                }
            }
            Instance::Scalar(_) => {}
        }
    }
    let mut text = String::new();
    visit(instance, "root", &mut text);
    text
}

fn prepared_input(control: &Control) -> Instance {
    if let Some(value) = &control.exact_i64 {
        return Instance::Scalar(Value::Int(value.parse().unwrap()));
    }
    if let Some(value) = &control.special_float {
        return Instance::Scalar(Value::Float(match value.as_str() {
            "negative-zero" => -0.0,
            "nan" => f64::NAN,
            "positive-infinity" => f64::INFINITY,
            "negative-infinity" => f64::NEG_INFINITY,
            _ => panic!("unknown finite control float"),
        }));
    }
    control.input.clone()
}

fn prepared_schema(control: &Control) -> Result<SchemaNode, String> {
    if control.id == "schema-error-order-before-required" {
        // This literal deliberately gives an undeclared required name so the
        // writer's earlier child error can be compared. SchemaNode's guarded
        // deserializer refuses that metadata before the writer is reached.
        // Admit only the exact frozen literal and construct its typed IR here.
        let literal = serde_json::json!({
            "name": "Root",
            "kind": {
                "kind": "group",
                "children": [
                    {"name": "z", "kind": {"kind": "scalar", "ty": "int"}},
                    {"name": "a", "kind": {"kind": "scalar", "ty": "int"}}
                ],
                "required": ["missing"]
            }
        });
        if control.schema_ir.as_ref() != Some(&literal) {
            return Err("the one explicit invalid-metadata control changed".to_owned());
        }
        let mut schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("z", ir::ScalarType::Int),
                SchemaNode::scalar("a", ir::ScalarType::Int),
            ],
        );
        if let ir::SchemaKind::Group { required, .. } = &mut schema.kind {
            required.push("missing".to_owned());
        }
        return Ok(schema);
    }
    if control.id == "constant-before-enum" {
        // This second precedence control deliberately combines fixed and
        // enum metadata, which guarded SchemaNode serde rejects. Preserve
        // its exact raw literal and construct only that combination safely.
        let literal = serde_json::json!({
            "name": "Root",
            "kind": {"kind": "scalar", "ty": "string"},
            "fixed": "a",
            "json_allowed_values": [
                {"type": "string", "value": "a"},
                {"type": "string", "value": "c"}
            ]
        });
        if control.schema_ir.as_ref() != Some(&literal) {
            return Err("the fixed-before-enum precedence control changed".to_owned());
        }
        let mut schema = SchemaNode::scalar_fixed("Root", ir::ScalarType::String, "a");
        schema.json_allowed_values = Some(
            ir::JsonAllowedValues::new([
                ir::JsonAllowedValue::String("a".to_owned()),
                ir::JsonAllowedValue::String("c".to_owned()),
            ])
            .map_err(|error| format!("{error:?}"))?,
        );
        return Ok(schema);
    }
    if let Some(schema) = &control.schema_ir {
        serde_json::from_value(schema.clone()).map_err(|error| format!("{error:?}"))
    } else if let Some(schema) = &control.schema_json {
        let source = serde_json::to_string(schema).map_err(|error| format!("{error:?}"))?;
        json_schema::import_str(&source).map_err(|error| format!("{error:?}"))
    } else {
        Err("control has no schema".to_owned())
    }
}

fn check_text(expected: &Expected, actual: &Result<String, JsonFormatError>) -> bool {
    match (&expected.bytes, &expected.error_debug, actual) {
        (Some(expected), None, Ok(actual)) => actual.as_bytes() == expected.as_bytes(),
        (None, Some(expected), Err(actual)) => format!("{actual:?}") == *expected,
        _ => false,
    }
}

fn check_value(expected: &Expected, actual: &Result<serde_json::Value, JsonFormatError>) -> bool {
    match (&expected.bytes, &expected.error_debug, actual) {
        (Some(expected), None, Ok(actual)) => {
            let expected: serde_json::Value = serde_json::from_str(expected).unwrap();
            actual == &expected
        }
        (None, Some(expected), Err(actual)) => format!("{actual:?}") == *expected,
        _ => false,
    }
}

#[test]
fn ordinary_writer_retains_literal_bytes_errors_and_input_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let evidence = std::env::var_os("FERRULE_JSON_WRITER184_EVIDENCE").map(PathBuf::from);
    if let Some(directory) = &evidence {
        fs::create_dir(directory)?;
    }
    let evidence = evidence.as_deref();
    retain(
        evidence,
        "manual-controls.original.json",
        CONTROLS.as_bytes(),
    )?;
    let catalog: Catalog = serde_json::from_str(CONTROLS)?;
    let mut prepared = Vec::new();
    for control in catalog.controls {
        let schema = prepared_schema(&control);
        retain_debug(
            evidence,
            &format!("{}.schema-source.original.txt", control.id),
            &(&control.schema_ir, &control.schema_json),
        )?;
        retain_debug(
            evidence,
            &format!("{}.schema.original.txt", control.id),
            &schema,
        )?;
        prepared.push((control, schema));
    }
    // Every schema-preparation outcome is retained before any public writer
    // call or expected comparison, so one bad setup cannot hide later setups.
    let mut originals = Vec::new();
    let mut failures = Vec::new();
    let mut public_calls = 0usize;
    for (control, schema) in prepared {
        let schema = match schema {
            Ok(schema) => schema,
            Err(_) => {
                failures.push(format!("{}: schema preparation", control.id));
                continue;
            }
        };
        let input = prepared_input(&control);
        let before = input.clone();
        let before_origins = origin_text(&input);
        retain_debug(
            evidence,
            &format!("{}.input.original.txt", control.id),
            &input,
        )?;
        retain(
            evidence,
            &format!("{}.input-origins.original.txt", control.id),
            before_origins.as_bytes(),
        )?;
        let text = to_string(&schema, &input);
        let value = to_value(&schema, &input);
        public_calls += 2;
        retain_debug(
            evidence,
            &format!("{}.text-result.original.txt", control.id),
            &text,
        )?;
        if let Ok(text) = &text {
            retain(
                evidence,
                &format!("{}.text-bytes.original.json", control.id),
                text.as_bytes(),
            )?;
        }
        retain_debug(
            evidence,
            &format!("{}.value-result.original.txt", control.id),
            &value,
        )?;
        if let Ok(value) = &value {
            retain(
                evidence,
                &format!("{}.value-bytes.original.json", control.id),
                &serde_json::to_vec(value)?,
            )?;
        }
        retain_debug(
            evidence,
            &format!("{}.input-after.original.txt", control.id),
            &input,
        )?;
        let after_origins = origin_text(&input);
        retain(
            evidence,
            &format!("{}.input-after-origins.original.txt", control.id),
            after_origins.as_bytes(),
        )?;
        // All public outcomes and complete input/origin originals precede
        // every expected comparison, including outcomes of later controls.
        originals.push((
            control,
            before,
            input,
            before_origins,
            after_origins,
            text,
            value,
        ));
    }
    publication_rejection(evidence, &mut failures)?;
    public_calls += 2;
    retain_debug(evidence, "public-call-count.original.txt", &public_calls)?;
    for (control, before, input, before_origins, after_origins, text, value) in originals {
        if !check_text(&control.expected, &text) {
            failures.push(format!("{}: complete text/error fields", control.id));
        }
        if !check_value(&control.expected, &value) {
            failures.push(format!("{}: normalized value/error fields", control.id));
        }
        // NaN is intentionally not equal to itself; its complete Debug and
        // both origin records are still retained. Other input identity is exact.
        if control.special_float.as_deref() != Some("nan") && before != input {
            failures.push(format!("{}: input identity", control.id));
        }
        if before_origins != after_origins {
            failures.push(format!("{}: input origins", control.id));
        }
    }
    retain(
        evidence,
        "comparisons.original.json",
        &serde_json::to_vec(&failures)?,
    )?;
    assert!(failures.is_empty(), "{failures:#?}");
    Ok(())
}

fn publication_rejection(
    evidence: Option<&Path>,
    failures: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let owned_directory = if let Some(directory) = evidence {
        directory.join("publication-control")
    } else {
        std::env::temp_dir().join(format!(
            "ferrule-json-writer184-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
        ))
    };
    fs::create_dir(&owned_directory)?;
    let existing = owned_directory.join("existing.json");
    let missing = owned_directory.join("missing.json");
    const SENTINEL: &[u8] = b"unchanged sentinel\n";
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&existing)?;
    file.write_all(SENTINEL)?;
    file.flush()?;
    drop(file);
    let schema = SchemaNode::scalar("Root", ir::ScalarType::Int);
    let input = Instance::Scalar(Value::String("bad".to_owned()));
    let first = write(&existing, &schema, &input);
    let second = write(&missing, &schema, &input);
    let existing_bytes = fs::read(&existing)?;
    let missing_exists = missing.try_exists()?;
    retain_debug(evidence, "publication-existing-result.original.txt", &first)?;
    retain_debug(evidence, "publication-missing-result.original.txt", &second)?;
    retain(
        evidence,
        "publication-existing-bytes.original.txt",
        &existing_bytes,
    )?;
    retain_debug(
        evidence,
        "publication-missing-exists.original.txt",
        &missing_exists,
    )?;
    let expected = "Shape { name: \"Root\", expected: \"integer\", got: \"string\" }";
    for (name, actual) in [("existing", first), ("missing", second)] {
        if !matches!(actual, Err(ref error) if format!("{error:?}") == expected) {
            failures.push(format!("publication {name}: exact pre-write rejection"));
        }
    }
    if existing_bytes != SENTINEL || missing_exists {
        failures.push("publication changed a destination after rejection".to_owned());
    }
    if evidence.is_none() && failures.is_empty() {
        fs::remove_file(&existing)?;
        fs::remove_dir(&owned_directory)?;
    }
    Ok(())
}
