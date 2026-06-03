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

/// Map model-family canonical id (from [`model_prefix_to_profile`]) to upstream provider.
pub fn provider_for_model_family(canonical_id: &str) -> UpstreamProvider {
    match canonical_id {
        "codex" => UpstreamProvider::Codex,
        other => UpstreamProvider::from_str(other),
    }
}

/// Pick a profile by upstream provider, preferring exact canonical id when present.
pub fn pick_profile_by_provider(
    profiles: &[ProfileDescriptor],
    provider: UpstreamProvider,
    preferred_canonical_id: Option<&str>,
) -> Option<(String, UpstreamProvider)> {
    if let Some(canonical) = preferred_canonical_id
        && let Some(p) = profiles.iter().find(|p| p.id == canonical && p.provider == provider)
    {
        return Some((p.id.clone(), p.provider));
    }

    let mut candidates: Vec<&ProfileDescriptor> =
        profiles.iter().filter(|p| p.provider == provider).collect();
    if candidates.is_empty() {
        return None;
    }
    if candidates.len() == 1 {
        let p = candidates[0];
        return Some((p.id.clone(), p.provider));
    }

    if let Some(canonical) = preferred_canonical_id {
        candidates.sort_by(|a, b| {
            let rank = |p: &ProfileDescriptor| -> u8 {
                if p.id == canonical {
                    0
                } else if p.id.starts_with(canonical) {
                    1
                } else {
                    2
                }
            };
            rank(a).cmp(&rank(b)).then_with(|| a.id.cmp(&b.id))
        });
    } else {
        candidates.sort_by(|a, b| a.id.cmp(&b.id));
    }
    let p = candidates[0];
    Some((p.id.clone(), p.provider))
}

/// Profile id for OpenAI routes (supports custom ids such as `openai-prod`).
pub fn resolve_openai_profile_id(profiles: &[ProfileDescriptor]) -> Option<String> {
    pick_profile_by_provider(profiles, UpstreamProvider::Openai, Some("openai"))
        .map(|(id, _)| id)
}

/// Profile id for Codex OAuth routes (supports custom ids such as `codex-plus`).
pub fn resolve_codex_profile_id(profiles: &[ProfileDescriptor]) -> Option<String> {
    pick_profile_by_provider(profiles, UpstreamProvider::Codex, Some("codex")).map(|(id, _)| id)
}

/// True when an explicit key/domain profile matches the model's implied upstream family.
pub fn explicit_profile_matches_model(profile: &ProfileDescriptor, model: &str) -> bool {
    let implied = model_prefix_to_profile(model);
    let expected = provider_for_model_family(implied);
    profile.provider == expected
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
        && let Some(profile) = profiles.iter().find(|p| p.id == id)
        && explicit_profile_matches_model(profile, ctx.model)
    {
        return (profile.id.clone(), profile.provider, true);
    }
    if let Some(id) = ctx.domain_upstream_profile.filter(|s| !s.trim().is_empty())
        && let Some(profile) = profiles.iter().find(|p| p.id == id)
        && explicit_profile_matches_model(profile, ctx.model)
    {
        return (profile.id.clone(), profile.provider, true);
    }

    if globals
        .cursor_models
        .should_force_deepseek_profile(ctx.model)
        && let Some(found) =
            pick("deepseek").or_else(|| pick_profile_by_provider(profiles, UpstreamProvider::Deepseek, Some("deepseek")))
    {
        return (found.0, found.1, false);
    }

    let from_model = model_prefix_to_profile(ctx.model);
    let provider = provider_for_model_family(from_model);
    if from_model == "openai" {
        if let Some(found) = pick_profile_by_provider(profiles, UpstreamProvider::Openai, Some("openai"))
            .or_else(|| resolve_openai_profile_id(profiles).and_then(|id| pick(&id)))
            // Codex CLI sends gpt-* display names; prefer OAuth codex pool when no openai profile exists.
            .or_else(|| {
                pick_profile_by_provider(profiles, UpstreamProvider::Codex, Some("codex"))
                    .or_else(|| resolve_codex_profile_id(profiles).and_then(|id| pick(&id)))
            })
        {
            return (found.0, found.1, false);
        }
    } else if from_model == "codex" {
        if let Some(found) = pick_profile_by_provider(profiles, UpstreamProvider::Codex, Some("codex"))
            .or_else(|| resolve_codex_profile_id(profiles).and_then(|id| pick(&id)))
        {
            return (found.0, found.1, false);
        }
    } else if let Some(found) = pick(from_model)
        .or_else(|| pick_profile_by_provider(profiles, provider, Some(from_model)))
    {
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
    fn gpt_model_prefers_codex_oauth_profile_over_default_deepseek() {
        let globals = PipelineGlobals::with_profiles(
            "deepseek",
            ["deepseek", "codex", "mimo-tp-sgp"].map(String::from),
        );
        let profiles = vec![
            ProfileDescriptor {
                id: "deepseek".into(),
                provider: UpstreamProvider::Deepseek,
            },
            ProfileDescriptor {
                id: "codex".into(),
                provider: UpstreamProvider::Codex,
            },
            ProfileDescriptor {
                id: "mimo-tp-sgp".into(),
                provider: UpstreamProvider::Mimo,
            },
        ];
        let ctx = PipelineRequestContext {
            model: "gpt-5.4-mini",
            ..Default::default()
        };
        let (id, provider, explicit) = resolve_upstream_profile_id(&globals, &profiles, &ctx);
        assert_eq!(id, "codex");
        assert_eq!(provider, UpstreamProvider::Codex);
        assert!(!explicit);
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

    #[test]
    fn custom_mimo_profile_id_resolves_by_provider() {
        let globals = PipelineGlobals::default();
        let profiles = vec![
            ProfileDescriptor {
                id: "deepseek".into(),
                provider: UpstreamProvider::Deepseek,
            },
            ProfileDescriptor {
                id: "mimo-tp-sgp".into(),
                provider: UpstreamProvider::Mimo,
            },
        ];
        let ctx = PipelineRequestContext {
            model: "mimo-v2.5-pro",
            ..Default::default()
        };
        let (id, provider, explicit) = resolve_upstream_profile_id(&globals, &profiles, &ctx);
        assert_eq!(id, "mimo-tp-sgp");
        assert_eq!(provider, UpstreamProvider::Mimo);
        assert!(!explicit);
    }

    #[test]
    fn custom_codex_profile_id_resolves_by_provider() {
        let globals = PipelineGlobals::default();
        let profiles = vec![ProfileDescriptor {
            id: "codex-plus".into(),
            provider: UpstreamProvider::Codex,
        }];
        assert_eq!(
            resolve_codex_profile_id(&profiles).as_deref(),
            Some("codex-plus")
        );
        let ctx = PipelineRequestContext {
            model: "codex-mini",
            ..Default::default()
        };
        let (id, provider, _) = resolve_upstream_profile_id(&globals, &profiles, &ctx);
        assert_eq!(id, "codex-plus");
        assert_eq!(provider, UpstreamProvider::Codex);
    }

    #[test]
    fn explicit_key_profile_matches_custom_codex_id() {
        let globals = PipelineGlobals::default();
        let profiles = vec![ProfileDescriptor {
            id: "codex-plus".into(),
            provider: UpstreamProvider::Codex,
        }];
        let ctx = PipelineRequestContext {
            model: "codex-mini",
            key_upstream_profile: Some("codex-plus"),
            ..Default::default()
        };
        let (id, provider, explicit) = resolve_upstream_profile_id(&globals, &profiles, &ctx);
        assert_eq!(id, "codex-plus");
        assert_eq!(provider, UpstreamProvider::Codex);
        assert!(explicit);
    }

    #[test]
    fn canonical_mimo_id_still_works() {
        let globals = PipelineGlobals::default();
        let profiles = vec![
            ProfileDescriptor {
                id: "mimo".into(),
                provider: UpstreamProvider::Mimo,
            },
            ProfileDescriptor {
                id: "mimo-tp-sgp".into(),
                provider: UpstreamProvider::Mimo,
            },
        ];
        let ctx = PipelineRequestContext {
            model: "mimo-v2.5-pro",
            ..Default::default()
        };
        let (id, _, _) = resolve_upstream_profile_id(&globals, &profiles, &ctx);
        assert_eq!(id, "mimo");
    }
}
