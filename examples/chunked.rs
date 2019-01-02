
use futures::prelude::*;
use tokio_fs::{stdin, stdout, stderr};
use tokio_threadpool::Builder;
use tokio_codec::{FramedRead, BytesCodec};
use futures::stream::Stream;
use tar_async::decode_tar;

pub fn main() -> Result<(), Box<std::error::Error>> {
    let pool = Builder::new()
        .pool_size(1)
        .build();

    pool.spawn({
        let input = FramedRead::new(stdin(), BytesCodec::new());

        decode_tar(input.map(|b| b.freeze())).for_each(|item| {
            eprintln!("item={:?}", item.header());
            item.for_each(|chunk| {
                Ok(eprintln!("chunk={:?}", chunk))
            })
        }).map_err(|e| eprintln!("ERROR: {}", e)).and_then(|_| Ok(()))
    });

    pool.shutdown_on_idle().wait().map_err(|_| "failed to shutdown the thread pool")?;
    Ok(())
}