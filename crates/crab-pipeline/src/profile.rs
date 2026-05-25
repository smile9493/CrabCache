use crate::types::{PipelineGlobals, PipelineRequestContext, ProfileDescriptor, UpstreamProvider};

/// Map model name prefix to default upstream profile id.
pub fn model_prefix_to_profile(model: &str) -> &'static str {
    let lower = model.to_lowercase();
    if lower.starts_with("deepseek-") {
        return "deepseek";
    }
    if lower.starts_with("xiaomi/mimo-") || lower.starts_with("mimo-") {
        return "mimo";
    }
    if lower.starts_with("gpt-")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("chatgpt-")
    {
        return "openai";
    }
    if lower.starts_with("claude-") {
        return "anthropic";
    }
    "deepseek"
}

pub fn resolve_upstream_profile_id(
    globals: &PipelineGlobals,
    profiles: &[ProfileDescriptor],
    ctx: &PipelineRequestContext<'_>,
) -> (String, UpstreamProvider, bool) {
    let pick = |id: &str| -> Option<(String, UpstreamProvider)> {
        profiles
            .iter()
            .find(|p| p.id == id)
            .map(|p| (p.id.clone(), p.provider))
    };

    if let Some(id) = ctx.key_upstream_profile.filter(|s| !s.trim().is_empty())
        && let Some(found) = pick(id)
    {
        return (found.0, found.1, true);
    }
    if let Some(id) = ctx.domain_upstream_profile.filter(|s| !s.trim().is_empty())
        && let Some(found) = pick(id)
    {
        return (found.0, found.1, true);
    }

    if globals
        .cursor_models
        .should_force_deepseek_profile(ctx.model)
        && pick("deepseek").is_some()
    {
        let found = pick("deepseek").unwrap();
        return (found.0, found.1, false);
    }

    let from_model = model_prefix_to_profile(ctx.model);
    if let Some(found) = pick(from_model) {
        return (found.0, found.1, false);
    }

    if let Some(found) = pick(&globals.default_upstream_profile) {
        return (found.0, found.1, false);
    }

    if let Some(first) = profiles.first() {
        return (first.id.clone(), first.provider, false);
    }

    (
        globals.default_upstream_profile.clone(),
        UpstreamProvider::Deepseek,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profiles() -> Vec<ProfileDescriptor> {
        vec![
            ProfileDescriptor {
                id: "deepseek".into(),
                provider: UpstreamProvider::Deepseek,
            },
            ProfileDescriptor {
                id: "openai".into(),
                provider: UpstreamProvider::Openai,
            },
        ]
    }

    #[test]
    fn key_profile_overrides_model() {
        let globals =
            PipelineGlobals::with_profiles("deepseek", ["deepseek", "openai"].map(String::from));
        let ctx = PipelineRequestContext {
            model: "gpt-4",
            key_upstream_profile: Some("deepseek"),
            ..Default::default()
        };
        let (id, provider, explicit) = resolve_upstream_profile_id(&globals, &profiles(), &ctx);
        assert_eq!(id, "deepseek");
        assert_eq!(provider, UpstreamProvider::Deepseek);
        assert!(explicit);
    }

    #[test]
    fn model_prefix_openai() {
        let globals = PipelineGlobals::default();
        let ctx = PipelineRequestContext {
            model: "gpt-4o",
            ..Default::default()
        };
        let (id, provider, _) = resolve_upstream_profile_id(&globals, &profiles(), &ctx);
        assert_eq!(id, "openai");
        assert_eq!(provider, UpstreamProvider::Openai);
    }

    #[test]
    fn model_prefix_mimo() {
        let globals = PipelineGlobals::default();
        let profiles = vec![ProfileDescriptor {
            id: "mimo".into(),
            provider: UpstreamProvider::Mimo,
        }];
        let ctx = PipelineRequestContext {
            model: "mimo-v2.5-pro",
            ..Default::default()
        };
        let (id, provider, _) = resolve_upstream_profile_id(&globals, &profiles, &ctx);
        assert_eq!(id, "mimo");
        assert_eq!(provider, UpstreamProvider::Mimo);
    }

    #[test]
    fn alias_forces_deepseek_profile() {
        use crate::cursor_models::{CursorModelEntry, CursorModelsConfig};
        use std::collections::HashMap;
        let mut aliases = HashMap::new();
        aliases.insert(
            "gpt-4o".into(),
            CursorModelEntry {
                upstream: "deepseek-v4-pro".into(),
                pipeline: crate::types::PipelineOverride::CursorDeepSeekV4,
            },
        );
        let globals = PipelineGlobals::with_profiles_mode_and_cursor_models(
            "deepseek",
            ["deepseek", "openai"].map(String::from),
            crate::types::PipelineMode::Auto,
            CursorModelsConfig {
                aliases,
                force_deepseek_profile_for_aliases: true,
                synthetic_models_enabled: false,
            },
        );
        let ctx = PipelineRequestContext {
            model: "gpt-4o",
            ..Default::default()
        };
        let (id, provider, _) = resolve_upstream_profile_id(&globals, &profiles(), &ctx);
        assert_eq!(id, "deepseek");
        assert_eq!(provider, UpstreamProvider::Deepseek);
    }
}
