use crate::comms;
use aes_gcm::{
    aead::{consts::U12, AeadMut},
    aes::Aes256,
    Aes256Gcm, AesGcm, KeyInit, Nonce,
};
use rand::{distributions::Alphanumeric, Rng};
use rand::{rngs::OsRng, RngCore};
use std::{error::Error, path::Path};
use tokio::{
    io::{BufReader, BufWriter},
    net::tcp::{ReadHalf, WriteHalf},
};
use x25519_dalek::{EphemeralSecret, PublicKey, SharedSecret};

const AES_NONCE_SIZE: usize = 12;
const DH_PBK_SIZE: usize = 32;

async fn edh(
    reader: &mut BufReader<ReadHalf<'_>>,
    writer: &mut BufWriter<WriteHalf<'_>>,
    go_first: bool,
) -> Result<SharedSecret, Box<dyn Error + Send + Sync>> {
    let buf: Vec<u8>;
    let own_sec = EphemeralSecret::new(OsRng);
    let own_pbk = PublicKey::from(&own_sec);
    let msg = own_pbk.as_bytes().to_vec();

    if go_first {
        comms::send(writer, None, None, &msg).await?;
        buf = comms::recv(reader, None).await?;
    } else {
        buf = comms::recv(reader, None).await?;
        comms::send(writer, None, None, &msg).await?;
    }

    let slice: [u8; DH_PBK_SIZE] = buf[..DH_PBK_SIZE].try_into()?;
    let recv_pbk = PublicKey::from(slice);

    Ok(own_sec.diffie_hellman(&recv_pbk))
}

pub async fn aes_cipher(
    reader: &mut BufReader<ReadHalf<'_>>,
    writer: &mut BufWriter<WriteHalf<'_>>,
    go_first: bool,
) -> Result<AesGcm<Aes256, U12>, Box<dyn Error + Sync + Send>> {
    let secret = edh(reader, writer, go_first).await?;
    Ok(Aes256Gcm::new(secret.as_bytes().into()))
}

fn generate_nonce(rng: &mut impl RngCore) -> Nonce<U12> {
    let mut nonce = Nonce::default();
    rng.fill_bytes(&mut nonce);

    nonce
}

pub fn aes_encrypt(
    data: &Vec<u8>,
    cipher: &mut AesGcm<Aes256, U12>,
    rng: &mut OsRng,
) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
    let nonce = generate_nonce(rng);
    let encrypted = cipher.encrypt(&nonce, data.as_ref()).unwrap(); // TODO: handle errors
    let mut data = nonce.to_vec();
    data.extend_from_slice(&encrypted);

    Ok(data)
}

pub fn aes_decrypt(
    data: &Vec<u8>,
    cipher: &mut AesGcm<Aes256, U12>,
) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
    let (nonce_bytes, data) = data.split_at(AES_NONCE_SIZE);
    let decrypted = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), data.as_ref())
        .unwrap(); // TODO: handle errors

    Ok(decrypted)
}

pub fn try_hash(path: &String) -> Result<String, Box<dyn Error + Send + Sync>> {
    let path = Path::new(path);
    let hash = sha256::try_digest(path)?;

    Ok(hash)
}

pub fn keygen() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(8)
        .map(char::from)
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_implementations() {
        use aes_gcm::aead;

        let mut gen_rng = aead::OsRng;
        let key = Aes256Gcm::generate_key(&mut gen_rng);
        let mut cipher = Aes256Gcm::new(&key);

        let data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let mut aes_rng = OsRng;
        let enc = aes_encrypt(&data, &mut cipher, &mut aes_rng).unwrap();
        let dec = aes_decrypt(&enc, &mut cipher).unwrap();

        assert_eq!(data, dec);
    }
}
