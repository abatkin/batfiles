//! Compressed streams: their decoders, and what a body declared `decompress` turns out to be.

use std::io;

use bzip2::read::MultiBzDecoder;
use flate2::read::MultiGzDecoder;
use thiserror::Error;

use super::detect::{self, Format};

/// How a stream is compressed.
#[derive(Debug, Clone, Copy)]
pub(super) enum Compression {
    None,
    Gzip,
    Bzip2,
}

impl Compression {
    /// `reader`, decompressed. Concatenated streams are read as one.
    pub(super) fn decoder<'r>(self, reader: impl io::Read + 'r) -> Box<dyn io::Read + 'r> {
        match self {
            Self::None => Box::new(reader),
            Self::Gzip => Box::new(MultiGzDecoder::new(reader)),
            Self::Bzip2 => Box::new(MultiBzDecoder::new(reader)),
        }
    }

    /// A tar archive compressed this way, in the words a diagnostic uses.
    fn tar(self) -> &'static str {
        match self {
            Self::None => "a tar archive",
            Self::Gzip => "a gzip-compressed tar archive",
            Self::Bzip2 => "a bzip2-compressed tar archive",
        }
    }
}

/// Why a body declared `decompress` was not decompressed.
#[derive(Debug, Error)]
pub(crate) enum DecompressError {
    /// A body that is not a compressed file, named where it is recognizable.
    #[error("is {saw}, and `decompress` reads a file compressed with gzip or bzip2")]
    NotCompressed { saw: &'static str },

    /// An archive, which `fetch-archive` unpacks rather than `decompress` reading it whole.
    #[error("is {saw}, which `fetch-archive` unpacks")]
    Archive { saw: &'static str },

    /// A compressed stream that could not be read to its end.
    #[error("could not be decompressed: {source}")]
    Unreadable { source: io::Error },
}

/// How the body beginning with `head` is compressed, or why it is not a compressed file.
pub(super) fn compression_of(head: &[u8]) -> Result<Compression, DecompressError> {
    match detect::identify(head) {
        Ok(Format::Tar(Compression::None)) => Err(DecompressError::Archive {
            saw: Compression::None.tar(),
        }),
        // Whether a compressed body holds a tar is known only once it is decompressed.
        Ok(Format::Tar(compression)) => Ok(compression),
        Ok(Format::Zip) => Err(DecompressError::Archive {
            saw: "a zip archive",
        }),
        Err(_) => Err(DecompressError::NotCompressed {
            saw: detect::looks_like(head).unwrap_or("not compressed"),
        }),
    }
}

/// Refuse a decompressed body whose first block, `head`, is a tar header.
pub(super) fn not_a_tar(head: &[u8], compression: Compression) -> Result<(), DecompressError> {
    if detect::is_tar_header(head) {
        Err(DecompressError::Archive {
            saw: compression.tar(),
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_gzip_or_bzip2_body_is_a_compressed_file() {
        assert!(matches!(
            compression_of(&[0x1f, 0x8b, 8]),
            Ok(Compression::Gzip)
        ));
        assert!(matches!(
            compression_of(b"BZh91AY&SY"),
            Ok(Compression::Bzip2)
        ));
        for (body, said) in [
            (&b"#!/bin/sh\n"[..], "is not compressed,"),
            (b"\xfd7zXZ\x00", "is an xz archive,"),
            (
                b"PK\x03\x04",
                "is a zip archive, which `fetch-archive` unpacks",
            ),
        ] {
            let error = compression_of(body).expect_err("not a compressed file");
            assert!(error.to_string().contains(said), "{error}");
        }
    }
}
