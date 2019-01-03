pub mod archive;
mod error;
mod header;
mod pax;
pub mod raw;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}

pub use self::error::Error;
pub use self::header::{decode_tar, RawAsyncEntry, RawEntryStream};

pub struct Config {
    unpack_xattrs: bool,
    preserve_permissions: bool,
    preserve_mtime: bool,
    ignore_zeros: bool,
}
