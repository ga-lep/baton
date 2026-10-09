//! Postcard encoding inside length-delimited frames.

use bytes::Bytes;
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use crate::MAX_FRAME;

/// Errors from encoding or decoding a frame payload.
#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("postcard: {0}")]
    Postcard(#[from] postcard::Error),
    #[error("frame of {0} bytes exceeds the maximum")]
    FrameTooLarge(usize),
}

/// Encodes a value to a frame payload.
///
/// # Errors
/// Fails on serialization errors or if the result exceeds [`MAX_FRAME`].
pub fn encode<T: Serialize>(value: &T) -> Result<Bytes, ProtoError> {
    let v = postcard::to_allocvec(value)?;
    if v.len() > MAX_FRAME {
        return Err(ProtoError::FrameTooLarge(v.len()));
    }
    Ok(Bytes::from(v))
}

/// Decodes a frame payload; trailing bytes are an error.
///
/// # Errors
/// Fails on oversize or malformed input; never panics.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ProtoError> {
    if bytes.len() > MAX_FRAME {
        return Err(ProtoError::FrameTooLarge(bytes.len()));
    }
    let (value, rest) = postcard::take_from_bytes(bytes)?;
    if rest.is_empty() {
        Ok(value)
    } else {
        Err(ProtoError::Postcard(postcard::Error::SerdeDeCustom))
    }
}

/// Wraps a stream in a length-delimited framer limited to [`MAX_FRAME`].
pub fn framed<S: AsyncRead + AsyncWrite>(stream: S) -> Framed<S, LengthDelimitedCodec> {
    let codec = LengthDelimitedCodec::builder()
        .max_frame_length(MAX_FRAME)
        .new_codec();
    Framed::new(stream, codec)
}
