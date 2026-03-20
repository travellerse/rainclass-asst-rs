#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TenantKind {
    Rain,
    #[default]
    Hetang,
    Yangtze,
    YellowRiver,
}

impl TenantKind {
    pub fn as_config_string(self) -> &'static str {
        match self {
            TenantKind::Rain => "Rain",
            TenantKind::Hetang => "Hetang",
            TenantKind::Yangtze => "Yangtze",
            TenantKind::YellowRiver => "YellowRiver",
        }
    }

    pub fn parse_config_str(input: &str) -> Option<Self> {
        match input.trim().to_lowercase().as_str() {
            "rain" => Some(TenantKind::Rain),
            "hetang" => Some(TenantKind::Hetang),
            "yangtze" => Some(TenantKind::Yangtze),
            "yellowriver" | "yellow-river" | "yellow_river" => Some(TenantKind::YellowRiver),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenant_kind_config_string_roundtrip() {
        for kind in [
            TenantKind::Rain,
            TenantKind::Hetang,
            TenantKind::Yangtze,
            TenantKind::YellowRiver,
        ] {
            let s = kind.as_config_string();
            assert_eq!(TenantKind::parse_config_str(s), Some(kind));
        }
    }

    #[test]
    fn tenant_kind_parse_accepts_common_variants() {
        assert_eq!(
            TenantKind::parse_config_str("yellow-river"),
            Some(TenantKind::YellowRiver)
        );
        assert_eq!(
            TenantKind::parse_config_str(" YellowRiver "),
            Some(TenantKind::YellowRiver)
        );
        assert_eq!(
            TenantKind::parse_config_str("yellow_river"),
            Some(TenantKind::YellowRiver)
        );
        assert_eq!(TenantKind::parse_config_str("unknown"), None);
    }
}
