use crate::is_deepseek_v4_model;
use crate::profile::{model_prefix_to_profile, resolve_upstream_profile_id};
use crate::signals::{cursor_agent_signals, user_agent_suggests_cursor};
use crate::types::{
    PipelineGlobals, PipelineMode, PipelineOverride, PipelineRequestContext, PipelineSelection,
    PipelineSelectionReason, ProfileDescriptor, RequestPipeline, UpstreamProvider,
};

pub fn select_request_pipeline(
    globals: &PipelineGlobals,
    profiles: &[ProfileDescriptor],
    ctx: &PipelineRequestContext<'_>,
) -> PipelineSelection {
    let (upstream_profile_id, provider, _profile_explicit) =
        resolve_upstream_profile_id(globals, profiles, ctx);
    let provider = normalize_legacy_codex_provider(provider, &upstream_profile_id);

    if globals.pipeline_mode == PipelineMode::ForceCursorV4 {
        return PipelineSelection {
            pipeline: RequestPipeline::CursorDeepSeekV4,
            upstream_profile_id,
            provider,
            reason: PipelineSelectionReason::GlobalForceCursorV4,
        };
    }

    let override_pipe = ctx
        .key_pipeline
        .or(ctx.domain_pipeline)
        .unwrap_or(PipelineOverride::Auto);

    if let Some(forced) = pipeline_from_override(override_pipe, provider)
        && pipeline_override_matches_model(override_pipe, ctx.model)
    {
        let reason = if ctx.key_pipeline == Some(override_pipe) {
            PipelineSelectionReason::KeyOverride
        } else {
            PipelineSelectionReason::DomainOverride
        };
        return PipelineSelection {
            pipeline: forced,
            upstream_profile_id,
            provider,
            reason,
        };
    }

    let (pipeline, reason) = auto_pipeline_with_reason(ctx, provider);

    // When a model alias overrides the pipeline (e.g. gpt-4o → deepseek-v4-pro via Codex key),
    // also override the upstream profile so the request uses the correct endpoints and key pool.
    let upstream_profile_id =
        if reason == PipelineSelectionReason::ModelAlias && ctx.model_alias_pipeline.is_some() {
            profile_for_pipeline(pipeline, profiles)
                .unwrap_or(upstream_profile_id)
        } else {
            upstream_profile_id
        };

    PipelineSelection {
        pipeline,
        upstream_profile_id,
        provider,
        reason,
    }
}

/// Key/domain pipeline override must match the model family (Codex GPT / MiMo / DeepSeek).
fn pipeline_override_matches_model(override_pipe: PipelineOverride, model: &str) -> bool {
    let implied = model_prefix_to_profile(model);
    match override_pipe {
        PipelineOverride::Auto => true,
        PipelineOverride::MimoTokenPlanRelay | PipelineOverride::MimoPaygRelay => {
            implied == "mimo"
        }
        PipelineOverride::CodexRelay => implied == "openai",
        PipelineOverride::CodexDeepSeek
        | PipelineOverride::CursorDeepSeekV4
        | PipelineOverride::DeepSeekLight => implied == "deepseek",
        PipelineOverride::GenericRelay => true,
    }
}

/// Legacy Codex profiles may still store `provider=openai` while `id=codex`.
fn normalize_legacy_codex_provider(
    provider: UpstreamProvider,
    upstream_profile_id: &str,
) -> UpstreamProvider {
    if provider == UpstreamProvider::Openai && upstream_profile_id.eq_ignore_ascii_case("codex") {
        UpstreamProvider::Codex
    } else {
        provider
    }
}

/// Find a profile whose provider matches the pipeline's expected provider.
fn profile_for_pipeline(
    pipeline: RequestPipeline,
    profiles: &[ProfileDescriptor],
) -> Option<String> {
    let target_id = match pipeline {
        RequestPipeline::CursorDeepSeekV4
        | RequestPipeline::DeepSeekLight
        | RequestPipeline::CodexDeepSeek => "deepseek",
        RequestPipeline::MimoTokenPlanRelay | RequestPipeline::MimoPaygRelay => "mimo",
        RequestPipeline::CodexRelay => {
            return profiles
                .iter()
                .find(|p| p.id == "codex" || p.id == "openai")
                .map(|p| p.id.clone());
        }
        RequestPipeline::GenericRelay => return None,
    };
    profiles
        .iter()
        .find(|p| p.id == target_id)
        .map(|p| p.id.clone())
}

fn pipeline_from_override(
    o: PipelineOverride,
    provider: UpstreamProvider,
) -> Option<RequestPipeline> {
    match o {
        PipelineOverride::Auto => None,
        PipelineOverride::CursorDeepSeekV4 => {
            if provider == UpstreamProvider::Deepseek {
                Some(RequestPipeline::CursorDeepSeekV4)
            } else {
                Some(RequestPipeline::GenericRelay)
            }
        }
        PipelineOverride::DeepSeekLight => {
            if provider == UpstreamProvider::Deepseek {
                Some(RequestPipeline::DeepSeekLight)
            } else {
                Some(RequestPipeline::GenericRelay)
            }
        }
        PipelineOverride::MimoTokenPlanRelay => {
            if provider == UpstreamProvider::Mimo {
                Some(RequestPipeline::MimoTokenPlanRelay)
            } else {
                Some(RequestPipeline::GenericRelay)
            }
        }
        PipelineOverride::MimoPaygRelay => {
            if provider == UpstreamProvider::Mimo {
                Some(RequestPipeline::MimoPaygRelay)
            } else {
                Some(RequestPipeline::GenericRelay)
            }
        }
        PipelineOverride::GenericRelay => Some(RequestPipeline::GenericRelay),
        PipelineOverride::CodexRelay => {
            if provider == UpstreamProvider::Codex {
                Some(RequestPipeline::CodexRelay)
            } else {
                Some(RequestPipeline::GenericRelay)
            }
        }
        PipelineOverride::CodexDeepSeek => {
            if provider == UpstreamProvider::Deepseek {
                Some(RequestPipeline::CodexDeepSeek)
            } else {
                Some(RequestPipeline::GenericRelay)
            }
        }
    }
}

fn auto_pipeline_with_reason(
    ctx: &PipelineRequestContext<'_>,
    provider: UpstreamProvider,
) -> (RequestPipeline, PipelineSelectionReason) {
    // Model alias pipeline takes priority regardless of profile provider.
    // This allows a Codex-profile API key to route aliased models (e.g. gpt-4o → deepseek-v4-pro)
    // through DeepSeek/MiMo pipelines instead of CodexRelay.
    if let Some(alias_pipe) = ctx.model_alias_pipeline {
        if let Some(resolved) = resolve_alias_pipeline(alias_pipe) {
            return (resolved, PipelineSelectionReason::ModelAlias);
        }
    }

    let pipeline = auto_pipeline_legacy(ctx, provider);
    let reason = match pipeline {
        RequestPipeline::CursorDeepSeekV4 => PipelineSelectionReason::CursorSignals,
        RequestPipeline::DeepSeekLight => PipelineSelectionReason::DeepSeekNonV4,
        RequestPipeline::MimoTokenPlanRelay
        | RequestPipeline::MimoPaygRelay => PipelineSelectionReason::MimoProvider,
        RequestPipeline::GenericRelay => PipelineSelectionReason::ProviderDefault,
        RequestPipeline::CodexRelay => PipelineSelectionReason::CodexProvider,
        // CodexDeepSeek is only selected via model alias; legacy never produces it.
        RequestPipeline::CodexDeepSeek => PipelineSelectionReason::CodexDeepSeekModelAlias,
    };
    (pipeline, reason)
}

/// Resolve an explicit model-alias pipeline override to a concrete [`RequestPipeline`].
///
/// Returns `None` for `Auto` (fall through to legacy heuristics).
fn resolve_alias_pipeline(alias_pipe: PipelineOverride) -> Option<RequestPipeline> {
    match alias_pipe {
        PipelineOverride::CursorDeepSeekV4 => Some(RequestPipeline::CursorDeepSeekV4),
        PipelineOverride::DeepSeekLight => Some(RequestPipeline::DeepSeekLight),
        PipelineOverride::MimoTokenPlanRelay => Some(RequestPipeline::MimoTokenPlanRelay),
        PipelineOverride::MimoPaygRelay => Some(RequestPipeline::MimoPaygRelay),
        PipelineOverride::GenericRelay => Some(RequestPipeline::GenericRelay),
        PipelineOverride::CodexRelay => Some(RequestPipeline::CodexRelay),
        PipelineOverride::CodexDeepSeek => Some(RequestPipeline::CodexDeepSeek),
        PipelineOverride::Auto => None,
    }
}

fn auto_pipeline_legacy(
    ctx: &PipelineRequestContext<'_>,
    provider: UpstreamProvider,
) -> RequestPipeline {
    match provider {
        UpstreamProvider::Deepseek => {
            if is_deepseek_v4_model(ctx.model)
                && (cursor_agent_signals(ctx.payload)
                    || ctx
                        .conversation_id_header
                        .is_some_and(|s| !s.trim().is_empty())
                    || user_agent_suggests_cursor(ctx.user_agent))
            {
                RequestPipeline::CursorDeepSeekV4
            } else {
                RequestPipeline::DeepSeekLight
            }
        }
        UpstreamProvider::Mimo => RequestPipeline::MimoTokenPlanRelay,
        UpstreamProvider::Codex => RequestPipeline::CodexRelay,
        UpstreamProvider::Openai | UpstreamProvider::Anthropic | UpstreamProvider::Other => {
            RequestPipeline::GenericRelay
        }
    }
}

/// Returns an error message when override is invalid for the resolved profile.
pub fn validate_pipeline_override(
    override_pipe: PipelineOverride,
    provider: UpstreamProvider,
) -> Option<&'static str> {
    match override_pipe {
        PipelineOverride::CursorDeepSeekV4
        | PipelineOverride::DeepSeekLight
        | PipelineOverride::CodexDeepSeek
            if provider != UpstreamProvider::Deepseek =>
        {
            Some(
                "cursor_deepseek_v4, deepseek_light, and codex_deepseek require a deepseek upstream profile",
            )
        }
        PipelineOverride::MimoTokenPlanRelay
        | PipelineOverride::MimoPaygRelay
            if provider != UpstreamProvider::Mimo =>
        {
            Some(
                "mimo_token_plan_relay / mimo_payg_relay require a mimo upstream profile",
            )
        }
        PipelineOverride::CodexRelay if provider != UpstreamProvider::Codex => {
            Some("codex_relay requires a codex upstream profile")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::resolve_upstream_profile_id;
    use serde_json::json;

    fn deepseek_profiles() -> Vec<ProfileDescriptor> {
        vec![ProfileDescriptor {
            id: "deepseek".into(),
            provider: UpstreamProvider::Deepseek,
        }]
    }

    #[test]
    fn v4_with_tools_selects_cursor_pipeline() {
        let globals = PipelineGlobals::default();
        let payload = json!({
            "model": "deepseek-v4-pro",
            "tools": [],
            "messages": [{"role": "user", "content": "hi"}]
        });
        let ctx = PipelineRequestContext {
            model: "deepseek-v4-pro",
            payload: Some(&payload),
            ..Default::default()
        };
        let sel = select_request_pipeline(&globals, &deepseek_profiles(), &ctx);
        assert_eq!(sel.pipeline, RequestPipeline::CursorDeepSeekV4);
    }

    #[test]
    fn deepseek_chat_light_pipeline() {
        let globals = PipelineGlobals::default();
        let payload = json!({
            "model": "deepseek-chat",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let ctx = PipelineRequestContext {
            model: "deepseek-chat",
            payload: Some(&payload),
            ..Default::default()
        };
        let sel = select_request_pipeline(&globals, &deepseek_profiles(), &ctx);
        assert_eq!(sel.pipeline, RequestPipeline::DeepSeekLight);
    }

    fn globals_with_gpt4o_alias() -> PipelineGlobals {
        use crate::cursor_models::{CursorModelEntry, CursorModelsConfig};
        use std::collections::HashMap;
        let mut aliases = HashMap::new();
        aliases.insert(
            "gpt-4o".into(),
            CursorModelEntry {
                upstream: "deepseek-v4-pro".into(),
                pipeline: PipelineOverride::CursorDeepSeekV4,
            },
        );
        PipelineGlobals::with_profiles_mode_and_cursor_models(
            "deepseek",
            ["deepseek", "openai"].map(String::from),
            PipelineMode::Auto,
            CursorModelsConfig {
                aliases,
                force_deepseek_profile_for_aliases: true,
                synthetic_models_enabled: false,
            },
        )
    }

    #[test]
    fn gpt4o_alias_selects_deepseek_v4_pipeline() {
        let globals = globals_with_gpt4o_alias();
        let profiles = vec![ProfileDescriptor {
            id: "deepseek".into(),
            provider: UpstreamProvider::Deepseek,
        }];
        let payload = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let ctx = PipelineRequestContext {
            model: "gpt-4o",
            payload: Some(&payload),
            alias_upstream_model: Some("deepseek-v4-pro"),
            model_alias_pipeline: Some(PipelineOverride::CursorDeepSeekV4),
            ..Default::default()
        };
        let sel = select_request_pipeline(&globals, &profiles, &ctx);
        assert_eq!(sel.upstream_profile_id, "deepseek");
        assert_eq!(sel.pipeline, RequestPipeline::CursorDeepSeekV4);
        assert_eq!(sel.reason, PipelineSelectionReason::ModelAlias);
    }

    #[test]
    fn gpt_on_openai_profile_generic() {
        let globals =
            PipelineGlobals::with_profiles("deepseek", ["deepseek", "openai"].map(String::from));
        let profiles = vec![
            ProfileDescriptor {
                id: "deepseek".into(),
                provider: UpstreamProvider::Deepseek,
            },
            ProfileDescriptor {
                id: "openai".into(),
                provider: UpstreamProvider::Openai,
            },
        ];
        let ctx = PipelineRequestContext {
            model: "gpt-4",
            ..Default::default()
        };
        let sel = select_request_pipeline(&globals, &profiles, &ctx);
        assert_eq!(sel.upstream_profile_id, "openai");
        assert_eq!(sel.pipeline, RequestPipeline::GenericRelay);
    }

    #[test]
    fn gpt4o_alias_codex_deepseek_selects_pipeline() {
        let globals = globals_with_gpt4o_codex_deepseek_alias();
        let profiles = vec![ProfileDescriptor {
            id: "deepseek".into(),
            provider: UpstreamProvider::Deepseek,
        }];
        let payload = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let ctx = PipelineRequestContext {
            model: "gpt-4o",
            payload: Some(&payload),
            alias_upstream_model: Some("deepseek-v4-pro"),
            model_alias_pipeline: Some(PipelineOverride::CodexDeepSeek),
            ..Default::default()
        };
        let sel = select_request_pipeline(&globals, &profiles, &ctx);
        assert_eq!(sel.upstream_profile_id, "deepseek");
        assert_eq!(sel.pipeline, RequestPipeline::CodexDeepSeek);
        assert_eq!(sel.reason, PipelineSelectionReason::ModelAlias);
    }

    fn globals_with_gpt4o_codex_deepseek_alias() -> PipelineGlobals {
        use crate::cursor_models::{CursorModelEntry, CursorModelsConfig};
        use std::collections::HashMap;
        let mut aliases = HashMap::new();
        aliases.insert(
            "gpt-4o".into(),
            CursorModelEntry {
                upstream: "deepseek-v4-pro".into(),
                pipeline: PipelineOverride::CodexDeepSeek,
            },
        );
        PipelineGlobals::with_profiles_mode_and_cursor_models(
            "deepseek",
            ["deepseek", "codex"].map(String::from),
            PipelineMode::Auto,
            CursorModelsConfig {
                aliases,
                force_deepseek_profile_for_aliases: true,
                synthetic_models_enabled: false,
            },
        )
    }

    #[test]
    fn mimo_model_selects_mimo_relay() {
        let globals =
            PipelineGlobals::with_profiles("deepseek", ["deepseek", "mimo"].map(String::from));
        let profiles = vec![
            ProfileDescriptor {
                id: "deepseek".into(),
                provider: UpstreamProvider::Deepseek,
            },
            ProfileDescriptor {
                id: "mimo".into(),
                provider: UpstreamProvider::Mimo,
            },
        ];
        let ctx = PipelineRequestContext {
            model: "mimo-v2.5-pro",
            ..Default::default()
        };
        let sel = select_request_pipeline(&globals, &profiles, &ctx);
        assert_eq!(sel.upstream_profile_id, "mimo");
        assert_eq!(sel.pipeline, RequestPipeline::MimoTokenPlanRelay);
        assert_eq!(sel.reason, PipelineSelectionReason::MimoProvider);
    }

    #[test]
    fn xiaomi_prefixed_model_routes_to_mimo_profile() {
        let globals = PipelineGlobals::default();
        let profiles = vec![ProfileDescriptor {
            id: "mimo".into(),
            provider: UpstreamProvider::Mimo,
        }];
        let ctx = PipelineRequestContext {
            model: "xiaomi/mimo-v2-flash",
            ..Default::default()
        };
        let (id, provider, _) = resolve_upstream_profile_id(&globals, &profiles, &ctx);
        assert_eq!(id, "mimo");
        assert_eq!(provider, UpstreamProvider::Mimo);
    }

    #[test]
    fn legacy_codex_profile_id_selects_codex_relay() {
        let globals = PipelineGlobals::default();
        let profiles = vec![ProfileDescriptor {
            id: "codex".into(),
            provider: UpstreamProvider::Openai,
        }];
        let ctx = PipelineRequestContext {
            model: "gpt-5-codex",
            ..Default::default()
        };
        let sel = select_request_pipeline(&globals, &profiles, &ctx);
        assert_eq!(sel.upstream_profile_id, "codex");
        assert_eq!(sel.provider, UpstreamProvider::Codex);
        assert_eq!(sel.pipeline, RequestPipeline::CodexRelay);
        assert_eq!(sel.reason, PipelineSelectionReason::CodexProvider);
    }

    #[test]
    fn gpt_model_ignores_key_mimo_pipeline_override() {
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
            key_pipeline: Some(PipelineOverride::MimoTokenPlanRelay),
            key_upstream_profile: Some("mimo"),
            ..Default::default()
        };
        let sel = select_request_pipeline(&globals, &profiles, &ctx);
        assert_eq!(sel.upstream_profile_id, "codex");
        assert_eq!(sel.pipeline, RequestPipeline::CodexRelay);
        assert_ne!(sel.pipeline, RequestPipeline::MimoTokenPlanRelay);
    }
}
