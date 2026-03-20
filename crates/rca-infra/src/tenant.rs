use crate::api::TenantHost;
use crate::storage::TenantKind;

pub fn tenant_host_from_kind(kind: TenantKind) -> TenantHost {
    match kind {
        TenantKind::Rain => TenantHost::Rain,
        TenantKind::Hetang => TenantHost::Hetang,
        TenantKind::Yangtze => TenantHost::Yangtze,
        TenantKind::YellowRiver => TenantHost::YellowRiver,
    }
}

pub fn core_kind_from_storage_kind(kind: TenantKind) -> rca_core::app::TenantKind {
    match kind {
        TenantKind::Rain => rca_core::app::TenantKind::Rain,
        TenantKind::Hetang => rca_core::app::TenantKind::Hetang,
        TenantKind::Yangtze => rca_core::app::TenantKind::Yangtze,
        TenantKind::YellowRiver => rca_core::app::TenantKind::YellowRiver,
    }
}

pub fn storage_kind_from_core_kind(kind: rca_core::app::TenantKind) -> TenantKind {
    match kind {
        rca_core::app::TenantKind::Rain => TenantKind::Rain,
        rca_core::app::TenantKind::Hetang => TenantKind::Hetang,
        rca_core::app::TenantKind::Yangtze => TenantKind::Yangtze,
        rca_core::app::TenantKind::YellowRiver => TenantKind::YellowRiver,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenant_host_from_kind_is_total() {
        let _ = tenant_host_from_kind(TenantKind::Rain);
        let _ = tenant_host_from_kind(TenantKind::Hetang);
        let _ = tenant_host_from_kind(TenantKind::Yangtze);
        let _ = tenant_host_from_kind(TenantKind::YellowRiver);
    }

    #[test]
    fn core_storage_kind_roundtrip() {
        for kind in [
            TenantKind::Rain,
            TenantKind::Hetang,
            TenantKind::Yangtze,
            TenantKind::YellowRiver,
        ] {
            let core = core_kind_from_storage_kind(kind);
            assert_eq!(storage_kind_from_core_kind(core), kind);
        }
    }
}
