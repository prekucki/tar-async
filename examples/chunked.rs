use futures::prelude::*;
use futures::stream::Stream;
use tar_async::{archive, raw};
use tokio_codec::{BytesCodec, FramedRead};
use tokio_fs::{stderr, stdin, stdout};
use tokio_threadpool::Builder;

pub fn main() -> Result<(), Box<std::error::Error>> {
    let pool = Builder::new().pool_size(1).build();

    pool.spawn(futures::lazy(|| {
        let input = FramedRead::new(stdin(), BytesCodec::new());

        /*
                decode_tar(input.map(|b| b.freeze()))
                    .for_each(|item| {
                        eprintln!("item={:?}", item.header());
                        item.for_each(|chunk| Ok(eprintln!("chunk={:?}", chunk)))
                    })
                    .map_err(|e| eprintln!("ERROR: {}", e))
                    .and_then(|_| Ok(()))
        */
        archive::decode_tar(input.map(|b| b.freeze()))
            .for_each(|item| Ok(eprintln!("item={:?}", item)))
            .map_err(|e| eprintln!("ERROR: {}", e))
            .and_then(|_| Ok(()))
    }));

    pool.shutdown_on_idle()
        .wait()
        .map_err(|_| "failed to shutdown the thread pool")?;
    Ok(())
}
