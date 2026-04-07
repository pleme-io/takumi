use std::fmt;

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

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String => f.write_str("String"),
            Self::Integer => f.write_str("Integer"),
            Self::Number => f.write_str("Number"),
            Self::Boolean => f.write_str("Boolean"),
            Self::Array(inner) => write!(f, "Array<{inner}>"),
            Self::Map(inner) => write!(f, "Map<String, {inner}>"),
            Self::Object(name) => write!(f, "{name}"),
            Self::Enum { values, .. } => write!(f, "Enum({})", values.join("|")),
            Self::Any => f.write_str("Any"),
        }
    }
}

impl FieldType {
    /// Check if this is a primitive type.
    #[must_use]
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            Self::String | Self::Integer | Self::Number | Self::Boolean
        )
    }

    /// Check if this is a collection type (Array or Map).
    #[must_use]
    pub fn is_collection(&self) -> bool {
        matches!(self, Self::Array(_) | Self::Map(_))
    }

    /// Get the inner type for Array or Map.
    #[must_use]
    pub fn inner_type(&self) -> Option<&Self> {
        match self {
            Self::Array(inner) | Self::Map(inner) => Some(inner),
            _ => None,
        }
    }

    /// Get enum values if this is an Enum type.
    #[must_use]
    pub fn enum_values(&self) -> Option<&[std::string::String]> {
        match self {
            Self::Enum { values, .. } => Some(values),
            _ => None,
        }
    }
}

/// Trait for customizing how `OpenAPI` schemas map to field types.
///
/// Default implementation handles standard `OpenAPI` to `FieldType` mapping.
/// Consumers can override for platform-specific type handling.
pub trait TypeMapper: Send + Sync {
    /// Map a schema to a field type.
    fn map_schema(&self, schema: &Schema) -> FieldType {
        schema_to_field_type(schema)
    }

    /// Map a type override string to a field type.
    /// Returns `None` if the override is not recognized.
    fn map_override(&self, override_str: &str) -> Option<FieldType> {
        match override_str {
            "bool" | "boolean" => Some(FieldType::Boolean),
            "int" | "int64" | "integer" => Some(FieldType::Integer),
            "float" | "float64" | "number" => Some(FieldType::Number),
            "string" => Some(FieldType::String),
            "list" => Some(FieldType::Array(Box::new(FieldType::String))),
            _ => None,
        }
    }
}

/// Default type mapper using standard `OpenAPI` to `FieldType` mapping.
pub struct DefaultTypeMapper;
impl TypeMapper for DefaultTypeMapper {}

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
                .map_or(FieldType::Any, |s| schema_to_field_type(s));
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
            if let Some(first_ref) = schema.all_of.iter().find(|s| s.ref_path.is_some())
                && let Some(ref_path) = first_ref.ref_path.as_deref()
            {
                let name = sekkei::ref_name(ref_path);
                return FieldType::Object(name.to_string());
            }
            FieldType::Any
        }
    };

    if let Some(values) = &schema.enum_values
        && !values.is_empty()
    {
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

    // ── FieldType Display ────────────────────────────────────────

    #[test]
    fn field_type_display_string() {
        assert_eq!(FieldType::String.to_string(), "String");
    }

    #[test]
    fn field_type_display_integer() {
        assert_eq!(FieldType::Integer.to_string(), "Integer");
    }

    #[test]
    fn field_type_display_number() {
        assert_eq!(FieldType::Number.to_string(), "Number");
    }

    #[test]
    fn field_type_display_boolean() {
        assert_eq!(FieldType::Boolean.to_string(), "Boolean");
    }

    #[test]
    fn field_type_display_any() {
        assert_eq!(FieldType::Any.to_string(), "Any");
    }

    #[test]
    fn field_type_display_array() {
        assert_eq!(
            FieldType::Array(Box::new(FieldType::Integer)).to_string(),
            "Array<Integer>"
        );
    }

    #[test]
    fn field_type_display_nested_array() {
        let nested = FieldType::Array(Box::new(FieldType::Array(Box::new(FieldType::String))));
        assert_eq!(nested.to_string(), "Array<Array<String>>");
    }

    #[test]
    fn field_type_display_map() {
        assert_eq!(
            FieldType::Map(Box::new(FieldType::String)).to_string(),
            "Map<String, String>"
        );
    }

    #[test]
    fn field_type_display_enum() {
        let e = FieldType::Enum {
            values: vec!["a".to_string(), "b".to_string()],
            underlying: Box::new(FieldType::String),
        };
        assert_eq!(e.to_string(), "Enum(a|b)");
    }

    #[test]
    fn field_type_display_object() {
        assert_eq!(FieldType::Object("Pet".to_string()).to_string(), "Pet");
    }

    // ── FieldType helpers ────────────────────────────────────────

    #[test]
    fn field_type_is_primitive() {
        assert!(FieldType::String.is_primitive());
        assert!(FieldType::Integer.is_primitive());
        assert!(FieldType::Number.is_primitive());
        assert!(FieldType::Boolean.is_primitive());
        assert!(!FieldType::Any.is_primitive());
        assert!(!FieldType::Array(Box::new(FieldType::String)).is_primitive());
        assert!(!FieldType::Map(Box::new(FieldType::String)).is_primitive());
        assert!(!FieldType::Object("Foo".to_string()).is_primitive());
    }

    #[test]
    fn field_type_is_collection() {
        assert!(FieldType::Array(Box::new(FieldType::String)).is_collection());
        assert!(FieldType::Map(Box::new(FieldType::Integer)).is_collection());
        assert!(!FieldType::String.is_collection());
        assert!(!FieldType::Any.is_collection());
        assert!(!FieldType::Object("Foo".to_string()).is_collection());
    }

    #[test]
    fn field_type_inner_type_array() {
        let arr = FieldType::Array(Box::new(FieldType::Integer));
        assert_eq!(arr.inner_type(), Some(&FieldType::Integer));
    }

    #[test]
    fn field_type_inner_type_map() {
        let map = FieldType::Map(Box::new(FieldType::Boolean));
        assert_eq!(map.inner_type(), Some(&FieldType::Boolean));
    }

    #[test]
    fn field_type_inner_type_none() {
        assert_eq!(FieldType::String.inner_type(), None);
        assert_eq!(FieldType::Any.inner_type(), None);
        assert_eq!(FieldType::Object("X".to_string()).inner_type(), None);
    }

    #[test]
    fn field_type_enum_values_some() {
        let e = FieldType::Enum {
            values: vec!["x".to_string(), "y".to_string()],
            underlying: Box::new(FieldType::String),
        };
        assert_eq!(
            e.enum_values(),
            Some(vec!["x".to_string(), "y".to_string()].as_slice())
        );
    }

    #[test]
    fn field_type_enum_values_none() {
        assert_eq!(FieldType::String.enum_values(), None);
        assert_eq!(FieldType::Integer.enum_values(), None);
        assert_eq!(FieldType::Array(Box::new(FieldType::Any)).enum_values(), None);
    }

    // ── TypeMapper trait ─────────────────────────────────────────

    #[test]
    fn default_type_mapper_override_bool() {
        let mapper = DefaultTypeMapper;
        assert_eq!(mapper.map_override("bool"), Some(FieldType::Boolean));
        assert_eq!(mapper.map_override("boolean"), Some(FieldType::Boolean));
    }

    #[test]
    fn default_type_mapper_override_int() {
        let mapper = DefaultTypeMapper;
        assert_eq!(mapper.map_override("int"), Some(FieldType::Integer));
        assert_eq!(mapper.map_override("int64"), Some(FieldType::Integer));
        assert_eq!(mapper.map_override("integer"), Some(FieldType::Integer));
    }

    #[test]
    fn default_type_mapper_override_float() {
        let mapper = DefaultTypeMapper;
        assert_eq!(mapper.map_override("float"), Some(FieldType::Number));
        assert_eq!(mapper.map_override("float64"), Some(FieldType::Number));
        assert_eq!(mapper.map_override("number"), Some(FieldType::Number));
    }

    #[test]
    fn default_type_mapper_override_string() {
        let mapper = DefaultTypeMapper;
        assert_eq!(mapper.map_override("string"), Some(FieldType::String));
    }

    #[test]
    fn default_type_mapper_override_list() {
        let mapper = DefaultTypeMapper;
        assert_eq!(
            mapper.map_override("list"),
            Some(FieldType::Array(Box::new(FieldType::String)))
        );
    }

    #[test]
    fn default_type_mapper_override_unknown() {
        let mapper = DefaultTypeMapper;
        assert_eq!(mapper.map_override("custom"), None);
        assert_eq!(mapper.map_override(""), None);
        assert_eq!(mapper.map_override("map"), None);
    }

    #[test]
    fn default_type_mapper_map_schema() {
        let mapper = DefaultTypeMapper;
        assert_eq!(mapper.map_schema(&string_schema()), FieldType::String);
        assert_eq!(mapper.map_schema(&integer_schema()), FieldType::Integer);
    }
}
