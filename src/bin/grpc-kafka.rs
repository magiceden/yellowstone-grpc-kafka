use {
    anyhow::Context,
    clap::{Parser, Subcommand},
    futures::{future::BoxFuture, stream::StreamExt},
    rdkafka::{config::ClientConfig, consumer::Consumer, message::Message, producer::FutureRecord},
    sha2::{Digest, Sha256},
    std::{net::SocketAddr, sync::Arc, time::Duration},
    tokio::task::JoinSet,
    tonic::transport::ClientTlsConfig,
    tracing::{debug, trace, warn, info, error},
    yellowstone_grpc_client::GeyserGrpcClient,
    yellowstone_grpc_kafka::{
        config::{load as config_load, GrpcRequestToProto},
        create_shutdown,
        health::ack_ping,
        kafka::{
            config::{Config, ConfigDedup, ConfigGrpc2Kafka, ConfigKafka2Grpc},
            dedup::KafkaDedup,
            grpc::GrpcService,
            metrics,
        },
        metrics::{run_server as prometheus_run_server, GprcMessageKind},
        setup_tracing,
    },
    yellowstone_grpc_proto::{
        prelude::{subscribe_update::UpdateOneof, SubscribeUpdate},
        prost::Message as _,
    },
};
use solana_sdk::pubkey::Pubkey;
use rand::random;
use solana_client::rpc_client::RpcClient;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::interval;
use std::collections::VecDeque;
use std::sync::Mutex;

#[derive(Debug, Clone, Parser)]
#[clap(author, version, about = "Yellowstone gRPC Kafka Tool")]
struct Args {
    /// Path to config file
    #[clap(short, long)]
    config: String,

    /// Prometheus listen address
    #[clap(long)]
    prometheus: Option<SocketAddr>,

    #[command(subcommand)]
    action: ArgsAction,
}

#[derive(Debug, Clone, Subcommand)]
enum ArgsAction {
    /// Receive data from Kafka, deduplicate and send them back to Kafka
    Dedup,
    /// Receive data from gRPC and send them to the Kafka
    #[command(name = "grpc2kafka")]
    Grpc2Kafka,
    /// Receive data from Kafka and send them over gRPC
    #[command(name = "kafka2grpc")]
    Kafka2Grpc,
}

impl ArgsAction {
    fn serialize_log_error<E: std::fmt::Debug>(err: E) -> String {
        format!("{:?}", err)
    }

    async fn run(self, config: Config, kafka_config: ClientConfig) -> anyhow::Result<()> {
        let shutdown = create_shutdown()?;
        match self {
            ArgsAction::Dedup => {
                let config = config.dedup.ok_or_else(|| {
                    anyhow::anyhow!("`dedup` section in config should be defined")
                })?;
                Self::dedup(kafka_config, config, shutdown).await
            }
            ArgsAction::Grpc2Kafka => {
                let config = config.grpc2kafka.ok_or_else(|| {
                    anyhow::anyhow!("`grpc2kafka` section in config should be defined")
                })?;
                Self::grpc2kafka(kafka_config, config, shutdown).await
            }
            ArgsAction::Kafka2Grpc => {
                let config = config.kafka2grpc.ok_or_else(|| {
                    anyhow::anyhow!("`kafka2grpc` section in config should be defined")
                })?;
                Self::kafka2grpc(kafka_config, config, shutdown).await
            }
        }
    }

    async fn dedup(
        mut kafka_config: ClientConfig,
        config: ConfigDedup,
        mut shutdown: BoxFuture<'static, ()>,
    ) -> anyhow::Result<()> {
        for (key, value) in config.kafka.into_iter() {
            kafka_config.set(key, value);
        }

        // input
        let (consumer, kafka_error_rx1) =
            metrics::StatsContext::create_stream_consumer(&kafka_config)
                .context("failed to create kafka consumer")?;
        consumer.subscribe(&[&config.kafka_input])?;

        // output
        let (kafka, kafka_error_rx2) = metrics::StatsContext::create_future_producer(&kafka_config)
            .context("failed to create kafka producer")?;

        let mut kafka_error = false;
        let kafka_error_rx = futures::future::join(kafka_error_rx1, kafka_error_rx2);
        tokio::pin!(kafka_error_rx);

        // dedup
        let dedup = config.backend.create().await?;

        // input -> output loop
        let kafka_output = Arc::new(config.kafka_output);
        let mut send_tasks = JoinSet::new();
        loop {
            let message = tokio::select! {
                _ = &mut shutdown => break,
                _ = &mut kafka_error_rx => {
                    kafka_error = true;
                    break;
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
                        message = consumer.recv() => message,
                    }
                },
                message = consumer.recv() => message,
            }?;
            metrics::recv_inc();
            trace!(
                "received message with key: {:?}",
                message.key().and_then(|k| std::str::from_utf8(k).ok())
            );

            let (key, payload) = match (
                message
                    .key()
                    .and_then(|k| String::from_utf8(k.to_vec()).ok()),
                message.payload(),
            ) {
                (Some(key), Some(payload)) => (key, payload.to_vec()),
                _ => continue,
            };
            let Some((slot, hash, bytes)) = key
                .split_once('_')
                .and_then(|(slot, hash)| slot.parse::<u64>().ok().map(|slot| (slot, hash)))
                .and_then(|(slot, hash)| {
                    let mut bytes: [u8; 32] = [0u8; 32];
                    const_hex::decode_to_slice(hash, &mut bytes)
                        .ok()
                        .map(|()| (slot, hash, bytes))
                })
            else {
                continue;
            };
            debug!("received message slot #{slot} with hash {hash}");

            let kafka = kafka.clone();
            let dedup = dedup.clone();
            let kafka_output = Arc::clone(&kafka_output);
            send_tasks.spawn(async move {
                if dedup.allowed(slot, bytes).await {
                    let record = FutureRecord::to(&kafka_output).key(&key).payload(&payload);
                    match kafka.send_result(record) {
                        Ok(future) => {
                            let result = future.await;
                            debug!("kafka send message with key: {key}, result: {result:?}");

                            result?.map_err(|(error, _message)| error)?;
                            metrics::sent_inc(GprcMessageKind::Unknown);
                            Ok::<(), anyhow::Error>(())
                        }
                        Err(error) => Err(error.0.into()),
                    }
                } else {
                    metrics::dedup_inc();
                    Ok(())
                }
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

    async fn grpc2kafka(
        mut kafka_config: ClientConfig,
        config: ConfigGrpc2Kafka,
        mut shutdown: BoxFuture<'static, ()>,
    ) -> anyhow::Result<()> {
        for (key, value) in config.kafka.into_iter() {
            kafka_config.set(key, value);
        }

        // Connect to kafka
        let (kafka, kafka_error_rx) = metrics::StatsContext::create_future_producer(&kafka_config)
            .context("failed to create kafka producer")?;
        let mut kafka_error = false;
        tokio::pin!(kafka_error_rx);

        // Create gRPC client & subscribe
        let mut client = GeyserGrpcClient::build_from_shared(config.endpoint)?
            .x_token(config.x_token)?
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(5))
            .tls_config(ClientTlsConfig::new().with_native_roots())?
            .connect()
            .await?;
        let mut geyser = client.subscribe_once(config.request.to_proto()).await?;

        // Receive-send loop
        let mut send_tasks = JoinSet::new();
        // by thomas.xiaodong, this part is about to get a new thread to compute some qps stuff
        let connection = Arc::new(RpcClient::new("https://young-twilight-sun.solana-mainnet.quiknode.pro/1a84a0b63b2865dbdecc5cc27916b8298e8c4083/".to_string()));
        let message_timestamps = Arc::new(Mutex::new(VecDeque::new()));
        let message_timestamps_ref = Arc::clone(&message_timestamps);
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(1));
            loop {
                ticker.tick().await;

                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("Time went backwards")
                    .as_millis() as u64;

                let mut timestamps = message_timestamps_ref.lock().unwrap();
                while let Some(&oldest) = timestamps.front() {
                    if now - oldest > 1000 {
                        timestamps.pop_front();
                    } else {
                        break;
                    }
                }

                let qps = timestamps.len();
                info!("Processed {} messages in the last second", qps);
            }
        });

        loop {
            let start_time = SystemTime::now();
            let message = tokio::select! {
                _ = &mut shutdown => break,
                _ = &mut kafka_error_rx => {
                    kafka_error = true;
                    break;
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
                        message = geyser.next() => message,
                    }
                },
                message = geyser.next() => message,
            }
            .transpose()?;

            let geyser_duration = start_time.elapsed().expect("Time went backwards").as_millis();
            match message {
                Some(message) => {
                    let payload = message.encode_to_vec();
                    let message = match &message.update_oneof {
                        Some(value) => value,
                        None => unreachable!("Expect valid message"),
                    };
                    let slot = match message {
                        UpdateOneof::Account(msg) => msg.slot,
                        UpdateOneof::Slot(msg) => msg.slot,
                        UpdateOneof::Transaction(msg) => msg.slot,
                        UpdateOneof::TransactionStatus(msg) => msg.slot,
                        UpdateOneof::Block(msg) => msg.slot,
                        UpdateOneof::Ping(_) => {
                            ack_ping();
                            continue;
                        }
                        UpdateOneof::Pong(_) => continue,
                        UpdateOneof::BlockMeta(msg) => msg.slot,
                        UpdateOneof::Entry(msg) => msg.slot,
                    };
                    // by thomas.xiaodong, this part is about to get us the owner of the account, so we can know which owner be a little alow
                    let owner = match message {
                        UpdateOneof::Account(msg) => msg.account.as_ref().map_or(Vec::new(), |info| info.owner.clone()),
                        UpdateOneof::Slot(_) => Vec::new(),
                        UpdateOneof::Transaction(_) => Vec::new(),
                        UpdateOneof::TransactionStatus(_) => Vec::new(),
                        UpdateOneof::Block(_) => Vec::new(),
                        UpdateOneof::Ping(_) => Vec::new(),
                        UpdateOneof::Pong(_) => Vec::new(),
                        UpdateOneof::BlockMeta(_) => Vec::new(),
                        UpdateOneof::Entry(_) => Vec::new(),
                    };

                    let hash = Sha256::digest(&payload);
                    let key = format!("{slot}_{}", const_hex::encode(hash));
                    let prom_kind = GprcMessageKind::from(message);

                    let record = FutureRecord::to(&config.kafka_topic)
                        .key(&key)
                        .payload(&payload);

                    match kafka.send_result(record) {
                        Ok(future) => {
                            let kafka_start_time = SystemTime::now();
                            let connection_ref = Arc::clone(&connection);
                            let message_timestamps_ref = Arc::clone(&message_timestamps);
                            let _ = send_tasks.spawn(async move {
                                let result = future.await;
                                let kafka_duration = kafka_start_time
                                .elapsed()
                                .expect("Time went backwards")
                                .as_millis();
                                debug!("kafka send message with key: {key}, result: {result:?}");

                                let _ = result?.map_err(|(error, _message)| error)?;
                                metrics::sent_inc(prom_kind);

                                // by thomas.xiaodong, this part is about to log the lag, log the latency
                                let now = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .expect("Time went backwards")
                                    .as_millis() as u64;
                                message_timestamps_ref.lock().unwrap().push_back(now);                         
                                 // let's get start to begin log
                                 if random::<f64>() < 0.01 {
                                    let before_block_time_call = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .expect("Time went backwards")
                                    .as_millis() as u64;
                                    match connection_ref.get_block_time(slot) {
                                        Ok(block_time) => {
                                            let after_block_time_call = SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .expect("Time went backwards")
                                            .as_millis() as u64;
                                            let block_time_duration = after_block_time_call - before_block_time_call;
                                            let current_time = SystemTime::now()
                                                .duration_since(UNIX_EPOCH)
                                                .expect("Time went backwards")
                                                .as_millis() as u64;

                                            let latency = current_time - block_time as u64 * 1000 - block_time_duration;
                                            if owner.len() == 32 {
                                                match Pubkey::try_from(owner.as_slice()) {
                                                    Ok(pubkey) => {
                                                        info!(
                                                            "accountUpdate e2e latency: {} ms, owner: {}, geyser_duration: {}ms, kafka_duration: {}ms, block_time_duration: {}ms",
                                                            latency, pubkey, geyser_duration, kafka_duration, block_time_duration
                                                        );
                                                    },
                                                    Err(_) => {
                                                        info!(
                                                            "accountUpdate e2e latency(owner i error): {} ms, geyser_duration: {}ms, kafka_duration: {}ms, block_time_duration: {}ms",
                                                            latency, geyser_duration, kafka_duration, block_time_duration
                                                        );
                                                    }
                                                };
                                            } else {
                                                info!(
                                                    "accountUpdate e2e latency: {} ms, geyser_duration: {}ms, kafka_duration: {}ms, block_time_duration: {}ms",
                                                    latency, geyser_duration, kafka_duration, block_time_duration
                                                );
                                            }
                                        }
                                        Err(err) => {
                                            error!(
                                                "Error getting block time: {}",
                                                Self::serialize_log_error(&err)
                                            );
                                        }
                                    }
                                }
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

    async fn kafka2grpc(
        mut kafka_config: ClientConfig,
        config: ConfigKafka2Grpc,
        mut shutdown: BoxFuture<'static, ()>,
    ) -> anyhow::Result<()> {
        for (key, value) in config.kafka.into_iter() {
            kafka_config.set(key, value);
        }

        let (grpc_tx, grpc_shutdown) = GrpcService::run(config.listen, config.channel_capacity)?;

        let (consumer, kafka_error_rx) =
            metrics::StatsContext::create_stream_consumer(&kafka_config)
                .context("failed to create kafka consumer")?;
        let mut kafka_error = false;
        tokio::pin!(kafka_error_rx);
        consumer.subscribe(&[&config.kafka_topic])?;

        loop {
            let message = tokio::select! {
                _ = &mut shutdown => break,
                _ = &mut kafka_error_rx => {
                    kafka_error = true;
                    break
                },
                message = consumer.recv() => message?,
            };
            metrics::recv_inc();
            debug!(
                "received message with key: {:?}",
                message.key().and_then(|k| std::str::from_utf8(k).ok())
            );

            if let Some(payload) = message.payload() {
                match SubscribeUpdate::decode(payload) {
                    Ok(message) => {
                        let _ = grpc_tx.send(message);
                    }
                    Err(error) => {
                        warn!("failed to decode message: {error}");
                    }
                }
            }
        }

        if !kafka_error {
            warn!("shutdown received...");
        }
        Ok(grpc_shutdown.await??)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    setup_tracing()?;

    // Parse args
    let args = Args::parse();
    let config = config_load::<Config>(&args.config).await?;

    // Run prometheus server
    if let Some(address) = args.prometheus.or(config.prometheus) {
        prometheus_run_server(address).await?;
    }

    // Create kafka config
    let mut kafka_config = ClientConfig::new();
    for (key, value) in config.kafka.iter() {
        kafka_config.set(key, value);
    }

    args.action.run(config, kafka_config).await
}
