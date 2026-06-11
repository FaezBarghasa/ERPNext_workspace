use rhai::{Engine, Scope, AST, Map, Dynamic};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct ScriptingEngine {
    engine: Engine,
    ast_cache: Mutex<HashMap<String, AST>>,
}

impl ScriptingEngine {
    pub fn new() -> Self {
        let mut engine = Engine::new();

        // 1. Enforce strict sandboxed operation limits for low overhead on RPi 5
        engine.set_max_operations(5000);

        engine.register_fn("log", |msg: &str| {
            println!("[Rhai Sandbox Log] {}", msg);
        });

        Self {
            engine,
            ast_cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn execute_lifecycle(
        &self,
        script_id: &str,
        script_src: &str,
        doc: Value,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        // 2. Fetch from cache or compile and store to avoid re-compilation CPU overhead
        let ast = {
            let mut cache = self.ast_cache.lock().unwrap();
            if let Some(cached_ast) = cache.get(script_id) {
                cached_ast.clone()
            } else {
                let compiled = self.engine.compile(script_src)?;
                cache.insert(script_id.to_string(), compiled.clone());
                compiled
            }
        };

        let mut scope = Scope::new();

        // Convert Serde JSON to Rhai compatible Dynamic objects
        let mut map = Map::new();
        if let Some(obj) = doc.as_object() {
            for (k, v) in obj {
                let val: Dynamic = match v {
                    Value::String(s) => Dynamic::from(s.clone()),
                    Value::Number(n) => {
                        if let Some(i) = n.as_i64() { Dynamic::from(i) }
                        else { Dynamic::from(n.as_f64().unwrap_or(0.0)) }
                    }
                    Value::Bool(b) => Dynamic::from(*b),
                    _ => Dynamic::UNIT,
                };
                map.insert(k.clone().into(), val);
            }
        }

        scope.push("doc", map);

        // Execute the sandbox script
        let result: Map = self.engine.eval_ast_with_scope(&mut scope, &ast)?;

        // Reconstruct Serde JSON
        let mut mutated_doc = serde_json::Map::new();
        for (k, v) in result {
            let json_val = if v.is::<String>() {
                Value::String(v.cast::<String>())
            } else if v.is::<i64>() {
                Value::Number(v.cast::<i64>().into())
            } else if v.is::<f64>() {
                Value::Number(serde_json::Number::from_f64(v.cast::<f64>()).unwrap())
            } else if v.is::<bool>() {
                Value::Bool(v.cast::<bool>())
            } else {
                Value::Null
            };
            mutated_doc.insert(k.to_string(), json_val);
        }

        Ok(Value::Object(mutated_doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_execute_lifecycle() {
        let engine = ScriptingEngine::new();
        let doc = json!({
            "base": 1000,
            "status": "Draft",
        });

        let script = r#"
            doc.base = doc.base * 2;
            doc.status = "Approved";
            doc
        "#;

        let result = engine.execute_lifecycle("test_script", script, doc).unwrap();
        assert_eq!(result["base"].as_i64(), Some(2000));
        assert_eq!(result["status"].as_str(), Some("Approved"));
    }
}

