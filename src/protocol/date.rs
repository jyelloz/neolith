use super::ProtocolError;
use deku::prelude::*;

use std::time::SystemTime;

use time::{
    Date,
    Duration,
    OffsetDateTime,
    UtcOffset,
    ext::NumericalStdDuration as _,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct DateParameter {
    pub year: i16,
    pub milliseconds: i16,
    pub seconds: i32,
}

impl DateParameter {
    fn try_from(time: SystemTime) -> Result<Self, ProtocolError> {
        let diff = time.duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(0.std_seconds());
        let diff = Duration::ZERO + diff;
        let chronodate = OffsetDateTime::UNIX_EPOCH.checked_add(diff)
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
