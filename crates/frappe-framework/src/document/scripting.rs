use rhai::{Engine, Scope, Dynamic, EvalAltResult};
use serde_json::Value;
use std::sync::RwLock;

/// ScriptError represents errors that can occur during script compilation, execution, or serialization.
#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    /// Rhai evaluation runtime error.
    #[error("Rhai evaluation error: {0}")]
    Rhai(#[from] Box<EvalAltResult>),
    /// Rhai compilation syntax error.
    #[error("Rhai compilation error: {0}")]
    Compilation(#[from] rhai::ParseError),
    /// Serde JSON serialization/deserialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    /// Error indicating access to a blocked sensitive system table.
    #[error("Access denied to sensitive table: {0}")]
    AccessDenied(String),
}

type DbResolverFn = dyn Fn(&str, &str, &str) -> Result<Value, String> + Send + Sync;

static DB_RESOLVER: RwLock<Option<Box<DbResolverFn>>> = RwLock::new(None);

/// Registers a global database resolver callback used by sandboxed scripts to query tables.
pub fn register_db_resolver<F>(resolver: F)
where
    F: Fn(&str, &str, &str) -> Result<Value, String> + Send + Sync + 'static,
{
    let mut lock = DB_RESOLVER.write().unwrap();
    *lock = Some(Box::new(resolver));
}

/// Helper function to check if a table is sensitive and access should be denied.
pub fn is_sensitive_table(table: &str) -> bool {
    let lower = table.to_lowercase();
    lower.contains("auth")
        || lower.contains("session")
        || lower.contains("user")
        || lower.contains("password")
        || lower.contains("lock")
}

/// Creates and configures a sandboxed Rhai engine with strict limits.
pub fn create_sandboxed_engine() -> Engine {
    let mut engine = Engine::new();

    // Enforce strict limits to prevent memory exhaustion and infinite loops
    engine.set_max_operations(5000);
    engine.set_max_call_levels(10);

    // Disable unsafe features
    engine.set_allow_anonymous_fn(false);

    // Register db_get_value function wrapper
    engine.register_fn(
        "db_get_value",
        |table: &str, id: &str, field_path: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            if is_sensitive_table(table) {
                return Err(Box::new(EvalAltResult::ErrorRuntime(
                    Dynamic::from(format!("Access denied to sensitive table: {}", table)),
                    rhai::Position::NONE,
                )));
            }

            let resolver_lock = DB_RESOLVER.read().unwrap();
            if let Some(ref resolver) = *resolver_lock {
                match resolver(table, id, field_path) {
                    Ok(val) => {
                        let dyn_val = rhai::serde::to_dynamic(val).map_err(|e| {
                            Box::new(EvalAltResult::ErrorRuntime(
                                Dynamic::from(e.to_string()),
                                rhai::Position::NONE,
                            ))
                        })?;
                        Ok(dyn_val)
                    }
                    Err(err) => Err(Box::new(EvalAltResult::ErrorRuntime(
                        Dynamic::from(err),
                        rhai::Position::NONE,
                    ))),
                }
            } else {
                Ok(Dynamic::from("mock_value"))
            }
        },
    );

    engine
}

/// Executes a lifecycle hook script on a document and updates its payload in place.
pub fn dispatch_lifecycle_event(
    engine: &Engine,
    script_src: &str,
    document_payload: &mut Value,
    event_type: &str,
) -> Result<(), ScriptError> {
    let mut scope = Scope::new();

    let dynamic_doc = rhai::serde::to_dynamic(document_payload.clone())?;
    scope.push("doc", dynamic_doc);
    scope.push("event_type", event_type.to_string());

    let ast = engine.compile(script_src)?;
    engine.run_ast_with_scope(&mut scope, &ast)?;

    if let Some(mutated_dynamic) = scope.get_value::<Dynamic>("doc") {
        let mutated_val: Value = rhai::serde::from_dynamic(&mutated_dynamic)?;
        *document_payload = mutated_val;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_lifecycle_event_discount() {
        let engine = create_sandboxed_engine();
        let mut doc = json!({
            "total_items": 12,
            "discount_percentage": 0.0,
        });

        let script = r#"
            if doc.total_items > 10 {
                doc.discount_percentage = 15.0;
            }
        "#;

        dispatch_lifecycle_event(&engine, script, &mut doc, "before_save").unwrap();
        assert_eq!(doc["discount_percentage"].as_f64(), Some(15.0));
    }

    #[test]
    fn test_sensitive_table_blocked() {
        let engine = create_sandboxed_engine();
        let script = r#"
            let val = db_get_value("tabUserAuth", "admin", "password");
        "#;
        let mut doc = json!({});
        let res = dispatch_lifecycle_event(&engine, script, &mut doc, "before_save");
        assert!(res.is_err());
        let err_str = res.err().unwrap().to_string();
        assert!(err_str.contains("Access denied to sensitive table"));
    }
}
