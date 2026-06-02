use crate::types::{PipelineGlobals, PipelineRequestContext, ProfileDescriptor, UpstreamProvider};

/// Map model name prefix to default upstream profile id.
pub fn model_prefix_to_profile(model: &str) -> &'static str {
    let lower = model.to_lowercase();

    // ── Original ──
    if lower.starts_with("deepseek-") {
        return "deepseek";
    }
    if lower.starts_with("mimo") {
        return "mimo";
    }
    if lower.starts_with("gpt-")
        || lower.starts_with("gpt ")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("chatgpt-")
    {
        return "openai";
    }
    if lower.starts_with("codex ") || lower.starts_with("codex-") || lower == "codex" {
        return "codex";
    }
    if lower.starts_with("claude-") {
        return "anthropic";
    }

    // ── International mainstream ──
    if lower.starts_with("grok-") {
        return "xai";
    }
    if lower.starts_with("mistral-")
        || lower.starts_with("codestral-")
        || lower.starts_with("pixtral-")
        || lower.starts_with("open-mistral-")
    {
        return "mistral";
    }
    if lower.starts_with("gemini-") {
        return "gemini";
    }
    if lower.starts_with("llama-") {
        return "together";
    }
    if lower.starts_with("mixtral-") {
        return "fireworks";
    }
    if lower.starts_with("command-") {
        return "cohere";
    }
    if lower.starts_with("nemotron-") {
        return "nvidia";
    }
    if lower.starts_with("reka-") {
        return "reka";
    }

    // ── China providers ──
    if lower.starts_with("qwen-") || lower.starts_with("qwq-") {
        return "alibaba";
    }
    if lower.starts_with("glm-") || lower.starts_with("chatglm-") {
        return "glm";
    }
    if lower.starts_with("kimi-") {
        return "kimi";
    }
    if lower.starts_with("ernie-") || lower.starts_with("yiyan") {
        return "baidu";
    }
    if lower.starts_with("hunyuan-") {
        return "tencent";
    }
    if lower.starts_with("spark-") || lower.starts_with("xinghuo") {
        return "iflytek";
    }
    if lower.starts_with("baichuan-") {
        return "baichuan";
    }
    if lower.starts_with("yi-") {
        return "yi";
    }
    if lower.starts_with("step-") {
        return "stepfun";
    }
    if lower.starts_with("doubao-") {
        return "doubao";
    }
    if lower.starts_with("minimax-") || lower.starts_with("abab-") {
        return "minimax";
    }
    if lower.starts_with("sensenova-") {
        return "sensenova";
    }
    if lower.starts_with("360-") {
        return "360ai";
    }

    // ── Inference platform specific models ──
    if lower.starts_with("jamba-") {
        return "ai21";
    }
    if lower.starts_with("gigachat-") {
        return "gigachat";
    }
    if lower.starts_with("solar-") {
        return "upstage";
    }
    if lower.starts_with("sabia-") || lower.starts_with("sabiazinho-") {
        return "maritalk";
    }
    if lower.starts_with("dbrx-") {
        return "databricks";
    }
    if lower.starts_with("snowflake-") {
        return "snowflake";
    }
    if lower.starts_with("lfm-") {
        return "liquid";
    }
    if lower.starts_with("palmyra-") {
        return "writer";
    }

    // Default fallback
    "deepseek"
}

/// Profile id for OpenAI routes.
pub fn resolve_openai_profile_id(profiles: &[ProfileDescriptor]) -> Option<String> {
    if profiles.iter().any(|p| p.id == "openai") {
        return Some("openai".into());
    }
    None
}

/// Profile id for Codex OAuth routes.
pub fn resolve_codex_profile_id(profiles: &[ProfileDescriptor]) -> Option<String> {
    if profiles.iter().any(|p| p.id == "codex") {
        return Some("codex".into());
    }
    None
}

/// True when an explicit key/domain profile id matches the model's implied upstream family.
pub fn explicit_profile_matches_model(profile_id: &str, model: &str) -> bool {
    let implied = model_prefix_to_profile(model);
    match implied {
        "openai" => profile_id == "openai",
        "codex" => profile_id == "codex",
        other => profile_id == other,
    }
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
        && explicit_profile_matches_model(id, ctx.model)
    {
        return (found.0, found.1, true);
    }
    if let Some(id) = ctx.domain_upstream_profile.filter(|s| !s.trim().is_empty())
        && let Some(found) = pick(id)
        && explicit_profile_matches_model(id, ctx.model)
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
    if from_model == "openai" {
        if let Some(openai_id) = resolve_openai_profile_id(profiles)
            && let Some(found) = pick(&openai_id)
        {
            return (found.0, found.1, false);
        }
    } else if from_model == "codex" {
        if let Some(codex_id) = resolve_codex_profile_id(profiles)
            && let Some(found) = pick(&codex_id)
        {
            return (found.0, found.1, false);
        }
    } else if let Some(found) = pick(from_model) {
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
            model: "deepseek-v4-pro",
            key_upstream_profile: Some("deepseek"),
            ..Default::default()
        };
        let (id, provider, explicit) = resolve_upstream_profile_id(&globals, &profiles(), &ctx);
        assert_eq!(id, "deepseek");
        assert_eq!(provider, UpstreamProvider::Deepseek);
        assert!(explicit);
    }

    #[test]
    fn key_mimo_profile_does_not_hijack_gpt_model() {
        let globals = PipelineGlobals::default();
        let profiles = vec![
            ProfileDescriptor {
                id: "codex".into(),
                provider: UpstreamProvider::Openai,
            },
            ProfileDescriptor {
                id: "mimo".into(),
                provider: UpstreamProvider::Mimo,
            },
        ];
        let ctx = PipelineRequestContext {
            model: "gpt-5.4-mini",
            key_upstream_profile: Some("mimo"),
            ..Default::default()
        };
        let (id, provider, explicit) = resolve_upstream_profile_id(&globals, &profiles, &ctx);
        assert_eq!(id, "codex");
        assert_eq!(provider, UpstreamProvider::Openai);
        assert!(!explicit);
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
    fn model_prefix_openai_resolves_codex_profile() {
        let globals = PipelineGlobals::default();
        let profiles = vec![ProfileDescriptor {
            id: "codex".into(),
            provider: UpstreamProvider::Openai,
        }];
        let ctx = PipelineRequestContext {
            model: "gpt-4o",
            ..Default::default()
        };
        let (id, provider, _) = resolve_upstream_profile_id(&globals, &profiles, &ctx);
        assert_eq!(id, "codex");
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
    fn model_prefix_mimopro() {
        let globals = PipelineGlobals::default();
        let profiles = vec![ProfileDescriptor {
            id: "mimo".into(),
            provider: UpstreamProvider::Mimo,
        }];
        let ctx = PipelineRequestContext {
            model: "mimopro",
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

    #[test]
    fn mimo_model_maps_to_mimo_profile() {
        assert_eq!(model_prefix_to_profile("mimo-v2.5-pro"), "mimo");
        assert_eq!(model_prefix_to_profile("mimo-v2-flash"), "mimo");
        assert_eq!(model_prefix_to_profile("mimo-v2"), "mimo");
    }

    #[test]
    fn codex_model_maps_to_codex_profile() {
        assert_eq!(model_prefix_to_profile("codex-mini"), "codex");
        assert_eq!(model_prefix_to_profile("codex-2025"), "codex");
        assert_eq!(model_prefix_to_profile("codex"), "codex");
    }

    #[test]
    fn gpt_model_maps_to_openai_profile() {
        assert_eq!(model_prefix_to_profile("gpt-5"), "openai");
        assert_eq!(model_prefix_to_profile("gpt-4o"), "openai");
        assert_eq!(model_prefix_to_profile("o1"), "openai");
        assert_eq!(model_prefix_to_profile("o3"), "openai");
    }

    #[test]
    fn codex_model_resolves_to_codex_profile() {
        let globals = PipelineGlobals::default();
        let profiles = vec![
            ProfileDescriptor {
                id: "openai".into(),
                provider: UpstreamProvider::Openai,
            },
            ProfileDescriptor {
                id: "codex".into(),
                provider: UpstreamProvider::Codex,
            },
        ];
        let ctx = PipelineRequestContext {
            model: "codex-mini",
            ..Default::default()
        };
        let (id, provider, _) = resolve_upstream_profile_id(&globals, &profiles, &ctx);
        assert_eq!(id, "codex");
        assert_eq!(provider, UpstreamProvider::Codex);
    }
}
