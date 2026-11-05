//! Attachments (§9.4): preparation of photos, encryption and storage of blobs.
//!
//! A blob is the encrypted full image (`postcard(Sealed)`), addressed by the
//! BLAKE3 hash of those bytes. The thumbnail travels inside the encrypted
//! event payload. Blobs are already encrypted, so they can sit in plain files.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::PathBuf;

use core_crypto::{ChannelKey, ChannelRef, Sealed};
use core_events::OrgId;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageReader};

pub const FULL_SIZE: u32 = 2048;
pub const THUMB_SIZE: u32 = 256;
pub const JPEG_QUALITY: u8 = 80;
pub const THUMB_QUALITY: u8 = 60;

pub trait BlobStore: Send {
    fn contains(&self, hash: &[u8; 32]) -> bool;
    fn get(&self, hash: &[u8; 32]) -> Result<Option<Vec<u8>>, String>;
    fn put(&mut self, hash: &[u8; 32], data: &[u8]) -> Result<(), String>;
    fn remove(&mut self, hash: &[u8; 32]) -> Result<(), String>;
    /// Every stored blob (backup).
    fn hashes(&self) -> Vec<[u8; 32]>;
}

#[derive(Debug, Default, Clone)]
pub struct MemBlobStore {
    items: BTreeMap<[u8; 32], Vec<u8>>,
}

impl BlobStore for MemBlobStore {
    fn contains(&self, hash: &[u8; 32]) -> bool {
        self.items.contains_key(hash)
    }
    fn get(&self, hash: &[u8; 32]) -> Result<Option<Vec<u8>>, String> {
        Ok(self.items.get(hash).cloned())
    }
    fn put(&mut self, hash: &[u8; 32], data: &[u8]) -> Result<(), String> {
        self.items.insert(*hash, data.to_vec());
        Ok(())
    }
    fn remove(&mut self, hash: &[u8; 32]) -> Result<(), String> {
        self.items.remove(hash);
        Ok(())
    }
    fn hashes(&self) -> Vec<[u8; 32]> {
        self.items.keys().copied().collect()
    }
}

/// Blobs as files named by their hash.
#[derive(Debug, Clone)]
pub struct DirBlobStore {
    dir: PathBuf,
}

impl DirBlobStore {
    pub fn new(dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(Self { dir })
    }

    fn path(&self, hash: &[u8; 32]) -> PathBuf {
        self.dir
            .join(blake3::Hash::from_bytes(*hash).to_hex().as_str())
    }
}

impl BlobStore for DirBlobStore {
    fn contains(&self, hash: &[u8; 32]) -> bool {
        self.path(hash).is_file()
    }
    fn get(&self, hash: &[u8; 32]) -> Result<Option<Vec<u8>>, String> {
        match std::fs::read(self.path(hash)) {
            Ok(d) => Ok(Some(d)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn put(&mut self, hash: &[u8; 32], data: &[u8]) -> Result<(), String> {
        let tmp = self.path(hash).with_extension("part");
        std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, self.path(hash)).map_err(|e| e.to_string())
    }
    fn remove(&mut self, hash: &[u8; 32]) -> Result<(), String> {
        match std::fs::remove_file(self.path(hash)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
    fn hashes(&self) -> Vec<[u8; 32]> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                blake3::Hash::from_hex(name.as_bytes())
                    .ok()
                    .map(|h| *h.as_bytes())
            })
            .collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PhotoError {
    #[error("unreadable image: {0}")]
    Decode(String),
    #[error("cannot encode image: {0}")]
    Encode(String),
    #[error(transparent)]
    Crypto(#[from] core_crypto::CryptoError),
}

/// A photo ready to attach.
#[derive(Debug, Clone)]
pub struct PreparedPhoto {
    /// Encrypted full image: store it and put `hash` in the event's attachments.
    pub blob: Vec<u8>,
    pub hash: [u8; 32],
    /// JPEG thumbnail, to embed in the (encrypted) event payload.
    pub thumbnail: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

fn jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, PhotoError> {
    let mut out = Vec::new();
    let rgb = img.to_rgb8();
    JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(&rgb)
        .map_err(|e| PhotoError::Encode(e.to_string()))?;
    Ok(out)
}

/// Associated data of a blob: binds it to org, channel and epoch.
pub fn blob_aad(org: &OrgId, channel: &ChannelRef) -> Vec<u8> {
    core_crypto::aad(&org.0, channel, "blob")
}

/// Resizes to 2048 px on the long side, re-encodes as JPEG (which drops
/// EXIF metadata, GPS included), makes a 256 px thumbnail and encrypts the
/// full image with the channel key.
pub fn prepare_photo(
    bytes: &[u8],
    org: &OrgId,
    channel: &ChannelRef,
    key: &ChannelKey,
) -> Result<PreparedPhoto, PhotoError> {
    let img = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| PhotoError::Decode(e.to_string()))?
        .decode()
        .map_err(|e| PhotoError::Decode(e.to_string()))?;
    let full = if img.width().max(img.height()) > FULL_SIZE {
        img.resize(FULL_SIZE, FULL_SIZE, FilterType::Lanczos3)
    } else {
        img
    };
    let thumb = full.thumbnail(THUMB_SIZE, THUMB_SIZE);
    let full_jpeg = jpeg(&full, JPEG_QUALITY)?;
    let thumbnail = jpeg(&thumb, THUMB_QUALITY)?;
    let sealed = key.encrypt(&blob_aad(org, channel), &full_jpeg)?;
    let blob = postcard::to_allocvec(&sealed).map_err(|e| PhotoError::Encode(e.to_string()))?;
    let hash = *blake3::hash(&blob).as_bytes();
    Ok(PreparedPhoto {
        blob,
        hash,
        thumbnail,
        width: full.width(),
        height: full.height(),
    })
}

/// A picture of a photo field, encrypted with a key of its own.
#[derive(Debug, Clone)]
pub struct SealedAttachment {
    pub blob: Vec<u8>,
    pub hash: [u8; 32],
    pub key: [u8; 32],
}

fn attachment_aad(org: &OrgId) -> Vec<u8> {
    let mut aad = b"attachment".to_vec();
    aad.extend_from_slice(&org.0);
    aad
}

/// Resizes like [`prepare_photo`] (EXIF dropped) and encrypts with a fresh
/// key, which travels in the payload of the events that show the picture:
/// whoever reads one of them can open it, on any channel.
pub fn seal_attachment(bytes: &[u8], org: &OrgId) -> Result<SealedAttachment, PhotoError> {
    let img = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| PhotoError::Decode(e.to_string()))?
        .decode()
        .map_err(|e| PhotoError::Decode(e.to_string()))?;
    let full = if img.width().max(img.height()) > FULL_SIZE {
        img.resize(FULL_SIZE, FULL_SIZE, FilterType::Lanczos3)
    } else {
        img
    };
    let key = ChannelKey::generate()?;
    let sealed = key.encrypt(&attachment_aad(org), &jpeg(&full, JPEG_QUALITY)?)?;
    let blob = postcard::to_allocvec(&sealed).map_err(|e| PhotoError::Encode(e.to_string()))?;
    Ok(SealedAttachment {
        hash: *blake3::hash(&blob).as_bytes(),
        blob,
        key: *key.as_bytes(),
    })
}

/// Decrypts an attachment back to its JPEG.
pub fn open_attachment(blob: &[u8], org: &OrgId, key: &[u8; 32]) -> Result<Vec<u8>, PhotoError> {
    let sealed: Sealed =
        postcard::from_bytes(blob).map_err(|e| PhotoError::Decode(e.to_string()))?;
    Ok(ChannelKey::from_bytes(*key).decrypt(&attachment_aad(org), &sealed)?)
}

/// Decrypts a blob back to its JPEG.
pub fn open_blob(
    blob: &[u8],
    org: &OrgId,
    channel: &ChannelRef,
    key: &ChannelKey,
) -> Result<Vec<u8>, PhotoError> {
    let sealed: Sealed =
        postcard::from_bytes(blob).map_err(|e| PhotoError::Decode(e.to_string()))?;
    Ok(key.decrypt(&blob_aad(org, channel), &sealed)?)
}

#[cfg(test)]
mod attachment_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use image::{ImageBuffer, Rgb};

    #[test]
    fn attachment_round_trip_needs_its_key_and_org() {
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_pixel(40, 30, Rgb([10, 200, 30]));
        let mut png = Vec::new();
        img.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let org = OrgId([1; 32]);
        let a = seal_attachment(&png, &org).unwrap();
        assert_eq!(a.hash, *blake3::hash(&a.blob).as_bytes());
        let jpeg = open_attachment(&a.blob, &org, &a.key).unwrap();
        assert_eq!(image::load_from_memory(&jpeg).unwrap().width(), 40);
        assert!(open_attachment(&a.blob, &OrgId([2; 32]), &a.key).is_err());
        assert!(open_attachment(&a.blob, &org, &[0; 32]).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_crypto::{ChannelDescriptor, ChannelId};
    use image::{ImageBuffer, Rgb};

    fn sample_png(w: u32, h: u32) -> Vec<u8> {
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_fn(w, h, |x, y| Rgb([(x % 256) as u8, (y % 256) as u8, 128]));
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap_or_default();
        out
    }

    #[test]
    fn photos_are_resized_thumbnailed_and_encrypted() {
        let org = OrgId([1; 32]);
        let channel = ChannelRef {
            id: ChannelId::derive(&org.0, &ChannelDescriptor::Admin).unwrap_or(ChannelId([0; 32])),
            epoch: 0,
        };
        let key = ChannelKey::generate().unwrap_or_else(|_| ChannelKey::from_bytes([3; 32]));
        let p = prepare_photo(&sample_png(3000, 1500), &org, &channel, &key)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((p.width, p.height), (2048, 1024));
        assert!(
            p.thumbnail.len() < 20_000,
            "thumbnail {} bytes",
            p.thumbnail.len()
        );
        assert_eq!(*blake3::hash(&p.blob).as_bytes(), p.hash);
        let jpeg = open_blob(&p.blob, &org, &channel, &key).unwrap_or_default();
        assert!(
            jpeg.starts_with(&[0xFF, 0xD8]),
            "a JPEG without EXIF segment"
        );
        assert!(!jpeg.windows(4).any(|w| w == b"Exif"));
        let other = ChannelKey::from_bytes([9; 32]);
        assert!(open_blob(&p.blob, &org, &channel, &other).is_err());
        assert!(prepare_photo(b"not an image", &org, &channel, &key).is_err());
    }

    #[test]
    fn dir_store_round_trip() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let mut s = DirBlobStore::new(dir.path().join("blobs")).unwrap_or_else(|e| panic!("{e}"));
        let h = *blake3::hash(b"x").as_bytes();
        assert!(!s.contains(&h));
        s.put(&h, b"x").unwrap_or_default();
        assert_eq!(s.get(&h).unwrap_or_default(), Some(b"x".to_vec()));
        s.remove(&h).unwrap_or_default();
        assert!(!s.contains(&h));
    }
}
