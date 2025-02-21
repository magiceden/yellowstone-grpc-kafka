use {
    std::{
        fs::OpenOptions,
        io::{self, prelude::*},
        path::Path,
    },
    tracing::{error, info},
};

fn touch(path: &Path, buf: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().create(true).write(true).open(path)?;
    file.write_all(buf)?;
    Ok(())
}

pub fn ack_ping() {
    info!("ping");
    touch(&Path::new("ping.txt"), b"ping").unwrap_or_else(|why| {
        error!("Could not write ping.txt {:?}", why.kind());
    });
}
