use core_crypto::CryptoError;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EventError {
    #[error("event cannot be encoded or decoded")]
    Encoding,
    #[error("event is larger than {max} bytes ({size})")]
    TooLarge { size: usize, max: usize },
    #[error("event id does not match its content")]
    BadId,
    #[error("signature verification failed")]
    BadSignature,
    #[error("malformed event: {0}")]
    Structure(&'static str),
    #[error("invalid genesis: {0}")]
    Genesis(&'static str),
    #[error("the content of this event was erased")]
    Erased,
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}
