use {
    rdkafka::config::ClientConfig,
    std::{collections::HashMap, env, net::SocketAddr},
};

pub struct KafkaEnvConfig {
    pub topic: String,
    pub queue_size: usize,
}

pub struct RequestEnvConfig {
    pub url: String,
    pub body: String,
}

pub struct EnvConfig {
    pub kafka: KafkaEnvConfig,
    pub request: RequestEnvConfig,
    pub kafka_config: ClientConfig,
    pub prometheus_address: Option<SocketAddr>,
}

impl EnvConfig {
    pub fn load() -> EnvConfig {
        EnvConfig {
            kafka: build_kafka_env_config(),
            request: build_request_env_config(),
            kafka_config: build_kafka_config(),
            prometheus_address: parse_prometheus_address(),
        }
    }
}

fn required_env_value(name: &str) -> String {
    env::var(name).expect(name)
}

fn optional_env_value(name: &str) -> Option<String> {
    env::var(name).ok()
}

fn build_kafka_env_config() -> KafkaEnvConfig {
    let topic = required_env_value("KAFKA_TOPIC");
    let queue_size = required_env_value("KAFKA_QUEUE_SIZE")
        .parse()
        .expect("KAFKA_QUEUE_SIZE");

    KafkaEnvConfig { topic, queue_size }
}

fn build_request_env_config() -> RequestEnvConfig {
    let body = required_env_value("REQUEST_BODY");
    let urls = required_env_value("REQUEST_URLS");
    let key = required_env_value("REQUEST_KEY");

    let mut url_map: HashMap<String, String> = json5::from_str(&urls).expect(&urls);
    let url = url_map.remove(&key).expect(&key);

    RequestEnvConfig { url, body }
}

fn build_kafka_config() -> ClientConfig {
    let mut kafka_config = ClientConfig::new();

    let required_configs = [
        ("bootstrap.servers", "KAFKA_BOOTSTRAP_SERVERS"),
        ("statistics.interval.ms", "KAFKA_STATISTICS_INTERVAL_MS"),
    ];

    let optional_configs = [
        ("security.protocol", "KAFKA_SECURITY_PROTOCOL"),
        ("sasl.mechanisms", "KAFKA_SASL_MECHANISMS"),
        ("sasl.username", "KAFKA_SASL_USERNAME"),
        ("sasl.password", "KAFKA_SASL_PASSWORD"),
        ("broker.address.family", "KAFKA_BROKER_ADDRESS_FAMILY"),
    ];

    for (config_name, env_name) in required_configs {
        kafka_config.set(config_name, required_env_value(env_name));
    }

    for (config_name, env_name) in optional_configs {
        if let Some(value) = optional_env_value(env_name) {
            kafka_config.set(config_name, value);
        }
    }

    kafka_config
}

fn parse_prometheus_address() -> Option<SocketAddr> {
    let prometheus_address = optional_env_value("PROMETHEUS_ADDRESS");

    if let Some(address) = prometheus_address {
        Some(address.parse().expect("PROMETHEUS_ADDRESS"))
    } else {
        None
    }
}
