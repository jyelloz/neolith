use num_enum::TryFromPrimitive;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, TryFromPrimitive, Copy, Clone)]
#[repr(i16)]
pub enum TransactionField {
    Nickname = 102,
    Username = 105,
    Password = 106,
    Version = 160,
}
