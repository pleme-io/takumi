use std::fmt;

use sekkei::Schema;
use serde::{Deserialize, Serialize};

/// One member of an `enum` — or of a `const`, which is a one-value enum.
///
/// JSON allows any value here, but [`FieldType`] derives `Hash + Eq`, so this
/// cannot simply hold a `serde_json::Value` (`f64` is neither). The scalar arms
/// cover every enum member and every `const` in practice; `Other` keeps
/// anything else as its JSON text rather than discarding it, because silently
/// dropping a value is precisely the defect this type exists to remove.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EnumValue {
    Str(std::string::String),
    Int(i64),
    Bool(bool),
    Null,
    /// A value no other arm can hold — a non-integer number, an array, an
    /// object — preserved as its JSON text so nothing is lost.
    Other(std::string::String),
}

impl EnumValue {
    /// The string content, when this member is a string.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The integer content, when this member is an integer.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }
}

impl From<&serde_json::Value> for EnumValue {
    fn from(v: &serde_json::Value) -> Self {
        match v {
            serde_json::Value::String(s) => Self::Str(s.clone()),
            serde_json::Value::Bool(b) => Self::Bool(*b),
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Number(n) => n
                .as_i64()
                .map_or_else(|| Self::Other(n.to_string()), Self::Int),
            other => Self::Other(other.to_string()),
        }
    }
}

impl fmt::Display for EnumValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Str(s) => f.write_str(s),
            Self::Int(i) => write!(f, "{i}"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Null => f.write_str("null"),
            Self::Other(t) => f.write_str(t),
        }
    }
}

impl From<&str> for EnumValue {
    fn from(s: &str) -> Self {
        Self::Str(s.to_string())
    }
}

impl From<std::string::String> for EnumValue {
    fn from(s: std::string::String) -> Self {
        Self::Str(s)
    }
}

impl From<i64> for EnumValue {
    fn from(i: i64) -> Self {
        Self::Int(i)
    }
}

/// Compare directly against a string, so a caller checking a string-valued
/// member does not have to construct an `EnumValue` to do it. Mirrors the
/// `String: PartialEq<&str>` convention. A non-string member is never equal to
/// a string, which is the honest answer rather than a stringified match.
impl PartialEq<&str> for EnumValue {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == Some(*other)
    }
}

impl PartialEq<EnumValue> for &str {
    fn eq(&self, other: &EnumValue) -> bool {
        other.as_str() == Some(*self)
    }
}

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
        /// Every declared member, in declaration order.
        ///
        /// Typed rather than `Vec<String>`: an enum's members are the values
        /// that distinguish it, and for a union they are the discriminant
        /// itself — so narrowing them to strings erased the information a
        /// generator most needs.
        values: Vec<EnumValue>,
        underlying: Box<FieldType>,
    },
    /// JSON `null` is the only inhabitant of this position.
    ///
    /// Distinct from `Nullable`: this is a value that is *always* null, which is
    /// how a spec spells "the presence of this key is the whole signal". Four
    /// such properties exist in Discord's spec, one of them `required`.
    Null,
    /// `T`, widened to admit JSON `null` here.
    ///
    /// Nullability lives inside the type rather than beside it on a field
    /// record, because it composes at container boundaries: an array whose
    /// items are nullable and which is itself nullable is `Option<Vec<Option<T>>>`,
    /// and a single `nullable: bool` on the field can encode only one of those
    /// two levels while silently losing the other.
    Nullable(Box<FieldType>),
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
            Self::Enum { values, .. } => {
                let rendered: Vec<std::string::String> =
                    values.iter().map(ToString::to_string).collect();
                write!(f, "Enum({})", rendered.join("|"))
            }
            Self::Null => f.write_str("Null"),
            Self::Nullable(inner) => write!(f, "Nullable<{inner}>"),
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

/// Resolves `$ref` pointers against a spec while refusing to loop.
///
/// Two things this deliberately is not. It is not single-hop: sekkei's own
/// `resolve_schema_ref` is one dictionary lookup, so a chain `Alias -> Real`
/// stops at `Alias` and a caller reading its (empty) properties sees a phantom
/// type with no error anywhere.
///
/// And the loop guard is a resolution STACK, not a global seen-set. That
/// distinction is the whole correctness of it: a schema graph is a DAG with
/// heavy sharing — Discord re-enters `ActionRowComponentResponse` from five
/// distinct parents — so a set that never forgets would treat the second,
/// legitimate visit as a cycle and yield the same phantom the multi-hop fix
/// exists to remove. Only a *path* revisit is a cycle.
pub struct RefResolver<'a> {
    spec: &'a sekkei::OpenApiSpec,
    stack: Vec<std::string::String>,
}

impl<'a> RefResolver<'a> {
    #[must_use]
    pub fn new(spec: &'a sekkei::OpenApiSpec) -> Self {
        Self {
            spec,
            stack: Vec::new(),
        }
    }

    /// The spec being resolved against.
    #[must_use]
    pub fn spec(&self) -> &'a sekkei::OpenApiSpec {
        self.spec
    }

    /// Follow a `$ref` chain to the first schema that is not itself a bare
    /// `$ref`, returning that schema **and the pointer that names it**.
    ///
    /// The pointer is returned alongside because the terminal target's own name
    /// is what a generator must emit — naming the alias instead produces a type
    /// whose properties are empty, which is the phantom this resolver exists to
    /// prevent. `None` when the pointer dangles, or when following it would
    /// re-enter a schema already on the current resolution path.
    #[must_use]
    pub fn resolve_named(&mut self, ref_path: &str) -> Option<(&'a Schema, std::string::String)> {
        let mut cur = ref_path.to_string();
        let depth = self.stack.len();
        let out = loop {
            if self.stack.contains(&cur) {
                break None; // a cycle on this path, not a shared re-visit
            }
            let Some(target) = self.spec.resolve_schema_ref(&cur) else {
                break None; // dangling pointer
            };
            self.stack.push(cur.clone());
            match &target.ref_path {
                Some(next) => cur.clone_from(next),
                None => break Some((target, cur)),
            }
        };
        self.stack.truncate(depth);
        out
    }

    /// As [`Self::resolve_named`], discarding the terminal pointer.
    #[must_use]
    pub fn resolve(&mut self, ref_path: &str) -> Option<&'a Schema> {
        self.resolve_named(ref_path).map(|(s, _)| s)
    }

    /// Run `f` with `name` held on the resolution path, so anything it resolves
    /// transitively can detect a cycle back to here.
    pub fn within<T>(&mut self, name: &str, f: impl FnOnce(&mut Self) -> T) -> T {
        self.stack.push(name.to_string());
        let out = f(self);
        self.stack.pop();
        out
    }

    /// How deep the current resolution path is.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.stack.len()
    }
}

/// True when this schema's only inhabitant is JSON `null`.
///
/// Tested against three spellings rather than one. sekkei normalises a scalar
/// `{"type":"null"}` to `schema_type = Some("null")` with `nullable = true`
/// (measured, not assumed), but `{"const": null}` and a `type` array naming
/// only `"null"` are equally legal and cost nothing to accept.
fn is_null_literal(s: &Schema) -> bool {
    s.schema_type.as_deref() == Some("null")
        || s.const_value
            .as_ref()
            .is_some_and(serde_json::Value::is_null)
        || (!s.type_union.is_empty() && s.type_union.iter().all(|t| t == "null"))
}

impl FieldType {
    /// This type with any `Nullable` wrapper removed.
    ///
    /// Every predicate below answers about the underlying type, because
    /// nullability is orthogonal to what a thing *is*: `Option<Vec<T>>` is still
    /// a collection, and `Option<SomeEnum>` is still an enum whose members a
    /// discriminant search must be able to read.
    ///
    /// This exists because `FieldType` is `#[non_exhaustive]` and only `Display`
    /// is an exhaustive match — every other accessor uses `matches!` or a
    /// wildcard, so a new variant is accepted silently with a wrong answer
    /// rather than caught by the compiler. Routing them all through one peel is
    /// what stops that being nine separate omissions.
    #[must_use]
    pub fn non_null(&self) -> &Self {
        match self {
            Self::Nullable(inner) => inner.non_null(),
            other => other,
        }
    }

    /// Whether this position admits JSON `null`.
    #[must_use]
    pub fn is_nullable(&self) -> bool {
        matches!(self, Self::Nullable(_) | Self::Null)
    }

    /// Check if this is a primitive type.
    #[must_use]
    pub fn is_primitive(&self) -> bool {
        matches!(
            self.non_null(),
            Self::String | Self::Integer | Self::Number | Self::Boolean
        )
    }

    /// Check if this is a collection type (Array or Map).
    #[must_use]
    pub fn is_collection(&self) -> bool {
        matches!(self.non_null(), Self::Array(_) | Self::Map(_))
    }

    /// Get the inner type for Array or Map.
    #[must_use]
    pub fn inner_type(&self) -> Option<&Self> {
        match self.non_null() {
            Self::Array(inner) | Self::Map(inner) => Some(inner),
            _ => None,
        }
    }

    /// Get enum values if this is an Enum type.
    #[must_use]
    pub fn enum_values(&self) -> Option<&[EnumValue]> {
        match self.non_null() {
            Self::Enum { values, .. } => Some(values),
            _ => None,
        }
    }

    /// The single value this type is pinned to, if it is a one-member enum.
    ///
    /// A `const`, and an `enum` of length one, are the same thing to a
    /// generator — and both are how a spec marks which variant of a union a
    /// schema is. Discord writes the latter (`{"enum":[2],"allOf":[…]}`) and
    /// never the former, so a discriminator search that looks only for `const`
    /// finds nothing across all 85 of its polymorphic unions.
    #[must_use]
    pub fn pinned_value(&self) -> Option<&EnumValue> {
        match self.non_null() {
            Self::Enum { values, .. } => match values.as_slice() {
                [only] => Some(only),
                _ => None,
            },
            _ => None,
        }
    }

    /// Returns `true` if this is an Object type.
    #[must_use]
    pub fn is_object(&self) -> bool {
        matches!(self.non_null(), Self::Object(_))
    }

    /// Returns `true` if this is an Enum type.
    #[must_use]
    pub fn is_enum(&self) -> bool {
        matches!(self.non_null(), Self::Enum { .. })
    }

    /// Returns the object name if this is an Object type.
    #[must_use]
    pub fn object_name(&self) -> Option<&str> {
        match self.non_null() {
            Self::Object(name) => Some(name),
            _ => None,
        }
    }

    /// Returns the nesting depth of the type (0 for scalars, 1+ for containers).
    #[must_use]
    pub fn depth(&self) -> usize {
        match self.non_null() {
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
    let core = schema_core_type(schema);
    // A `type` array carrying "null" was normalised by sekkei into
    // `nullable = true`; put that back into the type, where it composes. The
    // guard keeps `Nullable(Nullable(_))` and `Nullable(Null)` unconstructible.
    if schema.nullable && !matches!(core, FieldType::Null | FieldType::Nullable(_)) {
        return FieldType::Nullable(Box::new(core));
    }
    core
}

/// Map a schema to a field type **with the spec in hand**.
///
/// The spec-blind [`schema_to_field_type`] cannot resolve a `$ref`, so it
/// answers `Object(name)` and stops. That is fine for a name, and not fine for
/// anything that has to look *through* the reference — chiefly classifying a
/// union, which requires reading each variant's discriminating property.
///
/// Today the only behavioural difference is that a `$ref` chain resolves to its
/// terminal target rather than its first hop. That is a no-op on Discord's spec
/// (measured: zero component schemas are a bare `$ref`) and the correct answer
/// on any spec that does alias, where the single-hop answer is a type whose
/// properties are empty because they live one hop further on.
pub fn schema_to_field_type_in(r: &mut RefResolver<'_>, schema: &Schema) -> FieldType {
    if let Some(ref_path) = &schema.ref_path {
        // Follow the chain; name the terminal target, not the alias. A dangling
        // pointer or a cycle keeps the first-hop name, which is still the most
        // useful thing to call the type.
        let name = r.resolve_named(ref_path).map_or_else(
            || sekkei::ref_name(ref_path).to_string(),
            |(_, pointer)| sekkei::ref_name(&pointer).to_string(),
        );
        return FieldType::Object(name);
    }
    schema_to_field_type(schema)
}

/// The type ignoring any nullability the *node itself* declares.
fn schema_core_type(schema: &Schema) -> FieldType {
    // Step 1 — `$ref` short-circuits. A `$ref` node has no type of its own to
    // classify, and nothing in this spec carries a load-bearing sibling.
    if let Some(ref_path) = &schema.ref_path {
        let name = sekkei::ref_name(ref_path);
        return FieldType::Object(name.to_string());
    }

    // Step 2 — peel `null` BEFORE any union or const test, because this is the
    // most order-sensitive step in the whole classifier. A two-member
    // `oneOf:[T, {"type":"null"}]` is `Option<T>`, and it is how this spec
    // spells nullability 305 times. Tested as a union first it becomes a
    // one-variant union — the most common wrong answer available. Tested
    // against the all-const predicate first it escapes that bucket too, since
    // a null member carries no `const`.
    if is_null_literal(schema) {
        return FieldType::Null;
    }
    let members = if schema.one_of.is_empty() {
        &schema.any_of
    } else {
        &schema.one_of
    };
    if !members.is_empty() {
        let residue: Vec<&Schema> = members.iter().filter(|m| !is_null_literal(m)).collect();
        let had_null = residue.len() < members.len();
        match residue.as_slice() {
            // Every member was null.
            [] if had_null => return FieldType::Null,
            // The nullable-wrapper idiom, and the degenerate one-member union:
            // recurse on the sole survivor rather than wrapping it in a union
            // of one.
            [only] => {
                let inner = schema_to_field_type(only);
                return if had_null && !inner.is_nullable() {
                    FieldType::Nullable(Box::new(inner))
                } else {
                    inner
                };
            }
            // Two or more real members: a genuine union. Classifying it needs
            // the spec (to resolve each variant's $ref and read its
            // discriminant), which this spec-blind entry point does not have,
            // so it falls through to the existing behaviour for now rather
            // than guessing a tag.
            _ => {}
        }
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
        // Every member is kept. This previously filtered to `v.as_str()`,
        // which had two effects and both were silent: non-string members were
        // dropped, and an all-numeric enum produced an EMPTY list that fell
        // through to `base_type`, so the enum disappeared entirely rather than
        // arriving partially. Discord declares 165 integer members against 51
        // string ones, and its unions are discriminated by an integer `enum`
        // of length one — so the old filter erased the discriminant of every
        // polymorphic union in the spec before any classifier could read it.
        return FieldType::Enum {
            values: values.iter().map(EnumValue::from).collect(),
            underlying: Box::new(base_type),
        };
    }

    // `const` is an enum of one. sekkei keeps them as distinct keywords
    // because the wire does, but they mean the same thing to a generator, and
    // a discriminator search has to see both spellings to find anything.
    if let Some(pinned) = &schema.const_value {
        return FieldType::Enum {
            values: vec![EnumValue::from(pinned)],
            underlying: Box::new(base_type),
        };
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
                values: vec!["a".into(), "b".into()],
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
            values: vec!["a".into(), "b".into()],
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
            values: vec!["x".into(), "y".into()],
            underlying: Box::new(FieldType::String),
        };
        assert_eq!(
            e.enum_values(),
            Some(vec![EnumValue::from("x"), EnumValue::from("y")].as_slice())
        );
    }

    #[test]
    fn field_type_enum_values_none() {
        assert_eq!(FieldType::String.enum_values(), None);
        assert_eq!(FieldType::Integer.enum_values(), None);
        assert_eq!(
            FieldType::Array(Box::new(FieldType::Any)).enum_values(),
            None
        );
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
            values: vec!["a".into(), "b".into(), "c".into()],
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
    fn integer_enum_is_preserved_not_erased() {
        let s = Schema {
            schema_type: Some("integer".to_string()),
            enum_values: Some(vec![
                serde_json::Value::Number(1.into()),
                serde_json::Value::Number(2.into()),
            ]),
            ..Default::default()
        };
        // This assertion is the inverse of the one it replaces. The old test
        // pinned `FieldType::Integer` -- an all-numeric enum used to filter
        // down to an empty list and then vanish into its base type, so the
        // members were gone with no error. Discord declares 165 integer enum
        // members, and its unions are discriminated by an integer enum of
        // length one, so that erasure removed every discriminant in the spec.
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Enum {
                values: vec![EnumValue::Int(1), EnumValue::Int(2)],
                underlying: Box::new(FieldType::Integer),
            }
        );
    }

    // ── nullability ────────────────────────────────────────────────────

    fn json(src: &str) -> Schema {
        serde_json::from_str(src).unwrap()
    }

    #[test]
    fn one_of_with_a_null_member_is_option_not_a_one_variant_union() {
        // 305 sites in Discord's spec spell nullability this way. Classified as
        // a union first, each becomes a union of one -- the most common wrong
        // answer available -- so the peel must precede every union test.
        // r##"..."## because the JSON pointer contains `"#`, which would
        // terminate a single-hash raw string.
        let s = json(r##"{"oneOf":[{"$ref":"#/components/schemas/Pet"},{"type":"null"}]}"##);
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Nullable(Box::new(FieldType::Object("Pet".into())))
        );
    }

    #[test]
    fn type_array_and_one_of_null_reach_the_same_type() {
        let a = json(r#"{"type":["string","null"]}"#);
        let b = json(r#"{"oneOf":[{"type":"string"},{"type":"null"}]}"#);
        let want = FieldType::Nullable(Box::new(FieldType::String));
        assert_eq!(schema_to_field_type(&a), want);
        assert_eq!(schema_to_field_type(&b), want);
    }

    #[test]
    fn a_bare_null_schema_is_null_not_any() {
        assert_eq!(
            schema_to_field_type(&json(r#"{"type":"null"}"#)),
            FieldType::Null
        );
    }

    #[test]
    fn const_null_is_indistinguishable_from_absent_a_known_limit() {
        // `sekkei::Schema.const_value` is `Option<Value>`, and serde maps a JSON
        // `null` onto `None` — so `{"const": null}` and a schema with no `const`
        // at all arrive identically, and `is_null_literal` cannot see it.
        //
        // Recorded rather than fixed: `const: null` occurs ZERO times in
        // Discord's spec (measured), so distinguishing it would be speculative
        // work on a shape nothing produces. Fixing it needs a custom
        // deserializer on that field in sekkei, and this test is where that
        // change should announce itself by failing.
        assert_eq!(
            schema_to_field_type(&json(r#"{"const":null}"#)),
            FieldType::Any,
            "if this now returns Null, sekkei gained a null-distinguishing \
             deserializer and is_null_literal's const arm became reachable"
        );
    }

    #[test]
    fn nullable_never_nests() {
        // `type:["string","null"]` sets sekkei's `nullable` AND is a type array;
        // both paths must not each add a wrapper.
        let s = json(r#"{"oneOf":[{"type":["string","null"]},{"type":"null"}]}"#);
        let t = schema_to_field_type(&s);
        assert_eq!(t, FieldType::Nullable(Box::new(FieldType::String)));
        assert!(!matches!(t.non_null(), FieldType::Nullable(_)));
    }

    #[test]
    fn a_one_member_one_of_recurses_rather_than_wrapping() {
        let s = json(r#"{"oneOf":[{"type":"integer"}]}"#);
        assert_eq!(schema_to_field_type(&s), FieldType::Integer);
    }

    #[test]
    fn nullability_composes_through_a_container() {
        // Option<Vec<Option<T>>> -- 8 such sites exist in Discord's spec, and a
        // field-level `nullable: bool` can encode only one of the two levels.
        let s = json(r#"{"type":["array","null"],"items":{"type":["string","null"]}}"#);
        let t = schema_to_field_type(&s);
        assert!(t.is_nullable(), "outer");
        assert!(t.inner_type().unwrap().is_nullable(), "items");
    }

    // ── accessors on the new variants ──────────────────────────────────
    //
    // `Display` is the ONLY exhaustive match on FieldType; every accessor below
    // uses `matches!` or a wildcard, so a new variant is accepted silently with
    // a wrong answer instead of failing to compile. These assertions are the
    // forcing function the compiler does not provide.

    #[test]
    fn nullable_is_transparent_to_every_accessor() {
        let e = FieldType::Nullable(Box::new(FieldType::Enum {
            values: vec![EnumValue::Int(2)],
            underlying: Box::new(FieldType::Integer),
        }));
        // The sharpest one: the discriminant search reads `pinned_value`, and
        // 8 of Discord's buried discriminants are nullable-wrapped. Returning
        // None here would silently cost those unions their tag.
        assert_eq!(e.pinned_value(), Some(&EnumValue::Int(2)));
        assert!(e.is_enum());
        assert_eq!(e.enum_values().unwrap().len(), 1);

        let arr = FieldType::Nullable(Box::new(FieldType::Array(Box::new(FieldType::String))));
        assert!(arr.is_collection());
        assert_eq!(arr.inner_type(), Some(&FieldType::String));
        assert_eq!(arr.depth(), 1, "nullability is not a nesting level");

        let obj = FieldType::Nullable(Box::new(FieldType::Object("Pet".into())));
        assert!(obj.is_object());
        assert_eq!(obj.object_name(), Some("Pet"));

        assert!(FieldType::Nullable(Box::new(FieldType::String)).is_primitive());
        assert!(FieldType::Nullable(Box::new(FieldType::String)).is_nullable());
        assert!(FieldType::Null.is_nullable());
        assert!(!FieldType::Null.is_primitive());
    }

    // ── $ref resolution ────────────────────────────────────────────────

    fn spec_with(schemas: &str) -> sekkei::OpenApiSpec {
        let doc = format!(
            r#"{{"openapi":"3.1.0","info":{{"title":"t","version":"1"}},
                "paths":{{}},"components":{{"schemas":{schemas}}}}}"#
        );
        serde_json::from_str(&doc).unwrap()
    }

    #[test]
    fn a_ref_chain_resolves_to_its_terminal_target_not_its_first_hop() {
        // Single-hop resolution stops at `Alias`, whose properties are empty --
        // a phantom type, emitted with no error anywhere.
        let spec = spec_with(
            r##"{"Alias":{"$ref":"#/components/schemas/Real"},
                 "Real":{"type":"object","properties":{"a":{"type":"string"}}}}"##,
        );
        let mut r = RefResolver::new(&spec);
        let (target, pointer) = r.resolve_named("#/components/schemas/Alias").unwrap();
        assert_eq!(sekkei::ref_name(&pointer), "Real");
        assert!(target.properties.contains_key("a"), "reached the real body");

        let field = json(r##"{"$ref":"#/components/schemas/Alias"}"##);
        assert_eq!(
            schema_to_field_type_in(&mut RefResolver::new(&spec), &field),
            FieldType::Object("Real".into())
        );
    }

    #[test]
    fn a_cycle_is_refused_rather_than_looping() {
        let spec = spec_with(
            r##"{"A":{"$ref":"#/components/schemas/B"},
                 "B":{"$ref":"#/components/schemas/A"}}"##,
        );
        let mut r = RefResolver::new(&spec);
        // `is_none()` rather than `assert_eq!(.., None)`: sekkei::Schema does
        // not derive PartialEq, so the Option cannot be compared directly.
        assert!(r.resolve("#/components/schemas/A").is_none());
        assert_eq!(r.depth(), 0, "the path is unwound even on refusal");
    }

    #[test]
    fn a_shared_revisit_is_not_a_cycle() {
        // THE distinction that makes this a stack and not a seen-set. A schema
        // graph is a DAG with heavy sharing -- Discord re-enters
        // ActionRowComponentResponse from five distinct parents -- and a guard
        // that never forgets would call the second visit a cycle and hand back
        // the same phantom the resolver exists to prevent.
        let spec = spec_with(r#"{"Shared":{"type":"object"}}"#);
        let mut r = RefResolver::new(&spec);
        assert!(r.resolve("#/components/schemas/Shared").is_some());
        assert!(
            r.resolve("#/components/schemas/Shared").is_some(),
            "a second, sibling visit must still resolve"
        );
        // And nested under an unrelated path, it still resolves.
        r.within("Parent", |r| {
            assert!(r.resolve("#/components/schemas/Shared").is_some());
        });
        assert_eq!(r.depth(), 0);
    }

    #[test]
    fn a_dangling_ref_keeps_the_written_name() {
        let spec = spec_with(r#"{"Real":{"type":"object"}}"#);
        let field = json(r##"{"$ref":"#/components/schemas/Ghost"}"##);
        assert_eq!(
            schema_to_field_type_in(&mut RefResolver::new(&spec), &field),
            FieldType::Object("Ghost".into()),
            "the written name is still the most useful thing to call it"
        );
    }

    #[test]
    fn new_variants_render() {
        assert_eq!(FieldType::Null.to_string(), "Null");
        assert_eq!(
            FieldType::Nullable(Box::new(FieldType::String)).to_string(),
            "Nullable<String>"
        );
    }

    #[test]
    fn const_is_an_enum_of_one() {
        let s = Schema {
            schema_type: Some("integer".to_string()),
            const_value: Some(serde_json::json!(2)),
            ..Default::default()
        };
        let t = schema_to_field_type(&s);
        assert_eq!(t.pinned_value(), Some(&EnumValue::Int(2)));
    }

    #[test]
    fn single_member_enum_is_pinned_the_same_as_const() {
        // Discord writes its discriminants this way -- `{"enum":[2], ...}` --
        // and never as `const`, so both spellings must reach `pinned_value`
        // or a discriminator search finds nothing.
        let s = Schema {
            schema_type: Some("integer".to_string()),
            enum_values: Some(vec![serde_json::json!(2)]),
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s).pinned_value(),
            Some(&EnumValue::Int(2))
        );
    }

    #[test]
    fn a_multi_member_enum_is_not_pinned() {
        let s = Schema {
            schema_type: Some("integer".to_string()),
            enum_values: Some(vec![serde_json::json!(1), serde_json::json!(2)]),
            ..Default::default()
        };
        assert_eq!(schema_to_field_type(&s).pinned_value(), None);
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
            values: vec!["a".into()],
            underlying: Box::new(FieldType::String),
        };
        assert!(!e.is_primitive());
    }

    #[test]
    fn field_type_enum_not_collection() {
        let e = FieldType::Enum {
            values: vec!["a".into()],
            underlying: Box::new(FieldType::String),
        };
        assert!(!e.is_collection());
    }

    #[test]
    fn field_type_enum_no_inner_type() {
        let e = FieldType::Enum {
            values: vec!["a".into()],
            underlying: Box::new(FieldType::String),
        };
        assert!(e.inner_type().is_none());
    }

    #[test]
    fn field_type_display_single_enum_value() {
        let e = FieldType::Enum {
            values: vec!["only".into()],
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
            values: vec!["a".into()],
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
            values: vec!["a".into()],
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

    // ── schema_to_field_type: oneOf / anyOf composition ─────────

    #[test]
    fn one_of_with_ref_is_any() {
        // oneOf without allOf ref pattern falls through to Any
        let s = Schema {
            one_of: vec![
                Schema {
                    ref_path: Some("#/components/schemas/Cat".to_string()),
                    ..Default::default()
                },
                Schema {
                    ref_path: Some("#/components/schemas/Dog".to_string()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        // oneOf doesn't have special handling, falls to default branch
        assert_eq!(schema_to_field_type(&s), FieldType::Any);
    }

    #[test]
    fn any_of_without_type_is_any() {
        let s = Schema {
            any_of: vec![
                Schema {
                    schema_type: Some("string".to_string()),
                    ..Default::default()
                },
                Schema {
                    schema_type: Some("integer".to_string()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(schema_to_field_type(&s), FieldType::Any);
    }

    // ── schema_to_field_type: allOf with non-ref first ──────────

    #[test]
    fn all_of_ref_not_first_finds_it() {
        let s = Schema {
            all_of: vec![
                Schema {
                    schema_type: Some("object".to_string()),
                    ..Default::default()
                },
                Schema {
                    ref_path: Some("#/components/schemas/Mixin".to_string()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Object("Mixin".to_string())
        );
    }

    // ── schema_to_field_type: enum on non-string type ───────────

    #[test]
    fn enum_on_integer_with_string_values() {
        let s = Schema {
            schema_type: Some("integer".to_string()),
            enum_values: Some(vec![
                serde_json::Value::String("1".to_string()),
                serde_json::Value::String("2".to_string()),
            ]),
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Enum {
                values: vec!["1".into(), "2".into()],
                underlying: Box::new(FieldType::Integer),
            }
        );
    }

    #[test]
    fn enum_with_mixed_values_keeps_every_member() {
        let s = Schema {
            schema_type: Some("string".to_string()),
            enum_values: Some(vec![
                serde_json::Value::String("valid".to_string()),
                serde_json::Value::Number(42.into()),
                serde_json::Value::Null,
                serde_json::Value::String("also_valid".to_string()),
            ]),
            ..Default::default()
        };
        match schema_to_field_type(&s) {
            FieldType::Enum {
                values, underlying, ..
            } => {
                // The old test asserted `["valid", "also_valid"]` -- two of the
                // four members, silently. Order is preserved too, because a
                // member's position is meaningful in some encodings.
                assert_eq!(
                    values,
                    vec![
                        EnumValue::Str("valid".to_string()),
                        EnumValue::Int(42),
                        EnumValue::Null,
                        EnumValue::Str("also_valid".to_string()),
                    ]
                );
                assert_eq!(*underlying, FieldType::String);
            }
            other => panic!("expected Enum, got: {other:?}"),
        }
    }

    #[test]
    fn a_value_no_scalar_arm_can_hold_is_kept_as_text_not_dropped() {
        let s = Schema {
            schema_type: Some("number".to_string()),
            enum_values: Some(vec![serde_json::json!(1.5), serde_json::json!([1, 2])]),
            ..Default::default()
        };
        // `FieldType` derives Hash + Eq so a float cannot be held natively;
        // keeping the JSON text is the honest alternative to discarding it,
        // and it stays visible to a generator as a value it must handle.
        assert_eq!(
            schema_to_field_type(&s).enum_values(),
            Some(
                [
                    EnumValue::Other("1.5".to_string()),
                    EnumValue::Other("[1,2]".to_string()),
                ]
                .as_slice()
            )
        );
    }

    // ── nested type conversions ─────────────────────────────────

    #[test]
    fn array_of_array_of_objects() {
        let s = Schema {
            schema_type: Some("array".to_string()),
            items: Some(Box::new(Schema {
                schema_type: Some("array".to_string()),
                items: Some(Box::new(Schema {
                    ref_path: Some("#/components/schemas/Cell".to_string()),
                    ..Default::default()
                })),
                ..Default::default()
            })),
            ..Default::default()
        };
        let ft = schema_to_field_type(&s);
        assert_eq!(
            ft,
            FieldType::Array(Box::new(FieldType::Array(Box::new(FieldType::Object(
                "Cell".to_string()
            )))))
        );
        assert_eq!(ft.depth(), 2);
    }

    #[test]
    fn map_of_arrays() {
        let s = Schema {
            schema_type: Some("object".to_string()),
            additional_properties: Some(Box::new(Schema {
                schema_type: Some("array".to_string()),
                items: Some(Box::new(Schema {
                    schema_type: Some("string".to_string()),
                    ..Default::default()
                })),
                ..Default::default()
            })),
            ..Default::default()
        };
        assert_eq!(
            schema_to_field_type(&s),
            FieldType::Map(Box::new(FieldType::Array(Box::new(FieldType::String))))
        );
    }

    // ── From<&Schema> trait with complex types ──────────────────

    #[test]
    fn from_schema_array_of_refs() {
        let s = Schema {
            schema_type: Some("array".to_string()),
            items: Some(Box::new(Schema {
                ref_path: Some("#/components/schemas/Item".to_string()),
                ..Default::default()
            })),
            ..Default::default()
        };
        let ft: FieldType = (&s).into();
        assert_eq!(
            ft,
            FieldType::Array(Box::new(FieldType::Object("Item".to_string())))
        );
    }

    #[test]
    fn from_schema_enum() {
        let s = Schema {
            schema_type: Some("string".to_string()),
            enum_values: Some(vec![
                serde_json::Value::String("x".to_string()),
                serde_json::Value::String("y".to_string()),
            ]),
            ..Default::default()
        };
        let ft = FieldType::from(&s);
        assert!(ft.is_enum());
        assert_eq!(ft.enum_values().unwrap(), &["x", "y"]);
    }

    // ── TypeMapper trait with complex schemas ────────────────────

    #[test]
    fn default_type_mapper_maps_array_schema() {
        let mapper = DefaultTypeMapper;
        let s = Schema {
            schema_type: Some("array".to_string()),
            items: Some(Box::new(Schema {
                schema_type: Some("boolean".to_string()),
                ..Default::default()
            })),
            ..Default::default()
        };
        assert_eq!(
            mapper.map_schema(&s),
            FieldType::Array(Box::new(FieldType::Boolean))
        );
    }

    #[test]
    fn default_type_mapper_maps_ref_schema() {
        let mapper = DefaultTypeMapper;
        let s = Schema {
            ref_path: Some("#/components/schemas/Widget".to_string()),
            ..Default::default()
        };
        assert_eq!(
            mapper.map_schema(&s),
            FieldType::Object("Widget".to_string())
        );
    }

    // ── FieldType depth edge cases ──────────────────────────────

    #[test]
    fn field_type_depth_map() {
        assert_eq!(FieldType::Map(Box::new(FieldType::String)).depth(), 1);
    }

    #[test]
    fn field_type_depth_deeply_nested() {
        let ft = FieldType::Array(Box::new(FieldType::Array(Box::new(FieldType::Map(
            Box::new(FieldType::Integer),
        )))));
        assert_eq!(ft.depth(), 3);
    }

    #[test]
    fn field_type_depth_enum_with_underlying_array() {
        let e = FieldType::Enum {
            values: vec!["a".into()],
            underlying: Box::new(FieldType::Array(Box::new(FieldType::String))),
        };
        // Enum depth delegates to underlying
        assert_eq!(e.depth(), 1);
    }

    // ── Serde roundtrip for complex nested types ────────────────

    #[test]
    fn serde_roundtrip_enum_with_underlying_integer() {
        let ft = FieldType::Enum {
            values: vec!["1".into(), "2".into(), "3".into()],
            underlying: Box::new(FieldType::Integer),
        };
        let json = serde_json::to_string(&ft).unwrap();
        let back: FieldType = serde_json::from_str(&json).unwrap();
        assert_eq!(ft, back);
    }

    #[test]
    fn serde_roundtrip_deeply_nested() {
        let ft = FieldType::Map(Box::new(FieldType::Array(Box::new(FieldType::Map(
            Box::new(FieldType::Object("Deep".to_string())),
        )))));
        let json = serde_json::to_string(&ft).unwrap();
        let back: FieldType = serde_json::from_str(&json).unwrap();
        assert_eq!(ft, back);
    }

    // ── FromStr edge cases ──────────────────────────────────────

    #[test]
    fn field_type_from_str_empty_string_is_object() {
        let ft: FieldType = "".parse().unwrap();
        assert_eq!(ft, FieldType::Object(String::new()));
    }

    #[test]
    fn field_type_from_str_case_sensitive() {
        // "string" (lowercase) is not "String", so it becomes Object
        let ft: FieldType = "string".parse().unwrap();
        assert_eq!(ft, FieldType::Object("string".to_string()));
    }

    // ── TypeMapper map_override edge cases ───────────────────────

    #[test]
    fn default_type_mapper_override_all_aliases() {
        let mapper = DefaultTypeMapper;
        // Verify all documented aliases
        let bool_aliases = ["bool", "boolean"];
        for alias in &bool_aliases {
            assert_eq!(
                mapper.map_override(alias),
                Some(FieldType::Boolean),
                "alias '{alias}' should map to Boolean"
            );
        }
        let int_aliases = ["int", "int64", "integer"];
        for alias in &int_aliases {
            assert_eq!(
                mapper.map_override(alias),
                Some(FieldType::Integer),
                "alias '{alias}' should map to Integer"
            );
        }
        let num_aliases = ["float", "float64", "number"];
        for alias in &num_aliases {
            assert_eq!(
                mapper.map_override(alias),
                Some(FieldType::Number),
                "alias '{alias}' should map to Number"
            );
        }
    }
}
