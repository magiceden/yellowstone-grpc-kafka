use {
    anyhow::Context,
    futures::{future::BoxFuture, stream::StreamExt, SinkExt},
    rdkafka::producer::FutureRecord,
    sha2::{Digest, Sha256},
    std::time::Duration,
    tokio::task::JoinSet,
    tokio_tungstenite::{connect_async, tungstenite::protocol::Message},
    tracing::{debug, warn},
    websocket_source_v1::{
        create_shutdown, env::EnvConfig, health::ack_ping, kafka::metrics,
        metrics::run_server as prometheus_run_server, setup_tracing,
    },
};

async fn ws2kafka(config: EnvConfig, mut shutdown: BoxFuture<'static, ()>) -> anyhow::Result<()> {
    // Connect to kafka
    let (kafka, kafka_error_rx) =
        metrics::StatsContext::create_future_producer(&config.kafka_config)
            .context("failed to create kafka producer")?;
    let mut kafka_error = false;
    tokio::pin!(kafka_error_rx);

    // Connect to websocket
    let (ws_stream, _response) = connect_async(&config.websocket_request_url).await?;
    let (mut write_sink, mut read_stream) = ws_stream.split();

    // Create an interval to ping every 10 seconds
    let mut ping_interval = tokio::time::interval(Duration::from_millis(10000));

    // Subscribe to events
    write_sink
        .send(Message::text(config.websocket_request_body))
        .await?;

    // Receive-send loop
    let mut send_tasks = JoinSet::new();
    loop {
        let message = tokio::select! {
            _ = &mut shutdown => break,
            _ = &mut kafka_error_rx => {
                kafka_error = true;
                break;
            }
            _ = ping_interval.tick() => {
                tokio::spawn(async {
                    ack_ping().await;
                });
                continue;
            }
            maybe_result = send_tasks.join_next() => match maybe_result {
                Some(result) => {
                    result??;
                    continue;
                }
                None => tokio::select! {
                    _ = &mut shutdown => break,
                    _ = &mut kafka_error_rx => {
                        kafka_error = true;
                        break;
                    }
                    _ = ping_interval.tick() => {
                        tokio::spawn(async {
                            ack_ping().await;
                        });
                        continue;
                    }
                    message = read_stream.next() => message,
                }
            },
            message = read_stream.next() => message,
        }
        .transpose()?;

        match message {
            Some(message) => {
                if message.is_close() {
                    break;
                }
                if !message.is_text() {
                    continue;
                }
                let payload = message.to_string();
                let hash = Sha256::digest(&payload);
                let key = const_hex::encode(hash);

                let record = FutureRecord::to(&config.kafka_topic)
                    .key(&key)
                    .payload(&payload);

                match kafka.send_result(record) {
                    Ok(future) => {
                        let _ = send_tasks.spawn(async move {
                            let result = future.await;
                            debug!("kafka send message with key: {key}, result: {result:?}");

                            let _ = result?.map_err(|(error, _message)| error)?;
                            metrics::sent_inc();
                            Ok::<(), anyhow::Error>(())
                        });
                        if send_tasks.len() >= config.kafka_queue_size {
                            tokio::select! {
                                _ = &mut shutdown => break,
                                _ = &mut kafka_error_rx => {
                                    kafka_error = true;
                                    break;
                                }
                                result = send_tasks.join_next() => {
                                    if let Some(result) = result {
                                        result??;
                                    }
                                }
                            }
                        }
                    }
                    Err(error) => return Err(error.0.into()),
                }
            }
            None => break,
        }
    }
    if !kafka_error {
        warn!("shutdown received...");
        loop {
            tokio::select! {
                _ = &mut kafka_error_rx => break,
                result = send_tasks.join_next() => match result {
                    Some(result) => result??,
                    None => break
                }
            }
        }
    }
    Ok(())
}

async fn run(config: EnvConfig) -> anyhow::Result<()> {
    let shutdown = create_shutdown()?;
    ws2kafka(config, shutdown).await
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    setup_tracing()?;

    // Parse config values from env
    let config = EnvConfig::load();

    // Run prometheus server
    if let Some(address) = config.prometheus_address {
        prometheus_run_server(address).await?;
    }

    // Run main event loop
    run(config).await
}
