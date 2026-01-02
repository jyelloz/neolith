use clap::{Parser, ValueEnum};
use std::io::stdin;

use deku::{noseek::NoSeek, DekuContainerRead as _};
use neolith::{protocol as proto, server::transaction_stream::Frames};

#[derive(ValueEnum, Copy, Clone, Debug, PartialEq, Eq)]
enum Mode {
    Server,
    Client,
}

#[derive(Parser, Debug)]
#[command(name = "nlprotoparse")]
struct Command {
    mode: Mode,
}

fn main() -> anyhow::Result<()> {
    let args = Command::parse();
    let stdin = stdin().lock();
    let mut stdin = NoSeek::new(stdin);
    match args.mode {
        Mode::Server => {
            let (_, hs) = proto::ServerHandshakeReply::from_reader((&mut stdin, 0))?;
            eprintln!("server to client mode {hs:?}");
        }
        Mode::Client => {
            let (_, hs) = proto::ClientHandshakeRequest::from_reader((&mut stdin, 0))?;
            eprintln!("client to server mode {hs:?}");
        }
    }
    let frames = Frames::new(stdin);
    for f in frames {
        let f = f?;
        let tt = f.header.transaction_type();
        eprintln!("frame {tt:?} {f:?}");
    }
    Ok(())
}
