pub mod decode;
pub mod encode;

mod error;

pub use self::error::Error;

pub struct Config {
    unpack_xattrs: bool,
    preserve_permissions: bool,
    preserve_mtime: bool,
    ignore_zeros: bool,
}
