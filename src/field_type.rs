use serde::{Deserialize, Serialize};
use sekkei::Schema;

/// Platform-independent field type for code generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldType {
    String,
    Integer,
    Number,
    Boolean,
    Array(Box<FieldType>),
    Map(Box<FieldType>),
    Object(std::string::String),
    Enum {
        values: Vec<std::string::String>,
        underlying: Box<FieldType>,
    },
    Any,
}

/// Resolve a sekkei `Schema` to a `FieldType`.
#[must_use]
pub fn schema_to_field_type(schema: &Schema) -> FieldType {
    // Handle $ref — always takes precedence.
    if let Some(ref_path) = &schema.ref_path {
        let name = sekkei::ref_name(ref_path);
        return FieldType::Object(name.to_string());
    }

    let base_type = match schema.schema_type.as_deref() {
        Some("string") => FieldType::String,
        Some("integer") => FieldType::Integer,
        Some("number") => FieldType::Number,
        Some("boolean") => FieldType::Boolean,
        Some("array") => {
            let inner = schema
                .items
                .as_ref()
                .map(|s| schema_to_field_type(s))
                .unwrap_or(FieldType::Any);
            FieldType::Array(Box::new(inner))
        }
        Some("object") => {
            if let Some(additional) = &schema.additional_properties {
                let inner = schema_to_field_type(additional);
                FieldType::Map(Box::new(inner))
            } else if schema.properties.is_empty() {
                FieldType::Any
            } else {
                // Named inline object — use title if available.
                let name = schema
                    .title
                    .clone()
                    .unwrap_or_else(|| "InlineObject".to_string());
                FieldType::Object(name)
            }
        }
        _ => {
            // Check allOf/oneOf/anyOf for a $ref.
            if !schema.all_of.is_empty() {
                if let Some(first_ref) = schema.all_of.iter().find(|s| s.ref_path.is_some()) {
                    let name = sekkei::ref_name(first_ref.ref_path.as_deref().unwrap());
                    return FieldType::Object(name.to_string());
                }
            }
            FieldType::Any
        }
    };

    // Apply enum constraint if present.
    if let Some(values) = &schema.enum_values {
        if !values.is_empty() {
            let string_values: Vec<String> = values
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if !string_values.is_empty() {
                return FieldType::Enum {
                    values: string_values,
                    underlying: Box::new(base_type),
                };
            }
        }
    }

    base_type
}

#[cfg(test)]
mod tests {
    use super::*;
    use sekkei::Schema;

    fn string_schema() -> Schema {
        Schema {
            schema_type: Some("string".to_string()),
            ..Default::default()
        }
    }

    fn integer_schema() -> Schema {
        Schema {
            schema_type: Some("integer".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn basic_string() {
        assert_eq!(schema_to_field_type(&string_schema()), FieldType::String);
    }

    #[test]
    fn basic_integer() {
        assert_eq!(schema_to_field_type(&integer_schema()), FieldType::Integer);
    }

    #[test]
    fn basic_number() {
        let s = Schema {
            schema_type: Some("number".to_string()),
            ..Default::default()
        };
        assert_eq!(schema_to_field_type(&s), FieldType::Number);
    }

    #[test]
    fn basic_boolean() {
        let s = Schema {
            schema_type: Some("boolean".to_string()),
            ..Default::default()
        };
        assert_eq!(schema_to_field_type(&s), FieldType::Boolean);
    }

    #[test]
    fn array_of_strings() {
        let s = Schema {
            schema_type: Some("array".to_string()),
            items: Some(Box::new(string_schema())),
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Array(Box::new(FieldType::String))
        );
    }

    #[test]
    fn object_with_additional_properties() {
        let s = Schema {
            schema_type: Some("object".to_string()),
            additional_properties: Some(Box::new(string_schema())),
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Map(Box::new(FieldType::String))
        );
    }

    #[test]
    fn ref_schema() {
        let s = Schema {
            ref_path: Some("#/components/schemas/Pet".to_string()),
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Object("Pet".to_string())
        );
    }

    #[test]
    fn enum_schema() {
        let s = Schema {
            schema_type: Some("string".to_string()),
            enum_values: Some(vec![
                serde_json::Value::String("a".to_string()),
                serde_json::Value::String("b".to_string()),
            ]),
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Enum {
                values: vec!["a".to_string(), "b".to_string()],
                underlying: Box::new(FieldType::String),
            }
        );
    }

    #[test]
    fn unknown_type_is_any() {
        let s = Schema::default();
        assert_eq!(schema_to_field_type(&s), FieldType::Any);
    }

    #[test]
    fn empty_object_is_any() {
        let s = Schema {
            schema_type: Some("object".to_string()),
            ..Default::default()
        };
        assert_eq!(schema_to_field_type(&s), FieldType::Any);
    }

    #[test]
    fn nested_array() {
        let inner = Schema {
            schema_type: Some("array".to_string()),
            items: Some(Box::new(integer_schema())),
            ..Default::default()
        };
        let outer = Schema {
            schema_type: Some("array".to_string()),
            items: Some(Box::new(inner)),
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&outer),
            FieldType::Array(Box::new(FieldType::Array(Box::new(FieldType::Integer))))
        );
    }

    #[test]
    fn all_of_with_ref() {
        let s = Schema {
            all_of: vec![Schema {
                ref_path: Some("#/components/schemas/Base".to_string()),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Object("Base".to_string())
        );
    }

    #[test]
    fn object_with_properties_uses_title() {
        let mut props = std::collections::BTreeMap::new();
        props.insert("field".to_string(), string_schema());
        let s = Schema {
            schema_type: Some("object".to_string()),
            title: Some("MyType".to_string()),
            properties: props,
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Object("MyType".to_string())
        );
    }

    #[test]
    fn object_with_properties_no_title() {
        let mut props = std::collections::BTreeMap::new();
        props.insert("field".to_string(), string_schema());
        let s = Schema {
            schema_type: Some("object".to_string()),
            properties: props,
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Object("InlineObject".to_string())
        );
    }

    #[test]
    fn array_without_items_is_array_any() {
        let s = Schema {
            schema_type: Some("array".to_string()),
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Array(Box::new(FieldType::Any))
        );
    }
}
