use sekkei::{OpenApiSpec, all_operations};

use crate::field_type::FieldType;

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

impl ResolvedSpec {
    /// Find an operation by its `operation_id`.
    #[must_use]
    pub fn find_operation(&self, operation_id: &str) -> Option<&ResolvedOp> {
        self.operations
            .iter()
            .find(|op| op.id.as_deref() == Some(operation_id))
    }

    /// Find all operations matching a given HTTP method.
    pub fn operations_by_method<'a>(&'a self, method: &'a str) -> impl Iterator<Item = &'a ResolvedOp> {
        self.operations
            .iter()
            .filter(move |op| op.method == method)
    }

    /// Find a schema by name.
    #[must_use]
    pub fn find_schema(&self, name: &str) -> Option<&ResolvedSchema> {
        self.schemas.get(name)
    }

    /// Returns `true` if the spec contains no operations and no schemas.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty() && self.schemas.is_empty()
    }
}

impl ResolvedOp {
    /// Returns `true` if the operation has a request body.
    #[must_use]
    pub fn has_body(&self) -> bool {
        self.request_body.is_some()
    }

    /// Returns parameters filtered by location (e.g. "path", "query", "header").
    pub fn params_by_location<'a>(&'a self, location: &'a str) -> impl Iterator<Item = &'a ResolvedParam> {
        self.parameters
            .iter()
            .filter(move |p| p.location == location)
    }
}

impl ResolvedSchema {
    /// Returns required fields.
    pub fn required_fields(&self) -> impl Iterator<Item = &ResolvedField> {
        self.fields.iter().filter(|f| f.required)
    }

    /// Returns optional fields.
    pub fn optional_fields(&self) -> impl Iterator<Item = &ResolvedField> {
        self.fields.iter().filter(|f| !f.required)
    }
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
                let field_type = FieldType::from(field_schema);
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
        .map_or(FieldType::Any, FieldType::from);
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

    let field_type = FieldType::from(schema);
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
    Some(FieldType::from(schema))
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

    #[test]
    fn resolve_operation_summary_and_description() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        let list = resolved
            .operations
            .iter()
            .find(|o| o.id.as_deref() == Some("listPets"))
            .unwrap();
        assert_eq!(list.summary.as_deref(), Some("List all pets"));
    }

    #[test]
    fn resolve_create_pet_summary() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        let create = resolved
            .operations
            .iter()
            .find(|o| o.id.as_deref() == Some("createPet"))
            .unwrap();
        assert_eq!(create.summary.as_deref(), Some("Create a pet"));
    }

    #[test]
    fn resolve_operation_with_description() {
        let yaml = r#"
info:
  title: Desc Test
  version: "1.0"
paths:
  /items:
    get:
      operationId: listItems
      summary: List items
      description: Returns a paginated list of items
      responses:
        "200":
          description: OK
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        let op = &resolved.operations[0];
        assert_eq!(op.summary.as_deref(), Some("List items"));
        assert_eq!(
            op.description.as_deref(),
            Some("Returns a paginated list of items")
        );
    }

    #[test]
    fn resolve_param_dedup_operation_over_path() {
        let yaml = r##"
info:
  title: Dedup Test
  version: "1.0"
paths:
  /items/{id}:
    parameters:
      - name: id
        in: path
        required: true
        schema:
          type: string
    get:
      operationId: getItem
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: integer
      responses:
        "200":
          description: OK
"##;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        let op = &resolved.operations[0];
        assert_eq!(op.parameters.len(), 1, "duplicated param should be deduped");
        assert_eq!(op.parameters[0].name, "id");
        assert_eq!(
            op.parameters[0].field_type,
            FieldType::String,
            "path-level param takes precedence"
        );
    }

    #[test]
    fn resolve_param_without_schema_is_any() {
        let yaml = r#"
info:
  title: NoSchema Test
  version: "1.0"
paths:
  /items:
    get:
      operationId: listItems
      parameters:
        - name: filter
          in: query
          required: false
      responses:
        "200":
          description: OK
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        let op = &resolved.operations[0];
        assert_eq!(op.parameters[0].field_type, FieldType::Any);
    }

    #[test]
    fn resolve_param_description() {
        let yaml = r#"
info:
  title: ParamDesc Test
  version: "1.0"
paths:
  /items:
    get:
      operationId: listItems
      parameters:
        - name: limit
          in: query
          required: false
          description: Maximum number of items
          schema:
            type: integer
      responses:
        "200":
          description: OK
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        assert_eq!(
            resolved.operations[0].parameters[0].description.as_deref(),
            Some("Maximum number of items")
        );
    }

    #[test]
    fn resolve_response_ref() {
        let yaml = r##"
info:
  title: ResponseRef Test
  version: "1.0"
paths:
  /items:
    get:
      operationId: listItems
      responses:
        "200":
          $ref: "#/components/responses/ItemList"
components:
  responses:
    ItemList:
      description: A list of items
      content:
        application/json:
          schema:
            type: array
            items:
              type: string
"##;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        let op = &resolved.operations[0];
        assert_eq!(
            op.response_type,
            Some(FieldType::Array(Box::new(FieldType::String)))
        );
    }

    #[test]
    fn resolve_201_response() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        let create = resolved
            .operations
            .iter()
            .find(|o| o.id.as_deref() == Some("createPet"))
            .unwrap();
        assert_eq!(
            create.response_type,
            Some(FieldType::Object("Pet".to_string()))
        );
    }

    #[test]
    fn resolve_no_request_body() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        let list = resolved
            .operations
            .iter()
            .find(|o| o.id.as_deref() == Some("listPets"))
            .unwrap();
        assert!(list.request_body.is_none());
    }

    #[test]
    fn resolve_request_body_description() {
        let yaml = r#"
info:
  title: BodyDesc Test
  version: "1.0"
paths:
  /items:
    post:
      operationId: createItem
      requestBody:
        required: true
        description: The item to create
        content:
          application/json:
            schema:
              type: object
              properties:
                name:
                  type: string
      responses:
        "201":
          description: Created
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        let body = resolved.operations[0].request_body.as_ref().unwrap();
        assert_eq!(body.description.as_deref(), Some("The item to create"));
    }

    #[test]
    fn resolve_schema_description() {
        let yaml = r#"
info:
  title: SchemaDesc Test
  version: "1.0"
paths: {}
components:
  schemas:
    Widget:
      type: object
      description: A widget resource
      properties:
        id:
          type: integer
          description: Unique identifier
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        let widget = &resolved.schemas["Widget"];
        assert_eq!(widget.description.as_deref(), Some("A widget resource"));
        let id_field = widget.fields.iter().find(|f| f.name == "id").unwrap();
        assert_eq!(id_field.description.as_deref(), Some("Unique identifier"));
    }

    #[test]
    fn resolve_schema_required_vs_optional() {
        let yaml = r#"
info:
  title: Required Test
  version: "1.0"
paths: {}
components:
  schemas:
    User:
      type: object
      required:
        - name
      properties:
        name:
          type: string
        email:
          type: string
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        let user = &resolved.schemas["User"];
        let name_field = user.fields.iter().find(|f| f.name == "name").unwrap();
        assert!(name_field.required);
        let email_field = user.fields.iter().find(|f| f.name == "email").unwrap();
        assert!(!email_field.required);
    }

    #[test]
    fn resolve_operation_no_params() {
        let yaml = r#"
info:
  title: NoParams Test
  version: "1.0"
paths:
  /health:
    get:
      operationId: healthCheck
      responses:
        "200":
          description: OK
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        assert!(resolved.operations[0].parameters.is_empty());
    }

    #[test]
    fn resolve_multiple_methods_same_path() {
        let yaml = r#"
info:
  title: MultiMethod Test
  version: "1.0"
paths:
  /items:
    get:
      operationId: listItems
      responses:
        "200":
          description: OK
    post:
      operationId: createItem
      responses:
        "201":
          description: Created
    put:
      operationId: replaceItems
      responses:
        "200":
          description: OK
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        assert_eq!(resolved.operations.len(), 3);
        let methods: Vec<&str> = resolved.operations.iter().map(|o| o.method.as_str()).collect();
        assert!(methods.contains(&"get"));
        assert!(methods.contains(&"post"));
        assert!(methods.contains(&"put"));
    }

    #[test]
    fn resolve_patch_method() {
        let yaml = r#"
info:
  title: Patch Test
  version: "1.0"
paths:
  /items/{id}:
    patch:
      operationId: patchItem
      responses:
        "200":
          description: OK
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        assert_eq!(resolved.operations[0].method, "patch");
    }

    #[test]
    fn resolve_no_tags() {
        let yaml = r#"
info:
  title: NoTags Test
  version: "1.0"
paths:
  /items:
    get:
      operationId: listItems
      responses:
        "200":
          description: OK
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        assert!(resolved.operations[0].tags.is_empty());
    }

    #[test]
    fn resolve_spec_no_components() {
        let yaml = r#"
info:
  title: No Components
  version: "1.0"
paths:
  /items:
    get:
      operationId: listItems
      responses:
        "200":
          description: OK
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        assert!(resolved.schemas.is_empty());
        assert_eq!(resolved.operations.len(), 1);
    }

    #[test]
    fn resolve_request_body_non_json_content_type() {
        let yaml = r#"
info:
  title: NonJSON Body Test
  version: "1.0"
paths:
  /upload:
    post:
      operationId: uploadFile
      requestBody:
        required: true
        content:
          multipart/form-data:
            schema:
              type: object
              properties:
                file:
                  type: string
      responses:
        "200":
          description: OK
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        assert!(
            resolved.operations[0].request_body.is_none(),
            "non-JSON content type should not resolve a body"
        );
    }

    #[test]
    fn resolve_response_non_json_content() {
        let yaml = r#"
info:
  title: NonJSON Response Test
  version: "1.0"
paths:
  /download:
    get:
      operationId: downloadFile
      responses:
        "200":
          description: A file
          content:
            application/octet-stream:
              schema:
                type: string
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        assert!(
            resolved.operations[0].response_type.is_none(),
            "non-JSON response should not resolve a type"
        );
    }

    #[test]
    fn resolve_parameter_ref_from_components() {
        let yaml = r##"
info:
  title: ParamRef Test
  version: "1.0"
paths:
  /items:
    get:
      operationId: listItems
      parameters:
        - name: ""
          in: ""
          $ref: "#/components/parameters/LimitParam"
      responses:
        "200":
          description: OK
components:
  parameters:
    LimitParam:
      name: limit
      in: query
      required: false
      schema:
        type: integer
"##;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        let op = &resolved.operations[0];
        assert_eq!(op.parameters.len(), 1);
        assert_eq!(op.parameters[0].name, "limit");
        assert_eq!(op.parameters[0].location, "query");
        assert_eq!(op.parameters[0].field_type, FieldType::Integer);
    }

    #[test]
    fn resolve_multiple_schemas() {
        let yaml = r#"
info:
  title: MultiSchema Test
  version: "1.0"
paths: {}
components:
  schemas:
    Alpha:
      type: object
      properties:
        a:
          type: string
    Beta:
      type: object
      required:
        - b
      properties:
        b:
          type: integer
    Gamma:
      type: object
      properties:
        c:
          type: boolean
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        assert_eq!(resolved.schemas.len(), 3);
        assert!(resolved.schemas.contains_key("Alpha"));
        assert!(resolved.schemas.contains_key("Beta"));
        assert!(resolved.schemas.contains_key("Gamma"));
        assert!(resolved.schemas["Beta"].fields[0].required);
    }

    #[test]
    fn resolve_schema_name_matches_key() {
        let yaml = r#"
info:
  title: NameMatch Test
  version: "1.0"
paths: {}
components:
  schemas:
    MyModel:
      type: object
      properties:
        x:
          type: string
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        let schema = &resolved.schemas["MyModel"];
        assert_eq!(schema.name, "MyModel");
    }

    #[test]
    fn resolve_complex_spec_with_all_features() {
        let yaml = r##"
info:
  title: Complex API
  version: "2.0"
paths:
  /users:
    get:
      operationId: listUsers
      summary: List all users
      tags:
        - users
      parameters:
        - name: page
          in: query
          required: false
          schema:
            type: integer
        - name: size
          in: query
          required: false
          schema:
            type: integer
      responses:
        "200":
          description: User list
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: "#/components/schemas/User"
    post:
      operationId: createUser
      tags:
        - users
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: "#/components/schemas/CreateUser"
      responses:
        "201":
          description: Created
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/User"
  /users/{userId}:
    parameters:
      - name: userId
        in: path
        required: true
        schema:
          type: string
    get:
      operationId: getUser
      tags:
        - users
      responses:
        "200":
          description: A user
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/User"
    put:
      operationId: updateUser
      tags:
        - users
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: "#/components/schemas/CreateUser"
      responses:
        "200":
          description: Updated
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/User"
    delete:
      operationId: deleteUser
      tags:
        - users
      responses:
        "204":
          description: Deleted
components:
  schemas:
    User:
      type: object
      required:
        - id
        - email
      properties:
        id:
          type: string
        email:
          type: string
        name:
          type: string
        role:
          type: string
          enum:
            - admin
            - user
            - guest
    CreateUser:
      type: object
      required:
        - email
      properties:
        email:
          type: string
        name:
          type: string
"##;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        assert_eq!(resolved.operations.len(), 5);
        assert_eq!(resolved.schemas.len(), 2);

        let list = resolved
            .operations
            .iter()
            .find(|o| o.id.as_deref() == Some("listUsers"))
            .unwrap();
        assert_eq!(list.parameters.len(), 2);
        assert_eq!(
            list.response_type,
            Some(FieldType::Array(Box::new(FieldType::Object(
                "User".to_string()
            ))))
        );

        let get = resolved
            .operations
            .iter()
            .find(|o| o.id.as_deref() == Some("getUser"))
            .unwrap();
        assert_eq!(get.parameters.len(), 1);
        assert_eq!(get.parameters[0].name, "userId");

        let delete = resolved
            .operations
            .iter()
            .find(|o| o.id.as_deref() == Some("deleteUser"))
            .unwrap();
        assert!(delete.response_type.is_none());
        assert!(delete.request_body.is_none());

        let user_schema = &resolved.schemas["User"];
        assert_eq!(user_schema.fields.len(), 4);
        let role_field = user_schema.fields.iter().find(|f| f.name == "role").unwrap();
        assert_eq!(
            role_field.field_type,
            FieldType::Enum {
                values: vec![
                    "admin".to_string(),
                    "user".to_string(),
                    "guest".to_string()
                ],
                underlying: Box::new(FieldType::String),
            }
        );
    }

    #[test]
    fn resolve_path_and_op_params_combined() {
        let yaml = r##"
info:
  title: Combined Params
  version: "1.0"
paths:
  /items/{id}:
    parameters:
      - name: id
        in: path
        required: true
        schema:
          type: string
    get:
      operationId: getItem
      parameters:
        - name: fields
          in: query
          required: false
          schema:
            type: string
      responses:
        "200":
          description: OK
"##;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        let op = &resolved.operations[0];
        assert_eq!(op.parameters.len(), 2);
        let names: Vec<&str> = op.parameters.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"id"));
        assert!(names.contains(&"fields"));
    }

    #[test]
    fn resolve_response_without_content() {
        let yaml = r#"
info:
  title: NoContent Test
  version: "1.0"
paths:
  /ping:
    get:
      operationId: ping
      responses:
        "200":
          description: Pong
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        assert!(resolved.operations[0].response_type.is_none());
    }

    #[test]
    fn resolve_response_without_schema() {
        let yaml = r#"
info:
  title: NoSchema Response
  version: "1.0"
paths:
  /ping:
    get:
      operationId: ping
      responses:
        "200":
          description: OK
          content:
            application/json: {}
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        assert!(resolved.operations[0].response_type.is_none());
    }

    // ── ResolvedSpec helpers ────────────────────────────────────

    #[test]
    fn resolved_spec_find_operation() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        assert!(resolved.find_operation("listPets").is_some());
        assert!(resolved.find_operation("nonexistent").is_none());
    }

    #[test]
    fn resolved_spec_operations_by_method() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        let gets: Vec<_> = resolved.operations_by_method("get").collect();
        assert_eq!(gets.len(), 2);
        let posts: Vec<_> = resolved.operations_by_method("post").collect();
        assert_eq!(posts.len(), 1);
    }

    #[test]
    fn resolved_spec_find_schema() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        assert!(resolved.find_schema("Pet").is_some());
        assert!(resolved.find_schema("Missing").is_none());
    }

    #[test]
    fn resolved_spec_is_empty() {
        let yaml = r#"
info:
  title: Empty
  version: "1.0.0"
paths: {}
"#;
        let spec: OpenApiSpec = serde_yaml_ng::from_str(yaml).unwrap();
        let resolved = resolve(&spec);
        assert!(resolved.is_empty());

        let full = resolve(&load_pet_store());
        assert!(!full.is_empty());
    }

    // ── ResolvedOp helpers ──────────────────────────────────────

    #[test]
    fn resolved_op_has_body() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        let create = resolved.find_operation("createPet").unwrap();
        assert!(create.has_body());
        let list = resolved.find_operation("listPets").unwrap();
        assert!(!list.has_body());
    }

    #[test]
    fn resolved_op_params_by_location() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        let list = resolved.find_operation("listPets").unwrap();
        let query_params: Vec<_> = list.params_by_location("query").collect();
        assert_eq!(query_params.len(), 1);
        assert_eq!(query_params[0].name, "limit");

        let get_pet = resolved.find_operation("getPet").unwrap();
        let path_params: Vec<_> = get_pet.params_by_location("path").collect();
        assert_eq!(path_params.len(), 1);
        assert_eq!(path_params[0].name, "petId");
    }

    // ── ResolvedSchema helpers ──────────────────────────────────

    #[test]
    fn resolved_schema_required_fields() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        let pet = resolved.find_schema("Pet").unwrap();
        let required: Vec<_> = pet.required_fields().collect();
        assert_eq!(required.len(), 2);
        assert!(required.iter().all(|f| f.required));
    }

    #[test]
    fn resolved_schema_optional_fields() {
        let spec = load_pet_store();
        let resolved = resolve(&spec);
        let pet = resolved.find_schema("Pet").unwrap();
        let optional: Vec<_> = pet.optional_fields().collect();
        assert_eq!(optional.len(), 1);
        assert!(optional.iter().all(|f| !f.required));
    }
}
