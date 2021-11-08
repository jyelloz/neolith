pub mod files;
pub mod users;

#[derive(Debug)]
pub struct Chat(pub User, pub Vec<u8>);

impl Into<ChatMessage> for Chat {
    fn into(self) -> ChatMessage {
        let Self(user, text) = self;
        let User(username) = user;
        let message = [
            &b"\r "[..],
            &username[..],
            &b": "[..],
            &text[..],
        ].concat();
        ChatMessage {
            chat_id: None,
            message,
        }
    }
}

#[derive(Debug)]
pub struct User(pub Vec<u8>);
