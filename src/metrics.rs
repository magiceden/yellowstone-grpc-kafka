#[cfg(feature = "kafka")]
use crate::kafka::metrics::{
    END_TO_END_LATENCY, KAFKA_BATCH_SIZE, KAFKA_DEDUP_TOTAL, KAFKA_PRODUCE_LATENCY,
    KAFKA_QUEUE_DEPTH, KAFKA_RECV_TOTAL, KAFKA_SENT_TOTAL, KAFKA_STATS, LATEST_PROCESSED_SLOT,
    MEMORY_USAGE_BYTES, MESSAGE_LATENCY_BY_TYPE, MESSAGE_PROCESSING_LATENCY, MESSAGE_RATE,
    MESSAGE_SIZE_BYTES, MESSAGE_THROUGHPUT_BY_TYPE, OUT_OF_ORDER_SLOTS,
    QUEUE_DEPTH_HIGH_WATER_MARK, QUEUE_SATURATION_EVENTS, QUEUE_WAIT_TIME, SLOT_TIMING_DRIFT,
    SLOT_TO_RECEIVE_LATENCY,
};
use {
    crate::version::VERSION as VERSION_INFO,
    http_body_util::{combinators::BoxBody, BodyExt, Empty as BodyEmpty, Full as BodyFull},
    hyper::{
        body::{Bytes, Incoming as BodyIncoming},
        service::service_fn,
        Request, Response, StatusCode,
    },
    hyper_util::{
        rt::tokio::{TokioExecutor, TokioIo},
        server::conn::auto::Builder as ServerBuilder,
    },
    prometheus::{IntCounterVec, Opts, Registry, TextEncoder},
    std::{convert::Infallible, net::SocketAddr, sync::Once},
    tokio::net::TcpListener,
    tracing::{error, info},
    yellowstone_grpc_proto::prelude::subscribe_update::UpdateOneof,
};

lazy_static::lazy_static! {
    static ref REGISTRY: Registry = Registry::new();

    static ref VERSION: IntCounterVec = IntCounterVec::new(
        Opts::new("version", "Plugin version info"),
        &["buildts", "package", "proto", "rustc", "solana", "version"]
    ).unwrap();
}

pub async fn run_server(address: SocketAddr) -> anyhow::Result<()> {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        macro_rules! register {
            ($collector:ident) => {
                REGISTRY
                    .register(Box::new($collector.clone()))
                    .expect("collector can't be registered");
            };
        }

        register!(VERSION);
        #[cfg(feature = "kafka")]
        {
            register!(KAFKA_STATS);
            register!(KAFKA_DEDUP_TOTAL);
            register!(KAFKA_RECV_TOTAL);
            register!(KAFKA_SENT_TOTAL);
            register!(MESSAGE_PROCESSING_LATENCY);
            register!(KAFKA_PRODUCE_LATENCY);
            register!(SLOT_TO_RECEIVE_LATENCY);
            register!(END_TO_END_LATENCY);
            register!(KAFKA_QUEUE_DEPTH);
            register!(SLOT_TIMING_DRIFT);
            register!(MESSAGE_SIZE_BYTES);
            register!(OUT_OF_ORDER_SLOTS);
            register!(LATEST_PROCESSED_SLOT);
            register!(MESSAGE_LATENCY_BY_TYPE);
            register!(QUEUE_WAIT_TIME);
            register!(MESSAGE_RATE);
            register!(KAFKA_BATCH_SIZE);
            register!(MEMORY_USAGE_BYTES);
            register!(QUEUE_DEPTH_HIGH_WATER_MARK);
            register!(MESSAGE_THROUGHPUT_BY_TYPE);
            register!(QUEUE_SATURATION_EVENTS);
        }

        VERSION
            .with_label_values(&[
                VERSION_INFO.buildts,
                VERSION_INFO.package,
                VERSION_INFO.proto,
                VERSION_INFO.rustc,
                VERSION_INFO.solana,
                VERSION_INFO.version,
            ])
            .inc();
    });

    let listener = TcpListener::bind(&address).await?;
    info!("prometheus server started: {address:?}");
    tokio::spawn(async move {
        loop {
            let stream = match listener.accept().await {
                Ok((stream, _addr)) => stream,
                Err(error) => {
                    error!("failed to accept new connection: {error}");
                    break;
                }
            };
            tokio::spawn(async move {
                if let Err(error) = ServerBuilder::new(TokioExecutor::new())
                    .serve_connection(
                        TokioIo::new(stream),
                        service_fn(move |req: Request<BodyIncoming>| async move {
                            match req.uri().path() {
                                "/metrics" => metrics_handler(),
                                _ => not_found_handler(),
                            }
                        }),
                    )
                    .await
                {
                    error!("failed to handle request: {error}");
                }
            });
        }
    });

    Ok(())
}

fn metrics_handler() -> http::Result<Response<BoxBody<Bytes, Infallible>>> {
    let metrics = TextEncoder::new()
        .encode_to_string(&REGISTRY.gather())
        .unwrap_or_else(|error| {
            error!("could not encode custom metrics: {}", error);
            String::new()
        });
    Response::builder()
        .status(StatusCode::OK)
        .body(BodyFull::new(Bytes::from(metrics)).boxed())
}

fn not_found_handler() -> http::Result<Response<BoxBody<Bytes, Infallible>>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(BodyEmpty::new().boxed())
}

#[derive(Debug, Clone, Copy)]
pub enum GprcMessageKind {
    Account,
    Slot,
    Transaction,
    TransactionStatus,
    Block,
    Ping,
    Pong,
    BlockMeta,
    Entry,
    Unknown,
}

impl From<&UpdateOneof> for GprcMessageKind {
    fn from(msg: &UpdateOneof) -> Self {
        match msg {
            UpdateOneof::Account(_) => Self::Account,
            UpdateOneof::Slot(_) => Self::Slot,
            UpdateOneof::Transaction(_) => Self::Transaction,
            UpdateOneof::TransactionStatus(_) => Self::TransactionStatus,
            UpdateOneof::Block(_) => Self::Block,
            UpdateOneof::Ping(_) => Self::Ping,
            UpdateOneof::Pong(_) => Self::Pong,
            UpdateOneof::BlockMeta(_) => Self::BlockMeta,
            UpdateOneof::Entry(_) => Self::Entry,
        }
    }
}

impl GprcMessageKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            GprcMessageKind::Account => "account",
            GprcMessageKind::Slot => "slot",
            GprcMessageKind::Transaction => "transaction",
            GprcMessageKind::TransactionStatus => "transactionstatus",
            GprcMessageKind::Block => "block",
            GprcMessageKind::Ping => "ping",
            GprcMessageKind::Pong => "pong",
            GprcMessageKind::BlockMeta => "blockmeta",
            GprcMessageKind::Entry => "entry",
            GprcMessageKind::Unknown => "unknown",
        }
    }
}
