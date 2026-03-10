use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    Layer, Registry,
    filter::{EnvFilter, LevelFilter},
    fmt,
    layer::SubscriberExt,
};

pub fn init_logger(log_dir: PathBuf, default_level: &str) -> Vec<WorkerGuard> {
    let mut guards = Vec::new();
    std::fs::create_dir_all(&log_dir).unwrap_or_default();

    // Standard console env filter
    // Base env filter. By default, third-party libraries (hyper, h2, rustls, etc) are too noisy at debug/trace.
    // So if the user doesn't provide RUST_LOG, we default to info *globally*, but allow our own app
    // to use the requested `default_level` (which might be trace or debug).
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!(
            "info,rca_core={},rca_infra={},rca_cli={},rca_desktop={}",
            default_level, default_level, default_level, default_level
        ))
    });

    // Console layer
    let console_layer = fmt::layer()
        .with_ansi(true)
        .with_target(true)
        .with_thread_ids(false)
        .with_line_number(true)
        .with_filter(env_filter);

    // Filter fn needs to check metadata
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
        .with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
            *metadata.level() == tracing::Level::WARN
        }));

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
        .with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
            *metadata.level() == tracing::Level::INFO
        }));

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
        .with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
            *metadata.level() == tracing::Level::DEBUG
        }));

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
        .with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
            *metadata.level() == tracing::Level::TRACE
        }));

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
        .with_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new(format!(
                "info,rca_core={},rca_infra={},rca_cli={},rca_desktop={}",
                default_level, default_level, default_level, default_level
            ))
        }));

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
