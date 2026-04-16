//! Tracing init. When `OTEL_EXPORTER_OTLP_ENDPOINT` is set (e.g.
//! `http://localhost:4317`) we export spans via OTLP/gRPC to a collector.
//! Otherwise we just fall back to the fmt subscriber so local dev keeps working.
//!
//! Returns a guard that shuts down the tracer provider on drop — this is what
//! flushes any still-batched spans. Callers should hold the guard until the
//! process is about to exit.

use std::env;

use opentelemetry::{global, trace::TracerProvider as _, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{runtime, trace::Config as TraceConfig, Resource};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Drop this to shut down the tracer provider (flushes batched spans).
pub struct OtelGuard {
    shutdown: Option<Box<dyn FnOnce() + Send>>,
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            shutdown();
        }
    }
}

pub fn init() -> OtelGuard {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("gateway=info,tower_http=info"));

    match env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok() {
        Some(endpoint) => init_with_otlp(endpoint, filter),
        None => {
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer())
                .try_init();
            OtelGuard { shutdown: None }
        }
    }
}

fn init_with_otlp(endpoint: String, filter: EnvFilter) -> OtelGuard {
    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(endpoint);

    let tracer_provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(TraceConfig::default().with_resource(Resource::new(vec![
            KeyValue::new("service.name", "gateway"),
        ])))
        .install_batch(runtime::Tokio)
        .expect("otlp pipeline install");

    let tracer = tracer_provider.tracer("gateway");
    global::set_tracer_provider(tracer_provider);

    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .with(otel_layer)
        .try_init();

    OtelGuard {
        shutdown: Some(Box::new(|| {
            global::shutdown_tracer_provider();
        })),
    }
}
