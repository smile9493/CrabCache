use crate::types::PipelineOverride;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CursorModelEntry {
    pub upstream: String,
    pub pipeline: PipelineOverride,
}

#[derive(Debug, Clone, Default)]
pub struct CursorModelsConfig {
    pub aliases: HashMap<String, CursorModelEntry>,
    pub force_deepseek_profile_for_aliases: bool,
    pub synthetic_models_enabled: bool,
}

impl CursorModelsConfig {
    pub fn resolve<'a>(&'a self, client_model: &str) -> Option<&'a CursorModelEntry> {
        self.aliases.get(client_model)
    }

    pub fn should_force_deepseek_profile(&self, client_model: &str) -> bool {
        self.force_deepseek_profile_for_aliases && self.resolve(client_model).is_some()
    }
}

/// OpenAI-compatible `/v1/models` list from configured aliases (no upstream call).
pub fn synthetic_models_list_json(cfg: &CursorModelsConfig) -> Vec<u8> {
    use serde_json::json;
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut ids: Vec<&String> = cfg.aliases.keys().collect();
    ids.sort();
    let data: Vec<serde_json::Value> = ids
        .into_iter()
        .map(|id| {
            json!({
                "id": id,
                "object": "model",
                "created": created,
                "owned_by": "deepseek"
            })
        })
        .collect();
    serde_json::to_vec(&json!({ "object": "list", "data": data })).unwrap_or_default()
}

/// Startup validation: alias upstream models must be DeepSeek-compatible.
pub fn validate_cursor_models(cfg: &CursorModelsConfig) -> Result<(), String> {
    for (id, entry) in &cfg.aliases {
        if !entry.upstream.starts_with("deepseek-") {
            return Err(format!(
                "cursor model alias '{id}': upstream '{}' must start with 'deepseek-'",
                entry.upstream
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PipelineOverride;

    #[test]
    fn synthetic_models_json_lists_alias_ids() {
        let mut aliases = HashMap::new();
        aliases.insert(
            "gpt-4o".into(),
            CursorModelEntry {
                upstream: "deepseek-v4-pro".into(),
                pipeline: PipelineOverride::CursorDeepSeekV4,
            },
        );
        let cfg = CursorModelsConfig {
            aliases,
            force_deepseek_profile_for_aliases: true,
            synthetic_models_enabled: true,
        };
        let body: serde_json::Value = serde_json::from_slice(&synthetic_models_list_json(&cfg)).unwrap();
        let ids: Vec<String> = body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids, vec!["gpt-4o"]);
    }

    #[test]
    fn resolve_alias_hit() {
        let mut aliases = HashMap::new();
        aliases.insert(
            "gpt-4o".into(),
            CursorModelEntry {
                upstream: "deepseek-v4-pro".into(),
                pipeline: PipelineOverride::CursorDeepSeekV4,
            },
        );
        let cfg = CursorModelsConfig {
            aliases,
            force_deepseek_profile_for_aliases: true,
            synthetic_models_enabled: false,
        };
        let entry = cfg.resolve("gpt-4o").unwrap();
        assert_eq!(entry.upstream, "deepseek-v4-pro");
        assert!(cfg.should_force_deepseek_profile("gpt-4o"));
        assert!(!cfg.should_force_deepseek_profile("gpt-4"));
    }
}
