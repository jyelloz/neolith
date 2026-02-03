use std::{pin::Pin, sync::Arc, time::Duration};

use encoding_rs::MACINTOSH;
use futures::{FutureExt, StreamExt, lock::Mutex, never::Never, stream::FuturesUnordered};
use neolith::{
    protocol::{self as proto},
    server::{
        Article, ClientRequest, ClientRequestTransaction, ServerRequest,
        session::{self, Session, SessionHandle, SessionSink, SessionStream},
    },
};
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

#[derive(Debug, Copy, Clone)]
struct Transfer(proto::ReferenceNumber);

#[derive(Clone, Default)]
struct Server {
    conns: Arc<Mutex<Vec<SessionHandle>>>,
    news: Arc<Mutex<Vec<proto::Message>>>,
    transfers: Arc<Mutex<Vec<Transfer>>>,
}

impl Server {
    async fn broadcast(&self, tx: ServerRequest) {
        let mut conns = self.conns.lock().await;
        for conn in conns.iter_mut() {
            conn.notify(tx.clone()).await;
        }
    }
}

impl session::Server for Server {
    async fn send_chat(&self, msg: proto::ChatMessage) {
        let tx = ServerRequest::Chat(msg);
        self.broadcast(tx).await;
    }
    async fn post_news(&self, msg: proto::PostNews) {
        let mut news = self.news.lock().await;
        news.push(msg.0.clone());
        let tx = ServerRequest::News(Article(msg.0.into()));
        self.broadcast(tx).await;
    }
    async fn get_news(&self) -> Vec<proto::Message> {
        let news = self.news.lock().await;
        news.clone()
    }
    async fn download_file(&self, _: proto::DownloadFile) -> proto::DownloadFileReply {
        let mut transfers = self.transfers.lock().await;
        let reference = proto::ReferenceNumber::from(1);
        transfers.push(Transfer(reference));
        proto::DownloadFileReply {
            transfer_size: 1.into(),
            file_size: 1.into(),
            waiting_count: Some(1.into()),
            reference,
        }
    }

    async fn get_file(
        &self,
        path: proto::FilePath,
        name: proto::FileName,
    ) -> proto::FlattenedFileObject {
        todo!()
    }
}

fn new_session(
    server: Server,
    id: u32,
) -> Session<Pin<Box<dyn SessionStream>>, Pin<Box<dyn SessionSink<Error = Never>>>, Server> {
    let r = futures::stream::iter([
        proto::GetMessages.into(),
        proto::GetUserNameList.into(),
        proto::GetFileNameList(proto::FilePath::Root).into(),
        proto::DownloadFile {
            file_path: proto::FilePath::Root,
            filename: b"README".to_vec().into(),
        }
        .into(),
        proto::PostNews(format!("#{} was here", id).into_bytes().into()).into(),
        proto::SendChat {
            options: Default::default(),
            chat_id: None,
            message: b"Chat message!".to_vec().into(),
        }
        .into(),
    ]);
    let r = Box::pin(r.enumerate());
    let r = Box::pin(r.map(
        |(id, body): (usize, ClientRequest)| ClientRequestTransaction {
            id: (id as u32).into(),
            body,
        },
    ));
    let w = futures::sink::drain();
    Session::new(id, server, Box::pin(r), Box::pin(w))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .try_init()?;

    let server = Server::default();

    let a = new_session(server.clone(), 123);
    let b = new_session(server.clone(), 456);

    *server.conns.lock().await = vec![a.handle(), b.handle()];

    let events = {
        let mut ah = a.handle();
        let mut bh = b.handle();
        async move {
            for i in 0..10 {
                let f = format!("asdasdas {i}");
                let (f, _, _) = MACINTOSH.encode(&f);
                let f = f.to_vec();
                tokio::time::sleep(Duration::from_secs(1)).await;
                ah.notify(ServerRequest::Broadcast(f.clone().into())).await;
                tokio::time::sleep(Duration::from_secs(1)).await;
                bh.notify(ServerRequest::Broadcast(f.clone().into())).await;
            }
            server.conns.lock().await.clear();
        }
    };

    let futs = [
        a.run().boxed_local(),
        async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            b.run().await;
        }
        .boxed_local(),
        events.boxed_local(),
    ];
    let mut futs = futs.into_iter().collect::<FuturesUnordered<_>>();

    while let Some(_) = futs.next().await {}

    Ok(())
}
