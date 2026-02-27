-- =============================================================================
-- rs-broker Database Schema Migration
-- Version: 0001
-- Description: Initial schema for outbox, inbox, subscriber, and DLQ tables
-- =============================================================================

-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- =============================================================================
-- OUTBOX TABLE
-- Stores messages waiting to be published to Kafka
-- =============================================================================

CREATE TABLE IF NOT EXISTS outbox_messages (
    -- Primary key
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    
    -- Message identification
    message_id UUID NOT NULL UNIQUE,
    aggregate_type VARCHAR(255) NOT NULL,
    aggregate_id VARCHAR(255) NOT NULL,
    event_type VARCHAR(255) NOT NULL,
    
    -- Message content
    payload JSONB NOT NULL,
    headers JSONB,
    
    -- Kafka destination
    topic VARCHAR(255) NOT NULL,
    partition_key VARCHAR(255),
    
    -- Status tracking
    status VARCHAR(50) NOT NULL DEFAULT 'pending'::varchar,
    retry_count INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    
    -- Timestamps
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    published_at TIMESTAMP WITH TIME ZONE,
    
    -- Constraints
    CONSTRAINT outbox_status_check CHECK (
        status IN (
            'pending', 'publishing', 'published', 
            'retrying', 'failed', 'dlq'
        )
    )
);

-- Indexes for outbox performance
CREATE INDEX IF NOT EXISTS idx_outbox_status ON outbox_messages(status);
CREATE INDEX IF NOT EXISTS idx_outbox_status_created ON outbox_messages(status, created_at);
CREATE INDEX IF NOT EXISTS idx_outbox_aggregate ON outbox_messages(aggregate_type, aggregate_id);
CREATE INDEX IF NOT EXISTS idx_outbox_topic ON outbox_messages(topic);
CREATE INDEX IF NOT EXISTS idx_outbox_created_at ON outbox_messages(created_at);
-- Additional indexes for performance optimization
CREATE INDEX IF NOT EXISTS idx_outbox_status_created_at ON outbox_messages(status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_outbox_topic_created_at ON outbox_messages(topic, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_outbox_retry_count ON outbox_messages(retry_count);
CREATE INDEX IF NOT EXISTS idx_outbox_published_at ON outbox_messages(published_at);

-- =============================================================================
-- INBOX TABLE
-- Stores messages received from Kafka before processing
-- =============================================================================

CREATE TABLE IF NOT EXISTS inbox_messages (
    -- Primary key
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    
    -- Kafka metadata
    message_id UUID NOT NULL UNIQUE,
    topic VARCHAR(255) NOT NULL,
    partition INTEGER NOT NULL,
    offset BIGINT NOT NULL,
    
    -- Message content
    key VARCHAR(255),
    event_type VARCHAR(255),
    payload JSONB NOT NULL,
    headers JSONB,
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    -- Status tracking
    status VARCHAR(50) NOT NULL DEFAULT 'received'::varchar,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    
    -- Timestamps
    received_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    processed_at TIMESTAMP WITH TIME ZONE,
    
    -- Constraints
    CONSTRAINT inbox_status_check CHECK (
        status IN ('received', 'processing', 'processed', 'failed')
    )
);

-- Indexes for inbox performance
CREATE INDEX IF NOT EXISTS idx_inbox_status ON inbox_messages(status);
CREATE INDEX IF NOT EXISTS idx_inbox_message_id ON inbox_messages(message_id);
CREATE INDEX IF NOT EXISTS idx_inbox_topic_partition_offset ON inbox_messages(topic, partition, offset);
CREATE INDEX IF NOT EXISTS idx_inbox_received_at ON inbox_messages(received_at);
CREATE INDEX IF NOT EXISTS idx_inbox_status_received ON inbox_messages(status, received_at);
-- Additional indexes for performance optimization
CREATE INDEX IF NOT EXISTS idx_inbox_topic_offset ON inbox_messages(topic, offset);
CREATE INDEX IF NOT EXISTS idx_inbox_status_attempt_count ON inbox_messages(status, attempt_count);
CREATE INDEX IF NOT EXISTS idx_inbox_timestamp_status ON inbox_messages(timestamp, status);

-- =============================================================================
-- SUBSCRIBER TABLE
-- Stores registered subscribers for message delivery
-- =============================================================================

CREATE TABLE IF NOT EXISTS subscribers (
    -- Primary key
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    
    -- Subscriber identification
    subscriber_id VARCHAR(255) NOT NULL UNIQUE,
    service_name VARCHAR(255) NOT NULL,
    grpc_endpoint VARCHAR(255) NOT NULL,
    
    -- Subscription configuration
    topic_patterns TEXT[] NOT NULL,
    active BOOLEAN NOT NULL DEFAULT true,
    delivery_config JSONB,
    
    -- Timestamps
    registered_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Indexes for subscriber performance
CREATE INDEX IF NOT EXISTS idx_subscriber_service_name ON subscribers(service_name);
CREATE INDEX IF NOT EXISTS idx_subscriber_active ON subscribers(active);
CREATE INDEX IF NOT EXISTS idx_subscriber_grpc_endpoint ON subscribers(grpc_endpoint);
-- Additional indexes for performance optimization
CREATE INDEX IF NOT EXISTS idx_subscriber_topic_patterns ON subscribers USING GIN(topic_patterns);
CREATE INDEX IF NOT EXISTS idx_subscriber_registered_at ON subscribers(registered_at);

-- =============================================================================
-- INBOX DELIVERY TABLE
-- Tracks message delivery to individual subscribers
-- =============================================================================

CREATE TABLE IF NOT EXISTS inbox_deliveries (
    -- Primary key
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    
    -- Relationships
    inbox_message_id UUID NOT NULL REFERENCES inbox_messages(id) ON DELETE CASCADE,
    subscriber_id UUID NOT NULL REFERENCES subscribers(id) ON DELETE CASCADE,
    
    -- Delivery tracking
    status VARCHAR(50) NOT NULL DEFAULT 'pending'::varchar,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    
    -- Timestamps
    delivered_at TIMESTAMP WITH TIME ZONE,
    acknowledged_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    -- Constraints
    CONSTRAINT inbox_delivery_status_check CHECK (
        status IN ('pending', 'delivering', 'delivered', 'retrying', 'failed')
    ),
    
    -- Unique constraint to prevent duplicate deliveries
    UNIQUE(inbox_message_id, subscriber_id)
);

-- Indexes for inbox delivery performance
CREATE INDEX IF NOT EXISTS idx_inbox_delivery_inbox ON inbox_deliveries(inbox_message_id);
CREATE INDEX IF NOT EXISTS idx_inbox_delivery_subscriber ON inbox_deliveries(subscriber_id);
CREATE INDEX IF NOT EXISTS idx_inbox_delivery_status ON inbox_deliveries(status);

-- =============================================================================
-- DEAD LETTER QUEUE (DLQ) TABLE
-- Stores messages that failed all retry attempts
-- =============================================================================

CREATE TABLE IF NOT EXISTS dlq_messages (
    -- Primary key
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    
    -- Original message reference
    original_message_id UUID NOT NULL,
    original_topic VARCHAR(255) NOT NULL,
    dlq_topic VARCHAR(255) NOT NULL,
    
    -- Failure information
    failure_reason TEXT NOT NULL,
    retry_count INTEGER NOT NULL DEFAULT 0,
    
    -- Original message content
    payload JSONB NOT NULL,
    headers JSONB,
    
    -- Timestamps
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Indexes for DLQ performance
CREATE INDEX IF NOT EXISTS idx_dlq_original_message ON dlq_messages(original_message_id);
CREATE INDEX IF NOT EXISTS idx_dlq_original_topic ON dlq_messages(original_topic);
CREATE INDEX IF NOT EXISTS idx_dlq_dlq_topic ON dlq_messages(dlq_topic);
CREATE INDEX IF NOT EXISTS idx_dlq_created_at ON dlq_messages(created_at);

-- =============================================================================
-- OUTBOX RETRY TABLE
-- Tracks scheduled retry attempts for outbox messages
-- =============================================================================

CREATE TABLE IF NOT EXISTS outbox_retries (
    -- Primary key
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    
    -- Message reference
    outbox_message_id UUID NOT NULL REFERENCES outbox_messages(id) ON DELETE CASCADE,
    
    -- Retry tracking
    retry_count INTEGER NOT NULL,
    scheduled_at TIMESTAMP WITH TIME ZONE NOT NULL,
    executed_at TIMESTAMP WITH TIME ZONE,
    status VARCHAR(50) NOT NULL DEFAULT 'scheduled'::varchar,
    error_message TEXT,
    
    -- Constraints
    CONSTRAINT outbox_retry_status_check CHECK (
        status IN ('scheduled', 'executing', 'completed', 'failed')
    )
);

-- Indexes for outbox retry performance
CREATE INDEX IF NOT EXISTS idx_outbox_retry_message ON outbox_retries(outbox_message_id);
CREATE INDEX IF NOT EXISTS idx_outbox_retry_scheduled ON outbox_retries(scheduled_at);
CREATE INDEX IF NOT EXISTS idx_outbox_retry_status ON outbox_retries(status);

-- =============================================================================
-- FUNCTIONS AND TRIGGERS
-- =============================================================================

-- Function to update the updated_at timestamp
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to update outbox_messages.updated_at
CREATE TRIGGER update_outbox_updated_at
    BEFORE UPDATE ON outbox_messages
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

-- Trigger to update subscribers.updated_at
CREATE TRIGGER update_subscriber_updated_at
    BEFORE UPDATE ON subscribers
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

-- Trigger to update inbox_deliveries.updated_at
CREATE TRIGGER update_inbox_delivery_updated_at
    BEFORE UPDATE ON inbox_deliveries
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

-- Function to clean up old messages
CREATE OR REPLACE FUNCTION cleanup_old_messages(retention_days INTEGER DEFAULT 7)
RETURNS void AS $$
BEGIN
    -- Clean up old outbox messages that are published
    DELETE FROM outbox_messages 
    WHERE status = 'published' 
    AND published_at < NOW() - (retention_days || ' days')::INTERVAL;
    
    -- Clean up old processed inbox messages
    DELETE FROM inbox_messages 
    WHERE status = 'processed' 
    AND processed_at < NOW() - (retention_days || ' days')::INTERVAL;
    
    -- Clean up old DLQ messages
    DELETE FROM dlq_messages 
    WHERE created_at < NOW() - (retention_days || ' days')::INTERVAL;
    
    -- Clean up completed retry records
    DELETE FROM outbox_retries 
    WHERE status = 'completed' 
    AND executed_at < NOW() - (retention_days || ' days')::INTERVAL;
END;
$$ LANGUAGE plpgsql;

-- =============================================================================
-- COMMENTS
-- =============================================================================

COMMENT ON TABLE outbox_messages IS 'Stores messages waiting to be published to Kafka';
COMMENT ON TABLE inbox_messages IS 'Stores messages received from Kafka before processing';
COMMENT ON TABLE subscribers IS 'Stores registered subscribers for message delivery';
COMMENT ON TABLE inbox_deliveries IS 'Tracks message delivery to individual subscribers';
COMMENT ON TABLE dlq_messages IS 'Stores messages that failed all retry attempts';
COMMENT ON TABLE outbox_retries IS 'Tracks scheduled retry attempts for outbox messages';

-- =============================================================================
-- VERSION TRACKING
-- =============================================================================

CREATE TABLE IF NOT EXISTS schema_migrations (
    version VARCHAR(50) PRIMARY KEY,
    applied_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    description TEXT
);

INSERT INTO schema_migrations (version, description)
VALUES ('0001', 'Initial schema for outbox, inbox, subscriber, and DLQ tables')
ON CONFLICT (version) DO NOTHING;
