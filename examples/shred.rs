use std::io::{Read, Write};
use std::{io, thread, time};
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "basic")]
struct Opt {
    #[structopt(short = "p", long = "pattern")]
    pattern: Vec<usize>,
    #[structopt(short = "s", long = "sleep", default_value = "100")]
    sleep: u64,
}

fn main() -> Result<(), io::Error> {
    let opt = Opt::from_args();

    let stdin = io::stdin();
    let stdout = io::stdout();

    for chunk_size in opt.pattern.into_iter().cycle() {
        let mut buf = Vec::new();
        buf.resize(chunk_size, 0);

        let len = stdin.lock().read(buf.as_mut())?;
        if len == 0 {
            break;
        }
        stdout.lock().write_all(&buf[..len])?;
        stdout.lock().flush()?;
        thread::sleep(time::Duration::from_millis(opt.sleep))
    }
    Ok(())
}
