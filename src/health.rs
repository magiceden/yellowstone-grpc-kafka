use {
    tokio::{
        fs::File,
        io::{self, AsyncWriteExt},
    },
    tracing::{error, info},
};

async fn touch_ping() -> io::Result<()> {
    let mut file = File::create("ping.txt").await?;
    file.write_all(b"ping").await?;
    Ok(())
}

pub async fn ack_ping() {
    info!("ping");
    touch_ping().await.unwrap_or_else(|error| {
        error!("could not touch ping.txt: {}", error);
    });
}
