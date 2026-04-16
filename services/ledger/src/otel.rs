//! Tracing init for the ledger. Same contract as the gateway: honour
//! `OTEL_EXPORTER_OTLP_ENDPOINT` when set, otherwise fall back to fmt.

use std::env;

use opentelemetry::{global, trace::TracerProvider as _, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{runtime, trace::Config as TraceConfig, Resource};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

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
        .unwrap_or_else(|_| EnvFilter::new("ledger=info,tower_http=info"));

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
            KeyValue::new("service.name", "ledger"),
        ])))
        .install_batch(runtime::Tokio)
        .expect("otlp pipeline install");

    let tracer = tracer_provider.tracer("ledger");
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
