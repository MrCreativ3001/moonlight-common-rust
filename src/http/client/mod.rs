use std::time::Duration;

use crate::http::ParseError;

pub mod async_client;
pub mod blocking_client;

#[cfg(feature = "reqwest")]
pub mod reqwest;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
pub const DEFAULT_LONG_TIMEOUT: Duration = Duration::from_secs(90);

pub trait RequestError: TryInto<ParseError, Error = Self> {
    /// The machine cannot be reached: timeout, connection refused
    fn is_connect(&self) -> bool;
    /// The sunshine encryption is invalid (e.g. the host removed our client -> we're unpaired)
    fn is_encryption(&self) -> bool;
}

mod url {
    use url::Url;

    use crate::http::{QueryBuilder, QueryBuilderError, QueryParam};

    impl QueryBuilder for Url {
        fn append(&mut self, param: QueryParam) -> Result<(), QueryBuilderError> {
            self.query_pairs_mut().append_pair(param.key, param.value);

            Ok(())
        }
    }
}
