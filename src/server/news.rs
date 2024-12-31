use encoding_rs::Encoding;

use thiserror::Error;

use tokio::sync::{mpsc, oneshot, watch};

use super::bus::{Bus, Notification};

pub static SEPARATOR: &str = "\r--\r";

#[derive(Debug, Error)]
pub enum NewsError {
    #[error("execution error")]
    ExecutionError(#[from] oneshot::error::RecvError),
    #[error("service unavailable")]
    ServiceUnavailable,
}

type Result<T> = ::core::result::Result<T, NewsError>;

#[derive(Clone)]
pub struct News {
    encoding: &'static Encoding,
    articles: Vec<String>,
}

impl std::fmt::Debug for News {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { articles, .. } = self;
        f.debug_struct("News").field("articles", articles).finish()
    }
}

impl News {
    pub fn new(encoding: &'static Encoding) -> Self {
        Self {
            encoding,
            articles: vec![],
        }
    }
    pub fn post(&mut self, article: Vec<u8>) {
        let article = self.decode(&article);
        let Self { articles, .. } = self;
        articles.push(article);
    }
    pub fn all(&self) -> Vec<u8> {
        let Self { articles, .. } = self;
        let news = articles
            .iter()
            .map(String::as_str)
            .rev()
            .collect::<Vec<&str>>()
            .join(SEPARATOR);
        self.encode(&news)
    }
    fn decode(&self, s: &[u8]) -> String {
        self.encoding.decode(s).0.to_string()
    }
    fn encode(&self, s: &str) -> Vec<u8> {
        self.encoding.encode(s).0.to_vec()
    }
}

struct Command {
    article: Vec<u8>,
    tx: oneshot::Sender<()>,
}

#[derive(Debug, Clone)]
pub struct NewsService(mpsc::Sender<Command>, Bus);

impl NewsService {
    pub fn new(encoding: &'static Encoding, bus: Bus) -> (Self, NewsUpdateProcessor) {
        let (tx, rx) = mpsc::channel(10);
        let service = Self(tx, bus);
        let process = NewsUpdateProcessor::new(rx, encoding);
        (service, process)
    }
    pub async fn post(&mut self, article: Vec<u8>) {
        let (tx, rx) = oneshot::channel();
        let notification = Notification::News(article.clone().into());
        let command = Command { article, tx };
        let Self(tx, bus) = self;
        tx.send(command).await.ok();
        rx.await.ok();
        bus.publish(notification);
    }
}

pub struct NewsUpdateProcessor {
    queue: mpsc::Receiver<Command>,
    news: News,
    updates: watch::Sender<News>,
}

impl NewsUpdateProcessor {
    fn new(queue: mpsc::Receiver<Command>, encoding: &'static Encoding) -> Self {
        let news = News::new(encoding);
        let (updates, _) = watch::channel(news.clone());
        Self {
            queue,
            news,
            updates,
        }
    }
    #[tracing::instrument(name = "NewsUpdateProcessor", skip(self))]
    pub async fn run(self) -> Result<()> {
        let Self {
            mut queue,
            mut news,
            updates: notifications,
        } = self;
        while let Some(command) = queue.recv().await {
            let Command { article, tx } = command;
            news.post(article);
            tx.send(()).ok();
            notifications.send(news.clone()).ok();
        }
        Ok(())
    }
    pub fn subscribe(&self) -> watch::Receiver<News> {
        self.updates.subscribe()
    }
}
