use num_enum::{
    TryFromPrimitive,
    IntoPrimitive,
};

#[derive(
    Debug,
    Copy, Clone,
    Eq, PartialEq,
    Ord, PartialOrd,
    TryFromPrimitive, IntoPrimitive,
)]
#[repr(i16)]
pub enum TransactionField {
    ErrorText = 100,
    Data,
    UserName,
    UserId,
    UserIconId,
    UserLogin,
    UserPassword,
    ReferenceNumber,
    TransferSize,
    ChatOptions,
    UserAccess,
    UserAlias,
    UserFlags,
    Options,
    ChatId,
    ChatSubject,
    WaitingCount,

    ServerAgreement = 150,
    ServerBanner,
    ServerBannerType,
    ServerBannerUrl,
    NoServerAgreement,

    Version = 160,
    CommunityBannerId,
    ServerName,

    FileNameWithInfo = 200,
    FileName,
    FilePath,
    FileResumeData,
    FileTransferOptions,
    FileTypeString,
    FileCreatorString,
    FileSize,
    FileCreateDate,
    FileModifyDate,
    FileComment,
    FileNewName,
    FileNewPath,
    FileType,
    QuotingMessage,
    AutomaticResponse,

    FolderItemCount = 220,

    UserNameWithInfo = 300,

    NewsCategoryGuid = 319,
    NewsCategoryListData,
    NewsArticleListData,
    NewsCategoryName,
    NewsCategoryListDataV1_5,

    NewsPath = 325,
    NewsArticleId,
    NewsArticleDataFlavor,
    NewsArticleTitle,
    NewsArticlePoster,
    NewsArticleDate,
    NewsArticlePreviousArticle,
    NewsArticleNextArticle,
    NewsArticleData,
    NewsArticleFlags,
    NewsArticleParentArticle,
    NewsArticleFirstChildArticle,
    NewsArticleRecursiveDelete,

}
