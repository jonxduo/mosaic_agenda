#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CryptoError {
    #[error("the operating system random generator failed")]
    Random,
    #[error("invalid public key")]
    InvalidPublicKey,
    #[error("signature verification failed")]
    BadSignature,
    #[error("encryption failed")]
    Encrypt,
    #[error("decryption failed (wrong key or tampered data)")]
    Decrypt,
    #[error("sealed key is for another channel or epoch")]
    WrongChannel,
    #[error("no key for this channel and epoch")]
    MissingKey,
    #[error("malformed data: {0}")]
    Malformed(&'static str),
}
