#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiRuntimeMode {
    Real,
    Mock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiRuntimeSelection {
    pub mode: ApiRuntimeMode,
    pub source: &'static str,
    pub legacy_env_used: bool,
}

fn parse_bool(value: &str) -> Option<bool> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

pub fn read_env_bool(name: &str) -> Option<bool> {
    std::env::var(name).ok().and_then(|value| parse_bool(&value))
}

fn resolve_api_runtime_mode_with<F>(read_env: F) -> ApiRuntimeSelection
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(mode) = read_env("RCA_API_MODE") {
        let normalized = mode.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "real" => {
                return ApiRuntimeSelection {
                    mode: ApiRuntimeMode::Real,
                    source: "RCA_API_MODE",
                    legacy_env_used: false,
                };
            }
            "mock" => {
                return ApiRuntimeSelection {
                    mode: ApiRuntimeMode::Mock,
                    source: "RCA_API_MODE",
                    legacy_env_used: false,
                };
            }
            "auto" => {}
            _ => {}
        }
    }

    #[cfg(feature = "legacy-env")]
    if let Some(value) = read_env("RCA_USE_REAL_API") {
        if let Some(use_real_api) = parse_bool(&value) {
            return ApiRuntimeSelection {
                mode: if use_real_api {
                    ApiRuntimeMode::Real
                } else {
                    ApiRuntimeMode::Mock
                },
                source: "RCA_USE_REAL_API",
                legacy_env_used: true,
            };
        }
    }

    ApiRuntimeSelection {
        mode: ApiRuntimeMode::Real,
        source: "default",
        legacy_env_used: false,
    }
}

pub fn resolve_api_runtime_mode() -> ApiRuntimeSelection {
    resolve_api_runtime_mode_with(|name| std::env::var(name).ok())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{ApiRuntimeMode, resolve_api_runtime_mode_with};

    #[test]
    fn resolve_mode_defaults_to_real() {
        let env = HashMap::<&str, &str>::new();
        let selection = resolve_api_runtime_mode_with(|name| env.get(name).map(|v| (*v).to_string()));
        assert_eq!(selection.mode, ApiRuntimeMode::Real);
        assert_eq!(selection.source, "default");
        assert!(!selection.legacy_env_used);
    }

    #[test]
    fn resolve_mode_prefers_rca_api_mode() {
        let env = HashMap::from([("RCA_API_MODE", "mock"), ("RCA_USE_REAL_API", "1")]);
        let selection = resolve_api_runtime_mode_with(|name| env.get(name).map(|v| (*v).to_string()));
        assert_eq!(selection.mode, ApiRuntimeMode::Mock);
        assert_eq!(selection.source, "RCA_API_MODE");
        assert!(!selection.legacy_env_used);
    }

    #[test]
    fn resolve_mode_supports_legacy_boolean_env() {
        let env = HashMap::from([("RCA_USE_REAL_API", "0")]);
        let selection = resolve_api_runtime_mode_with(|name| env.get(name).map(|v| (*v).to_string()));
        assert_eq!(selection.mode, ApiRuntimeMode::Mock);
        assert_eq!(selection.source, "RCA_USE_REAL_API");
        assert!(selection.legacy_env_used);
    }

    #[test]
    fn invalid_values_fallback_to_default_real() {
        let env = HashMap::from([("RCA_API_MODE", "invalid"), ("RCA_USE_REAL_API", "maybe")]);
        let selection = resolve_api_runtime_mode_with(|name| env.get(name).map(|v| (*v).to_string()));
        assert_eq!(selection.mode, ApiRuntimeMode::Real);
        assert_eq!(selection.source, "default");
        assert!(!selection.legacy_env_used);
    }
}
