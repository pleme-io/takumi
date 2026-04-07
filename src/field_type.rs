use std::fmt;

use serde::{Deserialize, Serialize};
use sekkei::Schema;

/// Platform-independent field type for code generation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
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

impl Default for FieldType {
    fn default() -> Self {
        Self::Any
    }
}

impl std::str::FromStr for FieldType {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "String" => Self::String,
            "Integer" => Self::Integer,
            "Number" => Self::Number,
            "Boolean" => Self::Boolean,
            "Any" => Self::Any,
            other => Self::Object(other.to_string()),
        })
    }
}

impl From<&Schema> for FieldType {
    fn from(schema: &Schema) -> Self {
        schema_to_field_type(schema)
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

    /// Returns `true` if this is an Object type.
    #[must_use]
    pub fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }

    /// Returns `true` if this is an Enum type.
    #[must_use]
    pub fn is_enum(&self) -> bool {
        matches!(self, Self::Enum { .. })
    }

    /// Returns the object name if this is an Object type.
    #[must_use]
    pub fn object_name(&self) -> Option<&str> {
        match self {
            Self::Object(name) => Some(name),
            _ => None,
        }
    }

    /// Returns the nesting depth of the type (0 for scalars, 1+ for containers).
    #[must_use]
    pub fn depth(&self) -> usize {
        match self {
            Self::Array(inner) | Self::Map(inner) => 1 + inner.depth(),
            Self::Enum { underlying, .. } => underlying.depth(),
            _ => 0,
        }
    }
}

/// Trait for customizing how `OpenAPI` schemas map to field types.
///
/// Default implementation handles standard `OpenAPI` to `FieldType` mapping.
/// Consumers can override for platform-specific type handling.
pub trait TypeMapper: Send + Sync {
    /// Map a schema to a field type.
    #[must_use]
    fn map_schema(&self, schema: &Schema) -> FieldType {
        schema_to_field_type(schema)
    }

    /// Map a type override string to a field type.
    /// Returns `None` if the override is not recognized.
    #[must_use]
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

    // ── Serde round-trip ─────────────────────────────────────────

    #[test]
    fn serde_roundtrip_string() {
        let ft = FieldType::String;
        let json = serde_json::to_string(&ft).unwrap();
        let back: FieldType = serde_json::from_str(&json).unwrap();
        assert_eq!(ft, back);
    }

    #[test]
    fn serde_roundtrip_integer() {
        let ft = FieldType::Integer;
        let json = serde_json::to_string(&ft).unwrap();
        let back: FieldType = serde_json::from_str(&json).unwrap();
        assert_eq!(ft, back);
    }

    #[test]
    fn serde_roundtrip_array() {
        let ft = FieldType::Array(Box::new(FieldType::Number));
        let json = serde_json::to_string(&ft).unwrap();
        let back: FieldType = serde_json::from_str(&json).unwrap();
        assert_eq!(ft, back);
    }

    #[test]
    fn serde_roundtrip_map() {
        let ft = FieldType::Map(Box::new(FieldType::Boolean));
        let json = serde_json::to_string(&ft).unwrap();
        let back: FieldType = serde_json::from_str(&json).unwrap();
        assert_eq!(ft, back);
    }

    #[test]
    fn serde_roundtrip_object() {
        let ft = FieldType::Object("User".to_string());
        let json = serde_json::to_string(&ft).unwrap();
        let back: FieldType = serde_json::from_str(&json).unwrap();
        assert_eq!(ft, back);
    }

    #[test]
    fn serde_roundtrip_enum() {
        let ft = FieldType::Enum {
            values: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            underlying: Box::new(FieldType::String),
        };
        let json = serde_json::to_string(&ft).unwrap();
        let back: FieldType = serde_json::from_str(&json).unwrap();
        assert_eq!(ft, back);
    }

    #[test]
    fn serde_roundtrip_any() {
        let ft = FieldType::Any;
        let json = serde_json::to_string(&ft).unwrap();
        let back: FieldType = serde_json::from_str(&json).unwrap();
        assert_eq!(ft, back);
    }

    #[test]
    fn serde_roundtrip_nested() {
        let ft = FieldType::Array(Box::new(FieldType::Map(Box::new(FieldType::Object(
            "Item".to_string(),
        )))));
        let json = serde_json::to_string(&ft).unwrap();
        let back: FieldType = serde_json::from_str(&json).unwrap();
        assert_eq!(ft, back);
    }

    // ── schema_to_field_type edge cases ──────────────────────────

    #[test]
    fn ref_takes_precedence_over_type() {
        let s = Schema {
            schema_type: Some("string".to_string()),
            ref_path: Some("#/components/schemas/Name".to_string()),
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Object("Name".to_string())
        );
    }

    #[test]
    fn all_of_without_ref_is_any() {
        let s = Schema {
            all_of: vec![Schema {
                schema_type: Some("object".to_string()),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(schema_to_field_type(&s), FieldType::Any);
    }

    #[test]
    fn all_of_multiple_refs_uses_first() {
        let s = Schema {
            all_of: vec![
                Schema {
                    ref_path: Some("#/components/schemas/First".to_string()),
                    ..Default::default()
                },
                Schema {
                    ref_path: Some("#/components/schemas/Second".to_string()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Object("First".to_string())
        );
    }

    #[test]
    fn enum_with_empty_values_is_base_type() {
        let s = Schema {
            schema_type: Some("string".to_string()),
            enum_values: Some(vec![]),
            ..Default::default()
        };
        assert_eq!(schema_to_field_type(&s), FieldType::String);
    }

    #[test]
    fn enum_with_non_string_values_filters() {
        let s = Schema {
            schema_type: Some("integer".to_string()),
            enum_values: Some(vec![
                serde_json::Value::Number(1.into()),
                serde_json::Value::Number(2.into()),
            ]),
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Integer,
            "non-string enum values are filtered out, no Enum variant created"
        );
    }

    #[test]
    fn map_with_integer_values() {
        let s = Schema {
            schema_type: Some("object".to_string()),
            additional_properties: Some(Box::new(integer_schema())),
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Map(Box::new(FieldType::Integer))
        );
    }

    #[test]
    fn map_with_nested_object_values() {
        let s = Schema {
            schema_type: Some("object".to_string()),
            additional_properties: Some(Box::new(Schema {
                ref_path: Some("#/components/schemas/Widget".to_string()),
                ..Default::default()
            })),
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Map(Box::new(FieldType::Object("Widget".to_string())))
        );
    }

    #[test]
    fn array_of_refs() {
        let s = Schema {
            schema_type: Some("array".to_string()),
            items: Some(Box::new(Schema {
                ref_path: Some("#/components/schemas/Tag".to_string()),
                ..Default::default()
            })),
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Array(Box::new(FieldType::Object("Tag".to_string())))
        );
    }

    #[test]
    fn unrecognized_type_string_is_any() {
        let s = Schema {
            schema_type: Some("custom".to_string()),
            ..Default::default()
        };
        assert_eq!(schema_to_field_type(&s), FieldType::Any);
    }

    // ── FieldType enum edge cases ────────────────────────────────

    #[test]
    fn field_type_enum_not_primitive() {
        let e = FieldType::Enum {
            values: vec!["a".to_string()],
            underlying: Box::new(FieldType::String),
        };
        assert!(!e.is_primitive());
    }

    #[test]
    fn field_type_enum_not_collection() {
        let e = FieldType::Enum {
            values: vec!["a".to_string()],
            underlying: Box::new(FieldType::String),
        };
        assert!(!e.is_collection());
    }

    #[test]
    fn field_type_enum_no_inner_type() {
        let e = FieldType::Enum {
            values: vec!["a".to_string()],
            underlying: Box::new(FieldType::String),
        };
        assert!(e.inner_type().is_none());
    }

    #[test]
    fn field_type_display_single_enum_value() {
        let e = FieldType::Enum {
            values: vec!["only".to_string()],
            underlying: Box::new(FieldType::String),
        };
        assert_eq!(e.to_string(), "Enum(only)");
    }

    #[test]
    fn field_type_display_map_with_complex_value() {
        let ft = FieldType::Map(Box::new(FieldType::Array(Box::new(FieldType::Integer))));
        assert_eq!(ft.to_string(), "Map<String, Array<Integer>>");
    }

    // ── Custom TypeMapper ────────────────────────────────────────

    struct CustomMapper;
    impl TypeMapper for CustomMapper {
        fn map_schema(&self, _schema: &Schema) -> FieldType {
            FieldType::String
        }

        fn map_override(&self, override_str: &str) -> Option<FieldType> {
            if override_str == "uuid" {
                Some(FieldType::String)
            } else {
                None
            }
        }
    }

    #[test]
    fn custom_type_mapper_map_schema() {
        let mapper = CustomMapper;
        assert_eq!(mapper.map_schema(&integer_schema()), FieldType::String);
    }

    #[test]
    fn custom_type_mapper_map_override() {
        let mapper = CustomMapper;
        assert_eq!(mapper.map_override("uuid"), Some(FieldType::String));
        assert_eq!(mapper.map_override("int"), None);
    }

    #[test]
    fn type_mapper_as_trait_object() {
        let mapper: Box<dyn TypeMapper> = Box::new(DefaultTypeMapper);
        assert_eq!(mapper.map_schema(&string_schema()), FieldType::String);
        assert_eq!(mapper.map_override("int"), Some(FieldType::Integer));
    }

    // ── FieldType equality ───────────────────────────────────────

    #[test]
    fn field_type_equality() {
        assert_eq!(FieldType::String, FieldType::String);
        assert_ne!(FieldType::String, FieldType::Integer);
        assert_ne!(
            FieldType::Array(Box::new(FieldType::String)),
            FieldType::Array(Box::new(FieldType::Integer))
        );
        assert_eq!(
            FieldType::Object("A".to_string()),
            FieldType::Object("A".to_string())
        );
        assert_ne!(
            FieldType::Object("A".to_string()),
            FieldType::Object("B".to_string())
        );
    }

    #[test]
    fn field_type_clone() {
        let original = FieldType::Array(Box::new(FieldType::Object("Test".to_string())));
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn field_type_debug() {
        let ft = FieldType::String;
        let debug = format!("{ft:?}");
        assert!(debug.contains("String"));
    }

    // ── Default impl ────────────────────────────────────────────

    #[test]
    fn field_type_default_is_any() {
        assert_eq!(FieldType::default(), FieldType::Any);
    }

    // ── FromStr round-trip ──────────────────────────────────────

    #[test]
    fn field_type_from_str_primitives() {
        assert_eq!("String".parse::<FieldType>().unwrap(), FieldType::String);
        assert_eq!("Integer".parse::<FieldType>().unwrap(), FieldType::Integer);
        assert_eq!("Number".parse::<FieldType>().unwrap(), FieldType::Number);
        assert_eq!("Boolean".parse::<FieldType>().unwrap(), FieldType::Boolean);
        assert_eq!("Any".parse::<FieldType>().unwrap(), FieldType::Any);
    }

    #[test]
    fn field_type_from_str_object() {
        assert_eq!(
            "Pet".parse::<FieldType>().unwrap(),
            FieldType::Object("Pet".to_string())
        );
    }

    #[test]
    fn field_type_display_from_str_roundtrip() {
        for ft in [
            FieldType::String,
            FieldType::Integer,
            FieldType::Number,
            FieldType::Boolean,
            FieldType::Any,
        ] {
            let s = ft.to_string();
            let parsed: FieldType = s.parse().unwrap();
            assert_eq!(ft, parsed);
        }
    }

    // ── From<&Schema> ───────────────────────────────────────────

    #[test]
    fn field_type_from_schema() {
        let s = string_schema();
        let ft: FieldType = (&s).into();
        assert_eq!(ft, FieldType::String);
    }

    #[test]
    fn field_type_from_schema_integer() {
        let s = integer_schema();
        let ft: FieldType = FieldType::from(&s);
        assert_eq!(ft, FieldType::Integer);
    }

    // ── is_object / is_enum / object_name ───────────────────────

    #[test]
    fn field_type_is_object() {
        assert!(FieldType::Object("Pet".to_string()).is_object());
        assert!(!FieldType::String.is_object());
        assert!(!FieldType::Array(Box::new(FieldType::Any)).is_object());
    }

    #[test]
    fn field_type_is_enum() {
        let e = FieldType::Enum {
            values: vec!["a".to_string()],
            underlying: Box::new(FieldType::String),
        };
        assert!(e.is_enum());
        assert!(!FieldType::String.is_enum());
    }

    #[test]
    fn field_type_object_name() {
        assert_eq!(
            FieldType::Object("User".to_string()).object_name(),
            Some("User")
        );
        assert_eq!(FieldType::String.object_name(), None);
    }

    // ── depth ────────────────────────────────────────────────────

    #[test]
    fn field_type_depth_scalar() {
        assert_eq!(FieldType::String.depth(), 0);
        assert_eq!(FieldType::Integer.depth(), 0);
        assert_eq!(FieldType::Any.depth(), 0);
        assert_eq!(FieldType::Object("X".to_string()).depth(), 0);
    }

    #[test]
    fn field_type_depth_array() {
        assert_eq!(FieldType::Array(Box::new(FieldType::String)).depth(), 1);
    }

    #[test]
    fn field_type_depth_nested() {
        let nested = FieldType::Array(Box::new(FieldType::Map(Box::new(FieldType::Integer))));
        assert_eq!(nested.depth(), 2);
    }

    #[test]
    fn field_type_depth_enum() {
        let e = FieldType::Enum {
            values: vec!["a".to_string()],
            underlying: Box::new(FieldType::String),
        };
        assert_eq!(e.depth(), 0);
    }

    // ── Hash ────────────────────────────────────────────────────

    #[test]
    fn field_type_hashable() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(FieldType::String);
        set.insert(FieldType::Integer);
        set.insert(FieldType::String);
        assert_eq!(set.len(), 2);
    }
}
