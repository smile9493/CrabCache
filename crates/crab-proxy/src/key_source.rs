use serde::Serialize;

/// Describes where a profile's key pool was populated from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum KeySource {
    /// `[[upstream.profiles.keys]]` structured table array.
    Structured,
    /// `keys = ["sk-..."]` inline string array on the profile.
    LegacyInline,
    /// Global `[upstream].keys` fallback (DeepSeek-only).
    LegacyGlobal,
    /// No keys configured.
    None,
}

impl KeySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Structured => "structured",
            Self::LegacyInline => "legacy_inline",
            Self::LegacyGlobal => "legacy_global",
            Self::None => "none",
        }
    }
}
