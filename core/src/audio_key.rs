use std::{collections::HashMap, fmt, io::Write, time::Duration};

use byteorder::{BigEndian, ByteOrder, WriteBytesExt};
use bytes::Bytes;
use thiserror::Error;
use tokio::sync::oneshot;

use crate::{Error, FileId, SpotifyId, packet::PacketType, util::SeqGenerator};

#[derive(Debug, Hash, PartialEq, Eq, Copy, Clone)]
pub struct AudioKey(pub [u8; 16]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AudioKeyCode(pub u16);

impl AudioKeyCode {
    pub const DENIED: Self = Self(0x0001);

    pub fn retryable(self) -> bool {
        self != Self::DENIED
    }
}

impl fmt::Display for AudioKeyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:04x}", self.0)
    }
}

#[derive(Debug, Error)]
pub enum AudioKeyError {
    #[error("audio key refused with code {0}")]
    Refused(AudioKeyCode),
    #[error("audio key denied with code {0}")]
    Denied(AudioKeyCode),
    #[error("other end of channel disconnected")]
    Channel,
    #[error("unexpected packet type {0}")]
    Packet(u8),
    #[error("sequence {0} not pending")]
    Sequence(u32),
    #[error("audio key response timeout")]
    Timeout,
}

impl AudioKeyError {
    fn refusal(code: AudioKeyCode) -> Self {
        match code.retryable() {
            true => Self::Refused(code),
            false => Self::Denied(code),
        }
    }
}

impl From<AudioKeyError> for Error {
    fn from(err: AudioKeyError) -> Self {
        match err {
            AudioKeyError::Refused(_) => Error::unavailable(err),
            AudioKeyError::Denied(_) => Error::permission_denied(err),
            AudioKeyError::Channel => Error::aborted(err),
            AudioKeyError::Sequence(_) => Error::aborted(err),
            AudioKeyError::Packet(_) => Error::unimplemented(err),
            AudioKeyError::Timeout => Error::aborted(err),
        }
    }
}

component! {
    AudioKeyManager : AudioKeyManagerInner {
        sequence: SeqGenerator<u32> = SeqGenerator::new(0),
        pending: HashMap<u32, oneshot::Sender<Result<AudioKey, Error>>> = HashMap::new(),
        denied: bool = false,
    }
}

impl AudioKeyManager {
    pub(crate) fn dispatch(&self, cmd: PacketType, mut data: Bytes) -> Result<(), Error> {
        if data.len() < 4 {
            return Err(AudioKeyError::Packet(cmd as u8).into());
        }
        let seq = BigEndian::read_u32(data.split_to(4).as_ref());

        let sender = self
            .lock(|inner| inner.pending.remove(&seq))
            .ok_or(AudioKeyError::Sequence(seq))?;

        match cmd {
            PacketType::AesKey => {
                if data.len() < 16 {
                    error!("audio key {seq} was {} bytes, expected 16", data.len());
                    sender
                        .send(Err(AudioKeyError::Packet(cmd as u8).into()))
                        .map_err(|_| AudioKeyError::Channel)?;
                    return Ok(());
                }
                let mut key = [0u8; 16];
                key.copy_from_slice(&data.as_ref()[..16]);
                sender
                    .send(Ok(AudioKey(key)))
                    .map_err(|_| AudioKeyError::Channel)?
            }
            PacketType::AesKeyError => {
                let code = match data.len() >= 2 {
                    true => AudioKeyCode(BigEndian::read_u16(&data.as_ref()[..2])),
                    false => {
                        warn!("audio key error payload was {} bytes", data.len());
                        AudioKeyCode(0)
                    }
                };
                error!("error audio key {seq}: {code}");
                if !code.retryable() {
                    self.lock(|inner| inner.denied = true);
                }
                sender
                    .send(Err(AudioKeyError::refusal(code).into()))
                    .map_err(|_| AudioKeyError::Channel)?
            }
            _ => {
                trace!("Did not expect {cmd:?} AES key packet with data {data:#?}");
                return Err(AudioKeyError::Packet(cmd as u8).into());
            }
        }

        Ok(())
    }

    pub fn is_denied(&self) -> bool {
        self.lock(|inner| inner.denied)
    }

    pub async fn request(&self, track: SpotifyId, file: FileId) -> Result<AudioKey, Error> {
        let (tx, rx) = oneshot::channel();

        let seq = self.lock(move |inner| {
            let seq = inner.sequence.get();
            inner.pending.insert(seq, tx);
            seq
        });

        self.send_key_request(seq, track, file)?;
        const KEY_RESPONSE_TIMEOUT: Duration = Duration::from_millis(1500);
        match tokio::time::timeout(KEY_RESPONSE_TIMEOUT, rx).await {
            Err(_) => {
                error!("Audio key response timeout");
                self.lock(|inner| inner.pending.remove(&seq));
                Err(AudioKeyError::Timeout.into())
            }
            Ok(k) => k?,
        }
    }

    fn send_key_request(&self, seq: u32, track: SpotifyId, file: FileId) -> Result<(), Error> {
        let mut data: Vec<u8> = Vec::new();
        data.write_all(&file.0)?;
        data.write_all(&track.to_raw())?;
        data.write_u32::<BigEndian>(seq)?;
        data.write_u16::<BigEndian>(0x0000)?;

        self.session().send_packet(PacketType::RequestKey, data)
    }
}
