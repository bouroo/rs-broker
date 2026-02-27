//! Retry strategy implementation

use rs_broker_config::RetryConfig;

/// Exponential backoff retry strategy
#[derive(Clone, Debug)]
pub struct RetryStrategy {
    max_retries: u32,
    initial_delay_ms: u64,
    multiplier: f64,
    max_delay_ms: u64,
    enable_dlq: bool,
    dlq_topic: String,
}

impl RetryStrategy {
    /// Create a new retry strategy from config
    pub fn new(config: RetryConfig) -> Self {
        Self {
            max_retries: config.max_retries,
            initial_delay_ms: config.initial_delay_ms,
            multiplier: config.multiplier,
            max_delay_ms: config.max_delay_ms,
            enable_dlq: config.enable_dlq,
            dlq_topic: config.dlq_topic,
        }
    }

    /// Calculate the delay for a given attempt number
    pub fn calculate_delay(&self, attempt: u32) -> std::time::Duration {
        let delay = (self.initial_delay_ms as f64 * self.multiplier.powi(attempt as i32)) as u64;
        let delay = delay.min(self.max_delay_ms);
        std::time::Duration::from_millis(delay)
    }

    /// Check if should retry
    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt < self.max_retries
    }

    /// Check if DLQ is enabled
    pub fn is_dlq_enabled(&self) -> bool {
        self.enable_dlq
    }

    /// Get DLQ topic
    pub fn dlq_topic(&self) -> &str {
        &self.dlq_topic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> RetryConfig {
        RetryConfig {
            max_retries: 5,
            initial_delay_ms: 100,
            multiplier: 2.0,
            max_delay_ms: 10000,
            enable_dlq: true,
            dlq_topic: "dlq.test".to_string(),
        }
    }

    #[test]
    fn test_retry_strategy_creation() {
        let config = create_test_config();
        let strategy = RetryStrategy::new(config);

        assert_eq!(strategy.max_retries, 5);
        assert_eq!(strategy.initial_delay_ms, 100);
        assert!((strategy.multiplier - 2.0).abs() < f64::EPSILON);
        assert_eq!(strategy.max_delay_ms, 10000);
        assert!(strategy.enable_dlq);
        assert_eq!(strategy.dlq_topic, "dlq.test");
    }

    #[test]
    fn test_calculate_delay_first_attempt() {
        let config = create_test_config();
        let strategy = RetryStrategy::new(config);

        let delay = strategy.calculate_delay(0);
        assert_eq!(delay, std::time::Duration::from_millis(100));
    }

    #[test]
    fn test_calculate_delay_exponential_growth() {
        let config = create_test_config();
        let strategy = RetryStrategy::new(config);

        let delay_0 = strategy.calculate_delay(0);
        let delay_1 = strategy.calculate_delay(1);
        let delay_2 = strategy.calculate_delay(2);

        assert_eq!(delay_0, std::time::Duration::from_millis(100));
        assert_eq!(delay_1, std::time::Duration::from_millis(200));
        assert_eq!(delay_2, std::time::Duration::from_millis(400));
    }

    #[test]
    fn test_calculate_delay_respects_max() {
        let config = RetryConfig {
            max_retries: 10,
            initial_delay_ms: 1000,
            multiplier: 10.0,
            max_delay_ms: 5000,
            enable_dlq: true,
            dlq_topic: "dlq.test".to_string(),
        };
        let strategy = RetryStrategy::new(config);

        let delay = strategy.calculate_delay(5);
        assert_eq!(delay, std::time::Duration::from_millis(5000));
    }

    #[test]
    fn test_should_retry_within_limit() {
        let config = create_test_config();
        let strategy = RetryStrategy::new(config);

        assert!(strategy.should_retry(0));
        assert!(strategy.should_retry(1));
        assert!(strategy.should_retry(2));
        assert!(strategy.should_retry(3));
        assert!(strategy.should_retry(4));
    }

    #[test]
    fn test_should_retry_at_limit() {
        let config = create_test_config();
        let strategy = RetryStrategy::new(config);

        assert!(!strategy.should_retry(5));
        assert!(!strategy.should_retry(6));
        assert!(!strategy.should_retry(100));
    }

    #[test]
    fn test_dlq_enabled() {
        let config = create_test_config();
        let strategy = RetryStrategy::new(config);

        assert!(strategy.is_dlq_enabled());
        assert_eq!(strategy.dlq_topic(), "dlq.test");
    }

    #[test]
    fn test_dlq_disabled() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay_ms: 100,
            multiplier: 2.0,
            max_delay_ms: 1000,
            enable_dlq: false,
            dlq_topic: String::new(),
        };
        let strategy = RetryStrategy::new(config);

        assert!(!strategy.is_dlq_enabled());
        assert_eq!(strategy.dlq_topic(), "");
    }

    #[test]
    fn test_zero_retries() {
        let config = RetryConfig {
            max_retries: 0,
            initial_delay_ms: 100,
            multiplier: 2.0,
            max_delay_ms: 1000,
            enable_dlq: false,
            dlq_topic: String::new(),
        };
        let strategy = RetryStrategy::new(config);

        assert!(!strategy.should_retry(0));
        assert!(!strategy.should_retry(1));
    }

    #[test]
    fn test_clone() {
        let config = create_test_config();
        let strategy = RetryStrategy::new(config);
        let cloned = strategy.clone();

        assert_eq!(strategy.max_retries, cloned.max_retries);
        assert_eq!(strategy.initial_delay_ms, cloned.initial_delay_ms);
        assert_eq!(strategy.max_delay_ms, cloned.max_delay_ms);
    }
}
