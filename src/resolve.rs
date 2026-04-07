use sekkei::{OpenApiSpec, all_operations};

use crate::field_type::{FieldType, schema_to_field_type};

/// A fully resolved operation with typed parameters and response.
#[derive(Debug, Clone)]
pub struct ResolvedOp {
    pub id: Option<String>,
    pub method: String,
    pub path: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub parameters: Vec<ResolvedParam>,
    pub request_body: Option<ResolvedBody>,
    pub response_type: Option<FieldType>,
    pub tags: Vec<String>,
}

/// A resolved parameter with typed field.
#[derive(Debug, Clone)]
pub struct ResolvedParam {
    pub name: String,
    pub location: String,
    pub required: bool,
    pub description: Option<String>,
    pub field_type: FieldType,
}

/// A resolved request body.
#[derive(Debug, Clone)]
pub struct ResolvedBody {
    pub required: bool,
    pub field_type: FieldType,
    pub description: Option<String>,
}

/// A fully resolved schema with named fields.
#[derive(Debug, Clone)]
pub struct ResolvedSchema {
    pub name: String,
    pub fields: Vec<ResolvedField>,
    pub description: Option<String>,
}

/// A named, typed field in a resolved schema.
#[derive(Debug, Clone)]
pub struct ResolvedField {
    pub name: String,
    pub field_type: FieldType,
    pub required: bool,
    pub description: Option<String>,
}

/// Complete resolved spec.
#[derive(Debug, Clone)]
pub struct ResolvedSpec {
    pub operations: Vec<ResolvedOp>,
    pub schemas: indexmap::IndexMap<String, ResolvedSchema>,
}

/// Resolve an `OpenApiSpec` into typed operations and schemas.
#[must_use]
pub fn resolve(spec: &OpenApiSpec) -> ResolvedSpec {
    let mut operations = Vec::new();

    for (method, path, op) in all_operations(spec) {
        let mut parameters = Vec::new();

        if let Some(path_item) = spec.paths.get(&path) {
            for param in &path_item.parameters {
                if let Some(rp) = resolve_param(spec, param) {
                    parameters.push(rp);
                }
            }
        }

        for param in &op.parameters {
            if let Some(rp) = resolve_param(spec, param) {
                let already_present = parameters
                    .iter()
                    .any(|existing| existing.name == rp.name && existing.location == rp.location);
                if !already_present {
                    parameters.push(rp);
                }
            }
        }

        // Request body.
        let request_body = resolve_request_body(spec, op);

        // Response type (from 200/201 response).
        let response_type = resolve_response_type(spec, op);

        operations.push(ResolvedOp {
            id: op.operation_id.clone(),
            method,
            path,
            summary: op.summary.clone(),
            description: op.description.clone(),
            parameters,
            request_body,
            response_type,
            tags: op.tags.clone(),
        });
    }

    // Resolve schemas.
    let mut schemas = indexmap::IndexMap::new();
    if let Some(components) = &spec.components {
        for (name, schema) in &components.schemas {
            let mut fields = Vec::new();
            for (field_name, field_schema) in &schema.properties {
                let field_type = schema_to_field_type(field_schema);
                let required = schema.required.contains(field_name);
                fields.push(ResolvedField {
                    name: field_name.clone(),
                    field_type,
                    required,
                    description: field_schema.description.clone(),
                });
            }
            schemas.insert(
                name.clone(),
                ResolvedSchema {
                    name: name.clone(),
                    fields,
                    description: schema.description.clone(),
                },
            );
        }
    }

    ResolvedSpec {
        operations,
        schemas,
    }
}

fn resolve_param(spec: &OpenApiSpec, param: &sekkei::Parameter) -> Option<ResolvedParam> {
    let p = if let Some(ref_path) = &param.ref_path {
        spec.resolve_parameter_ref(ref_path)?
    } else {
        param
    };
    let field_type = p
        .schema
        .as_ref()
        .map_or(FieldType::Any, schema_to_field_type);
    Some(ResolvedParam {
        name: p.name.clone(),
        location: p.location.clone(),
        required: p.required,
        description: p.description.clone(),
        field_type,
    })
}

fn resolve_request_body(
    spec: &OpenApiSpec,
    op: &sekkei::Operation,
) -> Option<ResolvedBody> {
    let body = op.request_body.as_ref()?;

    // Handle $ref request bodies.
    let actual_body = if let Some(ref_path) = &body.ref_path {
        spec.resolve_request_body_ref(ref_path)?
    } else {
        body
    };

    let schema = actual_body
        .content
        .get("application/json")
        .and_then(|mt| mt.schema.as_ref())?;

    let field_type = schema_to_field_type(schema);
    Some(ResolvedBody {
        required: actual_body.required,
        field_type,
        description: actual_body.description.clone(),
    })
}

fn resolve_response_type(spec: &OpenApiSpec, op: &sekkei::Operation) -> Option<FieldType> {
    // Look for 200 or 201 response.
    let response = op
        .responses
        .get("200")
        .or_else(|| op.responses.get("201"))?;

    // Handle $ref responses.
    let actual_response = if let Some(ref_path) = &response.ref_path {
        spec.resolve_response_ref(ref_path)?
    } else {
        response
    };

    let content = actual_response.content.as_ref()?;
    let schema = content.get("application/json")?.schema.as_ref()?;
    Some(schema_to_field_type(schema))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PET_STORE_YAML: &str = r##"
info:
  title: Pet Store
  version: "1.0.0"
paths:
  /pets:
    get:
      operationId: listPets
      summary: List all pets
      tags:
        - pets
      parameters:
        - name: limit
          in: query
          required: false
          schema:
            type: integer
      responses:
        "200":
          description: A list of pets
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: "#/components/schemas/Pet"
    post:
      operationId: createPet
      summary: Create a pet
      tags:
        - pets
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: "#/components/schemas/CreatePetRequest"
      responses:
        "201":
          description: Pet created
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/Pet"
  /pets/{petId}:
    parameters:
      - name: petId
        in: path
        required: true
        schema:
          type: string
    get:
      operationId: getPet
      responses:
        "200":
          description: A pet
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/Pet"
    delete:
      operationId: deletePet
      responses:
        "204":
          description: Deleted
components:
  schemas:
    Pet:
      type: object
      required:
        - id
        - name
      properties:
        id:
          type: integer
        name:
          type: string
        status:
          type: string
          enum:
            - available
            - sold
    CreatePetRequest:
      type: object
      required:
        - name
      properties:
        name:
          type: string
"##;

    fn load_pet_store() -> OpenApiSpec {
        serde_yaml_ng::from_str(PET_STORE_YAML).unwrap()
    }

    #[test]
    fn resolve_operations_count() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        // GET /pets, POST /pets, GET /pets/{petId}, DELETE /pets/{petId}
        assert_eq!(resolved.operations.len(), 4);
    }

    #[test]
    fn resolve_list_pets() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        let list = resolved
            .operations
            .iter()
            .find(|o| o.id.as_deref() == Some("listPets"))
            .unwrap();
        assert_eq!(list.method, "get");
        assert_eq!(list.path, "/pets");
        assert_eq!(list.parameters.len(), 1);
        assert_eq!(list.parameters[0].name, "limit");
        assert_eq!(list.parameters[0].field_type, FieldType::Integer);
        assert!(!list.parameters[0].required);
        assert!(list.request_body.is_none());
        assert_eq!(
            list.response_type,
            Some(FieldType::Array(Box::new(FieldType::Object(
                "Pet".to_string()
            ))))
        );
    }

    #[test]
    fn resolve_create_pet() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        let create = resolved
            .operations
            .iter()
            .find(|o| o.id.as_deref() == Some("createPet"))
            .unwrap();
        assert_eq!(create.method, "post");
        let body = create.request_body.as_ref().unwrap();
        assert!(body.required);
        assert_eq!(
            body.field_type,
            FieldType::Object("CreatePetRequest".to_string())
        );
        assert_eq!(
            create.response_type,
            Some(FieldType::Object("Pet".to_string()))
        );
    }

    #[test]
    fn resolve_path_level_params() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        let get_pet = resolved
            .operations
            .iter()
            .find(|o| o.id.as_deref() == Some("getPet"))
            .unwrap();
        assert_eq!(get_pet.parameters.len(), 1);
        assert_eq!(get_pet.parameters[0].name, "petId");
        assert_eq!(get_pet.parameters[0].location, "path");
        assert!(get_pet.parameters[0].required);
    }

    #[test]
    fn resolve_schemas() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        assert_eq!(resolved.schemas.len(), 2);
        let pet = &resolved.schemas["Pet"];
        assert_eq!(pet.fields.len(), 3);
        let id_field = pet.fields.iter().find(|f| f.name == "id").unwrap();
        assert_eq!(id_field.field_type, FieldType::Integer);
        assert!(id_field.required);
        let status_field = pet.fields.iter().find(|f| f.name == "status").unwrap();
        assert_eq!(
            status_field.field_type,
            FieldType::Enum {
                values: vec!["available".to_string(), "sold".to_string()],
                underlying: Box::new(FieldType::String),
            }
        );
    }

    #[test]
    fn resolve_no_response_type() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        let delete = resolved
            .operations
            .iter()
            .find(|o| o.id.as_deref() == Some("deletePet"))
            .unwrap();
        assert!(delete.response_type.is_none());
    }

    #[test]
    fn resolve_tags() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        let list = resolved
            .operations
            .iter()
            .find(|o| o.id.as_deref() == Some("listPets"))
            .unwrap();
        assert_eq!(list.tags, vec!["pets"]);
    }

    #[test]
    fn resolve_empty_spec() {
        let yaml = r#"
info:
  title: Empty
  version: "1.0.0"
paths: {}
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        assert!(resolved.operations.is_empty());
        assert!(resolved.schemas.is_empty());
    }

    // ── Resolve edge cases ──────────────────────────────────────

    #[test]
    fn resolve_spec_with_schema_ref_parameter() {
        let yaml = r##"
info:
  title: SchemaRef Test
  version: "1.0"
paths:
  /items:
    get:
      operationId: listItems
      parameters:
        - name: filter
          in: query
          required: false
          schema:
            $ref: "#/components/schemas/Filter"
      responses:
        "200":
          description: OK
components:
  schemas:
    Filter:
      type: object
      properties:
        name:
          type: string
"##;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        let list_op = &resolved.operations[0];
        assert_eq!(list_op.parameters.len(), 1);
        assert_eq!(list_op.parameters[0].name, "filter");
        assert_eq!(
            list_op.parameters[0].field_type,
            FieldType::Object("Filter".to_string())
        );
    }

    #[test]
    fn resolve_spec_with_ref_request_body() {
        let yaml = r##"
info:
  title: RefBody Test
  version: "1.0"
paths:
  /items:
    post:
      operationId: createItem
      requestBody:
        $ref: "#/components/requestBodies/ItemBody"
      responses:
        "201":
          description: Created
components:
  requestBodies:
    ItemBody:
      required: true
      content:
        application/json:
          schema:
            type: object
            properties:
              name:
                type: string
"##;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        let create_op = &resolved.operations[0];
        assert!(create_op.request_body.is_some());
        assert!(create_op.request_body.as_ref().unwrap().required);
    }

    #[test]
    fn resolve_spec_with_multiple_tags() {
        let yaml = r#"
info:
  title: Multi-tag Test
  version: "1.0"
paths:
  /items:
    get:
      operationId: listItems
      tags:
        - items
        - public
      responses:
        "200":
          description: OK
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        let list_op = &resolved.operations[0];
        assert_eq!(list_op.tags, vec!["items", "public"]);
    }

    #[test]
    fn resolve_spec_with_no_operation_id() {
        let yaml = r#"
info:
  title: No OpId Test
  version: "1.0"
paths:
  /items:
    get:
      responses:
        "200":
          description: OK
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        assert_eq!(resolved.operations.len(), 1);
        assert!(resolved.operations[0].id.is_none());
    }
}
