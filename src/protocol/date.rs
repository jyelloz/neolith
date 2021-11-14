use super::{
    be_i16,
    be_i32,
    BIResult,
    ProtocolError,
};

use std::time::SystemTime;

use time::{
    Date,
    Duration,
    OffsetDateTime,
    UtcOffset,
    ext::NumericalStdDuration as _,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DateParameter {
    pub year: i16,
    pub milliseconds: i16,
    pub seconds: i32,
}

impl DateParameter {
    fn parse_year(bytes: &[u8]) -> BIResult<i16> {
        be_i16(bytes)
    }
    fn parse_milliseconds(bytes: &[u8]) -> BIResult<i16> {
        be_i16(bytes)
    }
    fn parse_seconds(bytes: &[u8]) -> BIResult<i32> {
        be_i32(bytes)
    }
    pub fn parse(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, year) = Self::parse_year(&bytes)?;
        let (bytes, milliseconds) = Self::parse_milliseconds(&bytes)?;
        let (bytes, seconds) = Self::parse_seconds(&bytes)?;
        Ok((
            bytes,
            Self {
                year,
                milliseconds,
                seconds,
            },
        ))
    }
    pub fn pack(&self) -> Vec<u8> {
        let Self { year, milliseconds, seconds } = self;
        [
            &year.to_be_bytes()[..],
            &milliseconds.to_be_bytes()[..],
            &seconds.to_be_bytes()[..],
        ].into_iter()
            .flat_map(|b| b.into_iter())
            .map(|b| *b)
            .collect()
    }
    fn try_from(time: SystemTime) -> Result<Self, ProtocolError> {
        let diff = time.duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(0.std_seconds());
        let diff = Duration::ZERO + diff;
        let chronodate = OffsetDateTime::UNIX_EPOCH.checked_add(diff.into())
            .ok_or(ProtocolError::SystemError)?;
        let year = chronodate.year();
        let year_start = Date::from_ordinal_date(year, 1)
            .or(Err(ProtocolError::SystemError))?
            .with_hms(0, 0, 0)
            .or(Err(ProtocolError::SystemError))?
            .assume_offset(UtcOffset::UTC);

        let seconds = (chronodate - year_start).whole_seconds() as i32;
        let date = Self {
            year: year as i16,
            seconds,
            milliseconds: 0,
        };
        Ok(date)
    }
}

impl From<SystemTime> for DateParameter {
    fn from(time: SystemTime) -> Self {
        Self::try_from(time).unwrap_or_default()
    }
}
