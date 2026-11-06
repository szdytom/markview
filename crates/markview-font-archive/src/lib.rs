//! Font archive recognition, extraction and ownership.
mod archive;

pub use archive::{
	Container, Extracted, MAX_FILE_BYTES, MAX_MEMBERS, MAX_UNPACKED_BYTES,
	glob_match, sha256_file, sha256_hex, sniff, sniff_file, temp_path, unpack,
};
