use std::{collections::HashMap, path::PathBuf, sync::Arc};

use tokio::sync::Mutex;

use super::TaskId;

#[derive(Debug, Default)]
pub struct TaskRouter {
    routes: Arc<Mutex<HashMap<TaskId, String>>>,
}

impl TaskRouter {
    pub async fn new() -> Self {
        let routes = match Self::load_routes().await {
            Ok(routes) => routes,
            Err(e) => {
                log::warn!("Failed to load task routes: {}", e);
                HashMap::new()
            }
        };

        Self {
            routes: Arc::new(Mutex::new(routes)),
        }
    }

    fn get_routes_path() -> PathBuf {
        PathBuf::from(&*crate::globals::CONFIG_DIR).join("routes.json")
    }

    async fn load_routes() -> Result<HashMap<TaskId, String>, Box<dyn std::error::Error>> {
        let route_path = Self::get_routes_path();
        if !route_path.exists() {
            return Ok(HashMap::new());
        }

        let data = tokio::fs::read_to_string(&route_path).await?;
        let routes: HashMap<TaskId, String> = serde_json::from_str(&data)?;
        log::info!("Loaded {} task route(s) from routes.json", routes.len());
        Ok(routes)
    }

    pub async fn get(&self, task_id: &TaskId) -> Option<String> {
        self.routes.lock().await.get(task_id).cloned()
    }

    pub async fn insert(&self, task_id: TaskId, chat_id: String) -> Option<String> {
        self.routes.lock().await.insert(task_id, chat_id)
    }

    pub async fn remove(&self, task_id: &TaskId) -> Option<String> {
        self.routes.lock().await.remove(task_id)
    }

    pub async fn copy(&self, from_task_id: &TaskId, to_task_id: TaskId) -> Option<String> {
        let mut routes = self.routes.lock().await;
        let chat_id = routes.get(from_task_id)?.clone();
        routes.insert(to_task_id, chat_id.clone());
        Some(chat_id)
    }

    pub async fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let route_path = Self::get_routes_path();
        let routes = self.routes.lock().await;
        let json = serde_json::to_string_pretty(&*routes)?;
        tokio::fs::write(&route_path, json).await?;
        log::info!("Saved {} task route(s) to routes.json", routes.len());
        Ok(())
    }

    #[cfg(test)]
    pub async fn len(&self) -> usize {
        self.routes.lock().await.len()
    }
}
