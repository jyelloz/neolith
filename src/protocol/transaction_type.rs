use num_enum::{
    IntoPrimitive,
    TryFromPrimitive,
};

#[derive(
    Debug, Eq, PartialEq, Ord, PartialOrd, IntoPrimitive, TryFromPrimitive
)]
#[repr(i16)]
pub enum TransactionType {

    Reply = 0,

    Error = 100,

    GetMessages,
    NewMessage,
    OldPostNews,
    ServerMessage,
    SendChat,
    ChatMessage,
    Login,
    SendInstantMessage,
    ShowAgreement,
    DisconnectUser,
    DisconnectMessage,
    InviteToNewChat,
    InviteToChat,
    RejectChatInvite,
    JoinChat,
    LeaveChat,
    NotifyChatUserChange,
    NotifyChatUserDelete,
    NotifyChatSubject,
    SetChatSubject,
    Agreed,
    ServerBanner,

    GetFileNameList = 200,

    DownloadFile = 202,
    UploadFile,
    DeleteFile,
    NewFolder,
    GetFileInfo,
    SetFileInfo,
    MoveFile,
    MakeFileAlias,
    DownloadFolder,
    DownloadBanner,
    UploadFolder,

    GetUserNameList = 300,
    NotifyUserChange,
    NotifyUserDelete,
    GetClientInfoText,
    SetClientUserInfo,

    NewUser = 350,
    DeleteUser,
    GetUser,
    SetUser,
    UserAccess,
    UserBroadcast,

    GetNewsCategoryNameList = 370,
    GetNewsArticleNameList,

    DeleteNewsItem = 380,
    NewNewsFolder,
    NewNewsCategory,

    GetNewsArticleData = 400,

    PostNewsArticle = 410,
    DeleteNewsArticle,

    ConnectionKeepAlive = 500,

}
