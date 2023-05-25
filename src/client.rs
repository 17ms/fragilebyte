use std::{error::Error, net::SocketAddr, path::PathBuf};

use tokio::{io::AsyncWriteExt, net::TcpStream};

use crate::{
    crypto::{self, Crypto},
    sockets::SocketHandler,
    util::{new_file, FileInfo},
};

#[derive(Clone)]
pub struct Client {
    addr: SocketAddr,
    key: String,
    output: PathBuf,
}

impl Client {
    pub fn new(addr: SocketAddr, key: String, output: PathBuf) -> Self {
        Self { addr, key, output }
    }

    pub async fn connection(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut socket = TcpStream::connect(self.addr).await?;
        let mut handler = SocketHandler::new(&mut socket);
        let crypto = Crypto::new(&mut handler, true).await?;
        handler.set_crypto(crypto);

        if !self.authorize(&mut handler).await? {
            // log: invalid access key '<self.key>'
            return Ok(());
        }

        let metadata = self.metadata(&mut handler).await?;
        self.requests(&mut handler, metadata).await?;

        Ok(())
    }

    async fn authorize(
        &self,
        handler: &mut SocketHandler<'_>,
    ) -> Result<bool, Box<dyn Error + Send + Sync>> {
        let msg = self.key.as_bytes().to_vec();
        handler.send(&msg).await?;

        let buf = handler.recv().await?;
        let msg = String::from_utf8(buf)?;
        let msg = msg.trim();

        if msg == "DISCONNECT" {
            return Ok(false);
        }

        Ok(true)
    }

    async fn metadata(
        &self,
        handler: &mut SocketHandler<'_>,
    ) -> Result<Vec<FileInfo>, Box<dyn Error + Send + Sync>> {
        let buf = handler.recv().await?;
        let amt = String::from_utf8(buf.clone())?.parse::<usize>()?;
        handler.send(&buf).await?; // confirmation

        let mut metadata = Vec::new();

        while metadata.len() < amt {
            let buf = handler.recv().await?;
            let data = String::from_utf8(buf)?;

            let split = data.split(':').collect::<Vec<&str>>();
            let name = split[0].trim().to_string();
            let size = split[1].trim().parse::<u64>()?;
            let hash = split[2].trim().to_string();

            let info = FileInfo::new(name, size, hash);

            metadata.push(info);
        }

        Ok(metadata)
    }

    async fn requests(
        &self,
        handler: &mut SocketHandler<'_>,
        metadata: Vec<FileInfo>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        for file in metadata {
            let (mut handle, path) = new_file(self.output.clone(), &file.name).await?;
            let msg = file.hash.as_bytes().to_vec();
            handler.send(&msg).await?;

            // log: downloading file to <path>

            let mut remaining = file.size;

            while remaining != 0 {
                let buf = handler.recv().await?;
                handle.write_all(&buf).await?;
                handle.flush().await?;
                remaining -= buf.len() as u64;
            }

            let check_hash = crypto::try_hash(&path)?;
            let msg = check_hash.as_bytes().to_vec();
            handler.send(&msg).await?;

            if check_hash != file.hash {
                return Err("Unsuccessful file transfer, hashes don't match".into());
            } // else: log that the transfer was successful
        }

        Ok(())
    }
}
