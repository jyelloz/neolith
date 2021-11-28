use encoding::{Encoding, EncoderTrap, DecoderTrap};

use thiserror::Error;

use tokio::sync::{mpsc, oneshot, watch};

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
pub struct News<E> {
    encoding: E,
    articles: Vec<String>,
}

impl <E> std::fmt::Debug for News<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { articles, .. } = self;
        f.debug_struct("News")
            .field("articles", articles)
            .finish()
    }
}

impl <E: Encoding> News<E> {
    pub fn new(encoding: E) -> Self {
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
        let news = articles.iter()
            .map(String::as_str)
            .rev()
            .collect::<Vec<&str>>()
            .join(SEPARATOR);
        self.encode(&news)
    }
    fn decode(&self, s: &[u8]) -> String {
        match self.encoding.decode(s, DecoderTrap::Ignore) {
            Ok(s) => s,
            Err(cow) => cow.to_string(),
        }
    }
    fn encode(&self, s: &str) -> Vec<u8> {
        match self.encoding.encode(s, EncoderTrap::Ignore) {
            Ok(s) => s,
            Err(cow) => cow.as_bytes().to_vec(),
        }
    }
}

struct Command {
    article: Vec<u8>,
    tx: oneshot::Sender<()>,
}

#[derive(Debug, Clone)]
pub struct NewsService(mpsc::Sender<Command>);

impl NewsService {
    pub fn new <E: Encoding + Clone>(encoding: E) -> (Self, NewsUpdateProcessor<E>) {
        let (tx, rx) = mpsc::channel(10);
        let service = Self(tx);
        let process = NewsUpdateProcessor::new(rx, encoding);
        (service, process)
    }
    pub async fn post(&mut self, article: Vec<u8>) {
        let (tx, rx) = oneshot::channel();
        let command = Command {
            article,
            tx,
        };
        self.0.send(command)
            .await
            .ok();
        rx.await.ok();
    }
}

pub struct NewsUpdateProcessor<E> {
    queue: mpsc::Receiver<Command>,
    news: News<E>,
    updates: watch::Sender<News<E>>,
}

impl <E: Encoding + Clone> NewsUpdateProcessor<E> {
    fn new(queue: mpsc::Receiver<Command>, encoding: E) -> Self {
        let news = News::new(encoding);
        let (updates, _) = watch::channel(news.clone());
        Self { queue, news, updates }
    }
    pub async fn run(self) -> Result<()> {
        let Self { mut queue, mut news, updates: notifications } = self;
        while let Some(command) = queue.recv().await {
            let Command { article, tx } = command;
            news.post(article);
            tx.send(()).ok();
            notifications.send(news.clone()).ok();
        }
        Ok(())
    }
    pub fn subscribe(&self) -> watch::Receiver<News<E>> {
        self.updates.subscribe()
    }
}
