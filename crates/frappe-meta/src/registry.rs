use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::schema::DynamicDocType;

pub struct SchemaRegistry {
    schemas: RwLock<HashMap<String, Arc<DynamicDocType>>>,
}

impl SchemaRegistry {
    pub fn new() -> Self {
        Self {
            schemas: RwLock::new(HashMap::new()),
        }
    }

    pub async fn register(&self, doc_type: DynamicDocType) {
        let mut w = self.schemas.write().await;
        w.insert(doc_type.name.clone(), Arc::new(doc_type));
    }

    pub async fn get(&self, name: &str) -> Option<Arc<DynamicDocType>> {
        let r = self.schemas.read().await;
        r.get(name).cloned()
    }

    pub async fn get_all(&self) -> Vec<Arc<DynamicDocType>> {
        let r = self.schemas.read().await;
        r.values().cloned().collect()
    }
}
