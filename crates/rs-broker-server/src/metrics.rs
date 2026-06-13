//! Prometheus metrics for rs-broker.
//!
//! Provides a [`Metrics`] struct that owns a [`prometheus::Registry`] and a set
//! of pre-registered counters, histograms, and gauges covering the outbox
//! publish pipeline, the inbox store pipeline, and the active subscriber set.

use std::sync::Arc;

use prometheus::{
    Encoder, Histogram, HistogramOpts, IntCounterVec, IntGauge, Opts, Registry, TextEncoder,
};

/// Bundles all Prometheus metrics for the broker.
///
/// Cloning is cheap: it shares the underlying registry and metric vectors via
/// `Arc`, so handle clones can be passed freely to publishers, consumers, and
/// HTTP handlers.
#[derive(Clone)]
pub struct Metrics {
    inner: Arc<MetricsInner>,
}

struct MetricsInner {
    registry: Registry,
    // Recording fields are the public metrics API. They are registered with the
    // registry in `Metrics::new` and exposed via the `Metrics` impl below; the
    // pipeline code that will call them is wired up separately.
    #[allow(dead_code)]
    outbox_messages_total: IntCounterVec,
    #[allow(dead_code)]
    outbox_publish_duration_seconds: Histogram,
    #[allow(dead_code)]
    inbox_messages_total: IntCounterVec,
    #[allow(dead_code)]
    subscribers_active: IntGauge,
}

impl std::fmt::Debug for Metrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Metrics").finish_non_exhaustive()
    }
}

impl Metrics {
    /// Construct a new `Metrics` instance with all metrics registered.
    pub fn new() -> anyhow::Result<Self> {
        let registry = Registry::new();

        let outbox_messages_total = IntCounterVec::new(
            Opts::new(
                "rs_broker_outbox_messages_total",
                "Total number of outbox messages processed, labeled by terminal status.",
            ),
            &["status"],
        )?;

        let outbox_publish_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "rs_broker_outbox_publish_duration_seconds",
                "Duration in seconds of an outbox publish attempt to Kafka.",
            )
            .buckets(vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ]),
        )?;

        let inbox_messages_total = IntCounterVec::new(
            Opts::new(
                "rs_broker_inbox_messages_total",
                "Total number of inbox messages received from Kafka.",
            ),
            &["status"],
        )?;

        let subscribers_active = IntGauge::new(
            "rs_broker_subscribers_active",
            "Number of currently active gRPC subscribers.",
        )?;

        registry.register(Box::new(outbox_messages_total.clone()))?;
        registry.register(Box::new(outbox_publish_duration_seconds.clone()))?;
        registry.register(Box::new(inbox_messages_total.clone()))?;
        registry.register(Box::new(subscribers_active.clone()))?;

        Ok(Self {
            inner: Arc::new(MetricsInner {
                registry,
                outbox_messages_total,
                outbox_publish_duration_seconds,
                inbox_messages_total,
                subscribers_active,
            }),
        })
    }

    /// Render the current metrics snapshot in Prometheus text exposition format.
    pub fn render(&self) -> anyhow::Result<String> {
        let encoder = TextEncoder::new();
        let metric_families = self.inner.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        Ok(String::from_utf8(buffer)?)
    }

    /// Record a successfully published outbox message.
    #[allow(dead_code)]
    pub fn record_published(&self) {
        self.inner
            .outbox_messages_total
            .with_label_values(&["published"])
            .inc();
    }

    /// Record a failed outbox publish attempt (will be retried or DLQ'd).
    #[allow(dead_code)]
    pub fn record_failed(&self) {
        self.inner
            .outbox_messages_total
            .with_label_values(&["failed"])
            .inc();
    }

    /// Record an outbox message that was routed to the dead-letter queue.
    #[allow(dead_code)]
    pub fn record_dlq(&self) {
        self.inner
            .outbox_messages_total
            .with_label_values(&["dlq"])
            .inc();
    }

    /// Observe the duration of an outbox publish attempt in seconds.
    #[allow(dead_code)]
    pub fn observe_publish_duration(&self, seconds: f64) {
        self.inner.outbox_publish_duration_seconds.observe(seconds);
    }

    /// Record a successfully stored inbox message.
    #[allow(dead_code)]
    pub fn record_inbox_stored(&self) {
        self.inner
            .inbox_messages_total
            .with_label_values(&["stored"])
            .inc();
    }

    /// Record an inbox message that failed to be stored.
    #[allow(dead_code)]
    pub fn record_inbox_failed(&self) {
        self.inner
            .inbox_messages_total
            .with_label_values(&["failed"])
            .inc();
    }

    /// Set the current number of active gRPC subscribers.
    #[allow(dead_code)]
    pub fn set_subscribers_active(&self, count: i64) {
        self.inner.subscribers_active.set(count);
    }

    /// Increment the active subscriber gauge.
    #[allow(dead_code)]
    pub fn inc_subscribers_active(&self) {
        self.inner.subscribers_active.inc();
    }

    /// Decrement the active subscriber gauge.
    #[allow(dead_code)]
    pub fn dec_subscribers_active(&self) {
        self.inner.subscribers_active.dec();
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new().expect("failed to construct default Metrics")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_registers_all_metrics() {
        let metrics = Metrics::new().expect("Metrics::new should succeed");
        // Touch each label set / metric so the text encoder exposes them.
        metrics.record_published();
        metrics.record_inbox_stored();
        metrics.set_subscribers_active(0);
        metrics.observe_publish_duration(0.0);

        let rendered = metrics.render().expect("render should succeed");
        assert!(rendered.contains("rs_broker_outbox_messages_total"));
        assert!(rendered.contains("rs_broker_outbox_publish_duration_seconds"));
        assert!(rendered.contains("rs_broker_inbox_messages_total"));
        assert!(rendered.contains("rs_broker_subscribers_active"));
    }

    #[test]
    fn record_methods_increment_labeled_counters() {
        let metrics = Metrics::new().unwrap();
        metrics.record_published();
        metrics.record_published();
        metrics.record_failed();
        metrics.record_dlq();

        let rendered = metrics.render().unwrap();
        assert!(rendered.contains(r#"rs_broker_outbox_messages_total{status="published"} 2"#));
        assert!(rendered.contains(r#"rs_broker_outbox_messages_total{status="failed"} 1"#));
        assert!(rendered.contains(r#"rs_broker_outbox_messages_total{status="dlq"} 1"#));
    }

    #[test]
    fn publish_duration_histogram_observes_values() {
        let metrics = Metrics::new().unwrap();
        metrics.observe_publish_duration(0.123);
        metrics.observe_publish_duration(2.5);

        let rendered = metrics.render().unwrap();
        assert!(rendered.contains("rs_broker_outbox_publish_duration_seconds_bucket"));
        assert!(rendered.contains("rs_broker_outbox_publish_duration_seconds_count 2"));
    }

    #[test]
    fn inbox_and_subscriber_metrics_work() {
        let metrics = Metrics::new().unwrap();
        metrics.record_inbox_stored();
        metrics.record_inbox_failed();
        metrics.set_subscribers_active(5);

        let rendered = metrics.render().unwrap();
        assert!(rendered.contains(r#"rs_broker_inbox_messages_total{status="stored"} 1"#));
        assert!(rendered.contains(r#"rs_broker_inbox_messages_total{status="failed"} 1"#));
        assert!(rendered.contains("rs_broker_subscribers_active 5"));

        metrics.inc_subscribers_active();
        metrics.dec_subscribers_active();
        let rendered = metrics.render().unwrap();
        assert!(rendered.contains("rs_broker_subscribers_active 5"));
    }

    #[test]
    fn metrics_is_send_sync_and_clone() {
        fn assert_send_sync_clone<T: Send + Sync + Clone>() {}
        assert_send_sync_clone::<Metrics>();
    }
}
