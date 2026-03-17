use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    Layer, Registry,
    filter::{EnvFilter, LevelFilter},
    fmt,
    layer::SubscriberExt,
};

fn default_env_filter_directives(default_level: &str) -> String {
    format!(
        "info,rca_core={},rca_infra={},rca_cli={},rca_desktop={}",
        default_level, default_level, default_level, default_level
    )
}

fn build_env_filter_for_console(default_level: &str) -> EnvFilter {
    EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_env_filter_directives(default_level)))
}

fn build_env_filter_for_app(default_level: &str) -> EnvFilter {
    // Keep console + app consistent unless the user provides RUST_LOG.
    EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_env_filter_directives(default_level)))
}

fn is_warn_level(level: tracing::Level) -> bool {
    level == tracing::Level::WARN
}

fn is_info_level(level: tracing::Level) -> bool {
    level == tracing::Level::INFO
}

fn is_debug_level(level: tracing::Level) -> bool {
    level == tracing::Level::DEBUG
}

fn is_trace_level(level: tracing::Level) -> bool {
    level == tracing::Level::TRACE
}

fn is_warn(metadata: &tracing::Metadata<'_>) -> bool {
    is_warn_level(*metadata.level())
}

fn is_info(metadata: &tracing::Metadata<'_>) -> bool {
    is_info_level(*metadata.level())
}

fn is_debug(metadata: &tracing::Metadata<'_>) -> bool {
    is_debug_level(*metadata.level())
}

fn is_trace(metadata: &tracing::Metadata<'_>) -> bool {
    is_trace_level(*metadata.level())
}

pub fn init_logger(log_dir: PathBuf, default_level: &str) -> Vec<WorkerGuard> {
    let mut guards = Vec::new();
    std::fs::create_dir_all(&log_dir).unwrap_or_default();

    // Standard console env filter
    // Base env filter. By default, third-party libraries (hyper, h2, rustls, etc) are too noisy at debug/trace.
    // So if the user doesn't provide RUST_LOG, we default to info *globally*, but allow our own app
    // to use the requested `default_level` (which might be trace or debug).
    let env_filter = build_env_filter_for_console(default_level);

    // Console layer
    let console_layer = fmt::layer()
        .with_ansi(true)
        .with_target(true)
        .with_thread_ids(false)
        .with_line_number(true)
        .with_filter(env_filter);

    // Error appender matches ERROR (and FATAL since we use target FATAL with ERROR level)
    let error_appender = tracing_appender::rolling::daily(log_dir.clone(), "error.log");
    let (error_writer, error_guard) = tracing_appender::non_blocking(error_appender);
    guards.push(error_guard);

    let error_layer = fmt::layer()
        .with_writer(error_writer)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_filter(LevelFilter::ERROR);

    // WARN logic
    let warn_appender = tracing_appender::rolling::daily(log_dir.clone(), "warn.log");
    let (warn_writer, warn_guard) = tracing_appender::non_blocking(warn_appender);
    guards.push(warn_guard);

    let warn_layer = fmt::layer()
        .with_writer(warn_writer)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_filter(tracing_subscriber::filter::filter_fn(is_warn));

    // INFO logic
    let info_appender = tracing_appender::rolling::daily(log_dir.clone(), "info.log");
    let (info_writer, info_guard) = tracing_appender::non_blocking(info_appender);
    guards.push(info_guard);

    let info_layer = fmt::layer()
        .with_writer(info_writer)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_filter(tracing_subscriber::filter::filter_fn(is_info));

    // DEBUG logic
    let debug_appender = tracing_appender::rolling::daily(log_dir.clone(), "debug.log");
    let (debug_writer, debug_guard) = tracing_appender::non_blocking(debug_appender);
    guards.push(debug_guard);

    let debug_layer = fmt::layer()
        .with_writer(debug_writer)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_filter(tracing_subscriber::filter::filter_fn(is_debug));

    // TRACE logic
    let trace_appender = tracing_appender::rolling::daily(log_dir.clone(), "trace.log");
    let (trace_writer, trace_guard) = tracing_appender::non_blocking(trace_appender);
    guards.push(trace_guard);

    let trace_layer = fmt::layer()
        .with_writer(trace_writer)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_filter(tracing_subscriber::filter::filter_fn(is_trace));

    // General app log including all
    let app_appender = tracing_appender::rolling::daily(log_dir.clone(), "app.log");
    let (app_writer, app_guard) = tracing_appender::non_blocking(app_appender);
    guards.push(app_guard);

    let app_layer = fmt::layer()
        .with_writer(app_writer)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_filter(build_env_filter_for_app(default_level));

    let subscriber = Registry::default()
        .with(console_layer)
        .with(error_layer)
        .with(warn_layer)
        .with(info_layer)
        .with(debug_layer)
        .with(trace_layer)
        .with(app_layer);

    tracing::subscriber::set_global_default(subscriber).expect("Failed to set tracing subscriber");

    guards
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_directives_contains_expected_modules() {
        let s = default_env_filter_directives("debug");
        assert!(s.starts_with("info,"));
        assert!(s.contains("rca_core=debug"));
        assert!(s.contains("rca_infra=debug"));
        assert!(s.contains("rca_cli=debug"));
        assert!(s.contains("rca_desktop=debug"));
    }

    #[test]
    fn build_env_filter_helpers_do_not_panic() {
        // We intentionally don't depend on the process environment.
        // The goal is to keep these helpers pure and stable for unit tests.
        let _ = build_env_filter_for_console("info");
        let _ = build_env_filter_for_app("trace");
    }

    #[test]
    fn level_predicates_match_only_expected_level() {
        assert!(is_warn_level(tracing::Level::WARN));
        assert!(!is_warn_level(tracing::Level::INFO));

        assert!(is_info_level(tracing::Level::INFO));
        assert!(!is_info_level(tracing::Level::DEBUG));

        assert!(is_debug_level(tracing::Level::DEBUG));
        assert!(!is_debug_level(tracing::Level::TRACE));

        assert!(is_trace_level(tracing::Level::TRACE));
        assert!(!is_trace_level(tracing::Level::ERROR));
    }

    #[test]
    fn metadata_filters_match_only_expected_levels() {
        // Keep this test purely about our filter predicates.
        struct Case {
            level: tracing::Level,
            warn: bool,
            info: bool,
            debug: bool,
            trace: bool,
        }

        let cases = [
            Case {
                level: tracing::Level::ERROR,
                warn: false,
                info: false,
                debug: false,
                trace: false,
            },
            Case {
                level: tracing::Level::WARN,
                warn: true,
                info: false,
                debug: false,
                trace: false,
            },
            Case {
                level: tracing::Level::INFO,
                warn: false,
                info: true,
                debug: false,
                trace: false,
            },
            Case {
                level: tracing::Level::DEBUG,
                warn: false,
                info: false,
                debug: true,
                trace: false,
            },
            Case {
                level: tracing::Level::TRACE,
                warn: false,
                info: false,
                debug: false,
                trace: true,
            },
        ];

        for c in cases {
            assert_eq!(is_warn_level(c.level), c.warn);
            assert_eq!(is_info_level(c.level), c.info);
            assert_eq!(is_debug_level(c.level), c.debug);
            assert_eq!(is_trace_level(c.level), c.trace);
        }
    }
}
