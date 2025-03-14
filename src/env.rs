use {
    rdkafka::config::ClientConfig,
    std::{env, net::SocketAddr},
};

pub struct EnvConfig {
    pub kafka_config: ClientConfig,
    pub kafka_queue_size: usize,
    pub kafka_topic: String,
    pub prometheus_address: Option<SocketAddr>,
    pub websocket_request_body: String,
    pub websocket_request_url: String,
}

impl EnvConfig {
    pub fn load() -> EnvConfig {
        let kafka_queue_size = required_env_value("KAFKA_QUEUE_SIZE");
        let kafka_topic = required_env_value("KAFKA_TOPIC");
        let prometheus_address = optional_env_value("PROMETHEUS_ADDRESS");
        let websocket_request_body = required_env_value("WEBSOCKET_REQUEST_BODY");
        let websocket_request_url = required_env_value("WEBSOCKET_REQUEST_URL");

        EnvConfig {
            kafka_config: build_kafka_config(),
            kafka_queue_size: parse_kafka_queue_size(kafka_queue_size),
            kafka_topic,
            prometheus_address: parse_prometheus_address(prometheus_address),
            websocket_request_body,
            websocket_request_url,
        }
    }
}

fn required_env_value(name: &str) -> String {
    env::var(name).expect(name)
}

fn optional_env_value(name: &str) -> Option<String> {
    env::var(name).ok()
}

fn build_kafka_config() -> ClientConfig {
    let mut kafka_config = ClientConfig::new();

    // Required Kafka config values
    kafka_config.set(
        "bootstrap.servers",
        required_env_value("KAFKA_BOOTSTRAP_SERVERS"),
    );
    kafka_config.set(
        "statistics.interval.ms",
        required_env_value("KAFKA_STATISTICS_INTERVAL_MS"),
    );

    // Optional Kafka config values
    if let Some(kafka_security_protocol) = optional_env_value("KAFKA_SECURITY_PROTOCOL") {
        kafka_config.set("security.protocol", kafka_security_protocol);
    }
    if let Some(kafka_sasl_mechanisms) = optional_env_value("KAFKA_SASL_MECHANISMS") {
        kafka_config.set("sasl.mechanisms", kafka_sasl_mechanisms);
    }
    if let Some(kafka_sasl_username) = optional_env_value("KAFKA_SASL_USERNAME") {
        kafka_config.set("sasl.username", kafka_sasl_username);
    }
    if let Some(kafka_sasl_password) = optional_env_value("KAFKA_SASL_PASSWORD") {
        kafka_config.set("sasl.password", kafka_sasl_password);
    }
    if let Some(kafka_broker_address_family) = optional_env_value("KAFKA_BROKER_ADDRESS_FAMILY") {
        kafka_config.set("broker.address.family", kafka_broker_address_family);
    }

    kafka_config
}

fn parse_kafka_queue_size(kafka_queue_size: String) -> usize {
    kafka_queue_size.parse().expect("KAFKA_QUEUE_SIZE")
}

fn parse_prometheus_address(prometheus_address: Option<String>) -> Option<SocketAddr> {
    if let Some(address) = prometheus_address {
        Some(address.parse().expect("PROMETHEUS_ADDRESS"))
    } else {
        None
    }
}
