//! An archive's format, decided by its leading bytes rather than by its URL.

use super::compressed::Compression;

/// The first bytes of a gzip stream.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// The first bytes of a bzip2 stream, before the digit giving its block size.
const BZIP2_MAGIC: &[u8] = b"BZh";

/// The first bytes of a zip archive: a local file header, or for an empty archive the end
/// of its central directory.
const ZIP_MAGICS: [&[u8]; 2] = [b"PK\x03\x04", b"PK\x05\x06"];

/// Number of bytes in a tar header block, and so the most [`identify`] reads.
pub(super) const HEADER_BYTES: usize = 512;

/// Where a tar header keeps the checksum of the block it is in.
const CHECKSUM_FIELD: std::ops::Range<usize> = 148..156;

/// An archive format batfiles unpacks.
#[derive(Debug, Clone, Copy)]
pub(super) enum Format {
    Tar(Compression),
    Zip,
}

/// The format of an archive beginning with `head`, or the words a diagnostic uses for what
/// it is instead.
pub(super) fn identify(head: &[u8]) -> Result<Format, &'static str> {
    if head.starts_with(&GZIP_MAGIC) {
        Ok(Format::Tar(Compression::Gzip))
    } else if is_bzip2(head) {
        Ok(Format::Tar(Compression::Bzip2))
    } else if ZIP_MAGICS.iter().any(|magic| head.starts_with(magic)) {
        Ok(Format::Zip)
    } else if is_tar_header(head) {
        Ok(Format::Tar(Compression::None))
    } else {
        Err(looks_like(head).unwrap_or("not an archive batfiles recognizes"))
    }
}

/// Whether `head` begins a bzip2 stream: its magic, then a block size from 1 to 9.
fn is_bzip2(head: &[u8]) -> bool {
    head.strip_prefix(BZIP2_MAGIC)
        .and_then(|rest| rest.first())
        .is_some_and(|size| (b'1'..=b'9').contains(size))
}

/// Whether a block is a tar header, by the checksum it carries of itself.
pub(super) fn is_tar_header(head: &[u8]) -> bool {
    let Some(block) = head.get(..HEADER_BYTES) else {
        return false;
    };
    let Some(declared) = octal(&block[CHECKSUM_FIELD]) else {
        return false;
    };
    let (unsigned, signed) =
        block
            .iter()
            .enumerate()
            .fold((0u32, 0i32), |(unsigned, signed), (offset, &byte)| {
                // The checksum field is summed as spaces.
                let byte = if CHECKSUM_FIELD.contains(&offset) {
                    b' '
                } else {
                    byte
                };
                (unsigned + u32::from(byte), signed + i32::from(byte as i8))
            });
    // Accept both signed-byte and unsigned-byte tar checksums.
    declared == unsigned || i64::from(declared) == i64::from(signed)
}

/// Parse a padded octal tar-header field; return `None` if invalid.
fn octal(field: &[u8]) -> Option<u32> {
    let mut value: u32 = 0;
    let mut digits = 0;
    for byte in field.iter().skip_while(|byte| **byte == b' ') {
        if !(b'0'..=b'7').contains(byte) {
            break;
        }
        value = value.checked_mul(8)?.checked_add(u32::from(byte - b'0'))?;
        digits += 1;
    }
    (digits > 0).then_some(value)
}

/// What a run of leading bytes is, in the words a diagnostic uses, where it is recognizable
/// as an archive or compressed stream batfiles does not read.
pub(super) fn looks_like(head: &[u8]) -> Option<&'static str> {
    for (magic, name) in [
        (&b"\xfd7zXZ\x00"[..], "an xz archive"),
        (&b"\x28\xb5\x2f\xfd"[..], "a zstd archive"),
        (&b"7z\xbc\xaf\x27\x1c"[..], "a 7-zip archive"),
    ] {
        if head.starts_with(magic) {
            return Some(name);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leading_bytes_are_named_where_they_are_recognizable() {
        assert_eq!(looks_like(b"\xfd7zXZ\x00rest"), Some("an xz archive"));
        assert_eq!(
            identify(b"<!DOCTYPE html>").err(),
            Some("not an archive batfiles recognizes")
        );
        assert_eq!(looks_like(b""), None);
    }

    #[test]
    fn a_bzip2_stream_is_recognized_by_its_magic_and_block_size() {
        assert!(matches!(
            identify(b"BZh91AY&SY"),
            Ok(Format::Tar(Compression::Bzip2))
        ));
        for refused in [&b"BZh0"[..], b"BZhx", b"BZh", b"BZ"] {
            assert!(!is_bzip2(refused), "{refused:?}");
        }
    }

    /// One header block, checksummed, with `magic` written where a `ustar`
    /// archive carries one and a V7 archive carries nothing.
    fn header(magic: &[u8]) -> Vec<u8> {
        let mut block = vec![0u8; HEADER_BYTES];
        block[..8].copy_from_slice(b"a/b\0\0\0\0\0");
        block[100..108].copy_from_slice(b"000644 \0");
        block[124..136].copy_from_slice(b"00000000002\0");
        block[257..257 + magic.len()].copy_from_slice(magic);
        block[CHECKSUM_FIELD].fill(b' ');
        let sum: u32 = block.iter().map(|&byte| u32::from(byte)).sum();
        let written = format!("{sum:06o}\0 ");
        block[CHECKSUM_FIELD].copy_from_slice(written.as_bytes());
        block
    }

    #[test]
    fn a_tar_is_recognized_by_its_checksum_rather_than_by_its_format() {
        // V7 headers lack the `ustar` magic; their checksum still identifies them as tar.
        for magic in [&b"ustar\0"[..], b"ustar  \0", b""] {
            assert!(is_tar_header(&header(magic)), "{magic:?}");
        }
    }

    #[test]
    fn something_that_is_not_a_tar_header_is_not_taken_for_one() {
        let mut wrong = header(b"ustar\0");
        wrong[0] = b'z';
        assert!(!is_tar_header(&wrong));
        assert!(!is_tar_header(b"ustar"));
        assert!(!is_tar_header(&[0u8; HEADER_BYTES]));
        assert!(!is_tar_header(&[b'x'; HEADER_BYTES]));
    }
}
