use futures::prelude::*;
use futures::stream::Stream;
use tar_async::decode::{flat, full, raw};
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
        full::decode_tar(input.map(|b| b.freeze()))
            .for_each(|item| {
                if item.header().path().unwrap().starts_with("test/bar") {
                    eprintln!("chunked item={:?}", item.header());
                    futures::future::Either::A(
                        item.for_each(|chunk| Ok(println!("chunk={:?}", chunk))),
                    )
                } else {
                    futures::future::Either::B(
                        Ok(eprintln!("item={:?}", item.header())).into_future(),
                    )
                }
            })
            .map_err(|e| eprintln!("ERROR: {}", e))
            .and_then(|_| Ok(()))
    }));

    pool.shutdown_on_idle()
        .wait()
        .map_err(|_| "failed to shutdown the thread pool")?;
    Ok(())
}
