use crate::resolve::ResolvedOp;

/// A group of operations that form a CRUD set for a resource.
#[derive(Debug, Clone)]
pub struct CrudGroup {
    pub name: String,
    pub create: Option<ResolvedOp>,
    pub read: Option<ResolvedOp>,
    pub update: Option<ResolvedOp>,
    pub delete: Option<ResolvedOp>,
    pub list: Option<ResolvedOp>,
}

/// Trait for customizing CRUD grouping logic.
///
/// Default implementation groups by HTTP method + path patterns.
/// Consumers can override for custom resource grouping strategies.
pub trait CrudGrouper: Send + Sync {
    /// Group operations into CRUD sets.
    fn group(&self, ops: &[ResolvedOp]) -> Vec<CrudGroup>;
}

/// Default grouper based on HTTP method + path patterns.
pub struct PathBasedGrouper;

impl CrudGrouper for PathBasedGrouper {
    fn group(&self, ops: &[ResolvedOp]) -> Vec<CrudGroup> {
        group_crud(ops)
    }
}

/// Group resolved operations into CRUD sets based on HTTP method + path patterns.
///
/// Groups operations by path prefix (e.g. `/pets` and `/pets/{id}` share a group).
#[must_use]
pub fn group_crud(ops: &[ResolvedOp]) -> Vec<CrudGroup> {
    let mut groups: indexmap::IndexMap<String, CrudGroup> = indexmap::IndexMap::new();

    for op in ops {
        let base_path = extract_base_path(&op.path);
        let resource_name = path_to_resource_name(&base_path);
        let is_collection = !op.path.contains('{');

        let group = groups
            .entry(resource_name.clone())
            .or_insert_with(|| CrudGroup {
                name: resource_name,
                create: None,
                read: None,
                update: None,
                delete: None,
                list: None,
            });

        match (op.method.as_str(), is_collection) {
            ("get", true) => group.list = Some(op.clone()),
            ("get", false) => group.read = Some(op.clone()),
            ("post", _) => group.create = Some(op.clone()),
            ("put" | "patch", _) => group.update = Some(op.clone()),
            ("delete", _) => group.delete = Some(op.clone()),
            _ => {}
        }
    }

    groups.into_values().collect()
}

/// Extract the base path without parameter segments.
/// `/pets/{petId}/toys/{toyId}` -> `/pets`
fn extract_base_path(path: &str) -> String {
    let segments: Vec<&str> = path.split('/').collect();
    let mut base = Vec::new();
    for seg in &segments {
        if seg.starts_with('{') {
            break;
        }
        base.push(*seg);
    }
    let result = base.join("/");
    if result.is_empty() {
        "/".to_string()
    } else {
        result
    }
}

/// Convert a path to a resource name.
/// `/pets` -> `pets`, `/api/v1/users` -> `users`
fn path_to_resource_name(path: &str) -> String {
    path.rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or("root")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_op(method: &str, path: &str, id: &str) -> ResolvedOp {
        ResolvedOp {
            id: Some(id.to_string()),
            method: method.to_string(),
            path: path.to_string(),
            summary: None,
            description: None,
            parameters: vec![],
            request_body: None,
            response_type: None,
            tags: vec![],
        }
    }

    #[test]
    fn basic_crud_grouping() {
        let ops = vec![
            make_op("get", "/pets", "listPets"),
            make_op("post", "/pets", "createPet"),
            make_op("get", "/pets/{petId}", "getPet"),
            make_op("put", "/pets/{petId}", "updatePet"),
            make_op("delete", "/pets/{petId}", "deletePet"),
        ];
        let groups = group_crud(&ops);
        assert_eq!(groups.len(), 1);
        let g = &groups[0];
        assert_eq!(g.name, "pets");
        assert!(g.list.is_some());
        assert!(g.create.is_some());
        assert!(g.read.is_some());
        assert!(g.update.is_some());
        assert!(g.delete.is_some());
    }

    #[test]
    fn multiple_resources() {
        let ops = vec![
            make_op("get", "/pets", "listPets"),
            make_op("get", "/users", "listUsers"),
        ];
        let groups = group_crud(&ops);
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn extract_base_path_simple() {
        assert_eq!(extract_base_path("/pets"), "/pets");
        assert_eq!(extract_base_path("/pets/{petId}"), "/pets");
        assert_eq!(extract_base_path("/api/v1/pets/{petId}"), "/api/v1/pets");
    }

    #[test]
    fn path_to_resource_name_simple() {
        assert_eq!(path_to_resource_name("/pets"), "pets");
        assert_eq!(path_to_resource_name("/api/v1/users"), "users");
        assert_eq!(path_to_resource_name("/"), "root");
    }

    #[test]
    fn empty_ops() {
        let groups = group_crud(&[]);
        assert!(groups.is_empty());
    }

    #[test]
    fn patch_treated_as_update() {
        let ops = vec![make_op("patch", "/pets/{petId}", "patchPet")];
        let groups = group_crud(&ops);
        assert!(groups[0].update.is_some());
    }

    // ── CrudGrouper trait ────────────────────────────────────────

    #[test]
    fn path_based_grouper_delegates_to_group_crud() {
        let grouper = PathBasedGrouper;
        let ops = vec![
            make_op("get", "/pets", "listPets"),
            make_op("post", "/pets", "createPet"),
            make_op("get", "/pets/{petId}", "getPet"),
        ];
        let groups = grouper.group(&ops);
        assert_eq!(groups.len(), 1);
        assert!(groups[0].list.is_some());
        assert!(groups[0].create.is_some());
        assert!(groups[0].read.is_some());
    }

    // ── CRUD edge cases ─────────────────────────────────────────

    #[test]
    fn crud_nested_path() {
        let ops = vec![
            make_op("get", "/users/{userId}/posts", "listUserPosts"),
            make_op("post", "/users/{userId}/posts", "createUserPost"),
        ];
        let groups = group_crud(&ops);
        assert!(!groups.is_empty());
        // Nested resources group under the base path before the first param
        let user_group = groups.iter().find(|g| g.name == "users");
        assert!(user_group.is_some());
    }

    #[test]
    fn crud_api_versioned_path() {
        let ops = vec![
            make_op("get", "/api/v1/items", "listItems"),
            make_op("get", "/api/v1/items/{id}", "getItem"),
            make_op("delete", "/api/v1/items/{id}", "deleteItem"),
        ];
        let groups = group_crud(&ops);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "items");
        assert!(groups[0].list.is_some());
        assert!(groups[0].read.is_some());
        assert!(groups[0].delete.is_some());
    }

    #[test]
    fn crud_single_operation() {
        let ops = vec![make_op("get", "/health", "healthCheck")];
        let groups = group_crud(&ops);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "health");
        assert!(groups[0].list.is_some()); // GET on collection path
    }
}
