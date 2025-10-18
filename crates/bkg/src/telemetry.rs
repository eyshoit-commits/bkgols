use std::time::Duration;

use tracing::{info, warn};

use crate::config::TelemetryConfig;

/// Guard type responsible for keeping telemetry exporters alive during runtime.
#[derive(Debug, Clone)]
pub struct TelemetryGuard {
    enabled: bool,
    otel_endpoint: Option<String>,
    sampling_interval: Option<Duration>,
}

impl TelemetryGuard {
    pub fn initialize(config: &TelemetryConfig) -> Self {
        if !config.enabled {
            warn!("telemetry disabled by configuration");
            return Self {
                enabled: false,
                otel_endpoint: None,
                sampling_interval: None,
            };
        }

        if let Some(endpoint) = &config.otel_endpoint {
            info!(target: "telemetry", endpoint, "starting OpenTelemetry exporter");
        } else {
            info!(target: "telemetry", "using stdout telemetry sink");
        }

        if let Some(interval) = config.sampling_interval {
            info!(target: "telemetry", %interval.as_secs_f32(), "telemetry sampling interval (seconds)");
        }

        Self {
            enabled: config.enabled,
            otel_endpoint: config.otel_endpoint.clone(),
            sampling_interval: config.sampling_interval,
        }
    }

    pub fn shutdown(&self) {
        if self.enabled {
            info!(target: "telemetry", "flushing telemetry pipelines");
        }
    }
}
