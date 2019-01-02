
mod header;


#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}

pub use self::header::{decode_tar, Error, RawAsyncEntry, RawEntryStream};