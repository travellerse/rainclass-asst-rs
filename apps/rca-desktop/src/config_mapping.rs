use rca_core::app::{AppConfigDto, TenantKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfigFromUi {
    pub(crate) monitor_interval: i32,
    pub(crate) auto_checkin: bool,
    pub(crate) auto_answer: bool,
    pub(crate) auto_answer_random_guess: bool,
    pub(crate) answer_delay: i32,
    pub(crate) notify_enabled: bool,
    pub(crate) webhook_url: String,
    pub(crate) check_update_on_startup: bool,
    pub(crate) active_tenant: String,
}

pub(crate) fn build_config_dto_from_ui_values(values: ConfigFromUi) -> AppConfigDto {
    let tenant = TenantKind::parse_config_str(&values.active_tenant).unwrap_or_default();
    AppConfigDto {
        monitor_interval_secs: values.monitor_interval.max(1) as u64,
        auto_checkin_enabled: values.auto_checkin,
        auto_answer_enabled: values.auto_answer,
        auto_answer_random_guess: values.auto_answer_random_guess,
        auto_danmu_enabled: true,
        danmu_threshold: 4,
        answer_delay_ms: values.answer_delay.max(0) as u64,
        answer_delay_type: 1,
        answer_delay_custom_percent: 50,
        notify_enabled: values.notify_enabled,
        notify_events: Default::default(),
        webhook_url: values.webhook_url,
        check_update_on_startup: values.check_update_on_startup,
        tenant,
        auth_state_hint: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_config_dto_clamps_monitor_interval_and_delay() {
        let values = ConfigFromUi {
            monitor_interval: 0,
            auto_checkin: true,
            auto_answer: false,
            auto_answer_random_guess: true,
            answer_delay: -5,
            notify_enabled: true,
            webhook_url: "https://example.invalid/hook".to_string(),
            check_update_on_startup: true,
            active_tenant: "hetang".to_string(),
        };
        let dto = build_config_dto_from_ui_values(values);

        assert_eq!(dto.monitor_interval_secs, 1);
        assert_eq!(dto.answer_delay_ms, 0);
        assert!(dto.auto_checkin_enabled);
        assert!(!dto.auto_answer_enabled);
        assert!(dto.auto_answer_random_guess);
        assert!(dto.notify_enabled);
        assert_eq!(dto.webhook_url, "https://example.invalid/hook");
        assert!(dto.check_update_on_startup);
        assert_eq!(dto.tenant, TenantKind::Hetang);
        assert_eq!(dto.danmu_threshold, 4);
        assert!(dto.auto_danmu_enabled);
        assert_eq!(dto.answer_delay_type, 1);
        assert_eq!(dto.answer_delay_custom_percent, 50);
        assert_eq!(dto.auth_state_hint, None);
    }

    #[test]
    fn build_config_dto_preserves_positive_values() {
        let values = ConfigFromUi {
            monitor_interval: 15,
            auto_checkin: false,
            auto_answer: true,
            auto_answer_random_guess: false,
            answer_delay: 1200,
            notify_enabled: false,
            webhook_url: "".to_string(),
            check_update_on_startup: false,
            active_tenant: "rain".to_string(),
        };
        let dto = build_config_dto_from_ui_values(values);

        assert_eq!(dto.monitor_interval_secs, 15);
        assert_eq!(dto.answer_delay_ms, 1200);
        assert!(!dto.auto_checkin_enabled);
        assert!(dto.auto_answer_enabled);
        assert!(!dto.auto_answer_random_guess);
        assert!(!dto.notify_enabled);
        assert_eq!(dto.webhook_url, "");
        assert!(!dto.check_update_on_startup);
        assert_eq!(dto.tenant, TenantKind::Rain);
    }
}
