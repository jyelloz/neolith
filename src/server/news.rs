use encoding::{Encoding, EncoderTrap, DecoderTrap};

pub static SEPARATOR: &'static str = "\r--\r";

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
