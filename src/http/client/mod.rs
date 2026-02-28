use crate::http::{ClientInfo, Endpoint, ParseError, Request};
use crate::http::{QueryBuilder, QueryBuilderError, QueryParam};

use std::time::Duration;
use url::Url;

pub mod async_client;
pub mod blocking_client;

#[cfg(feature = "hyper")]
pub mod hyper;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
pub const DEFAULT_LONG_TIMEOUT: Duration = Duration::from_secs(90);

pub trait RequestError: TryInto<ParseError, Error = Self> {
    /// The machine cannot be reached: timeout, connection refused
    fn is_connect(&self) -> bool;
    /// The sunshine encryption is invalid (e.g. the host removed our client -> we're unpaired)
    fn is_encryption(&self) -> bool;
}

fn build_url<E>(
    use_https: bool,
    client_info: ClientInfo<'_>,
    hostport: &str,
    request: &E::Request,
) -> Result<Url, url::ParseError>
where
    E: Endpoint,
{
    let protocol = if use_https { "https" } else { "http" };
    let authority = format!("{protocol}://{hostport}{}", E::path());
    let mut url = Url::parse(&authority)?;

    client_info
        .append_query_params(&mut url)
        .expect("add query parameter to url");

    request
        .append_query_params(&mut url)
        .expect("add query parameter to url");

    Ok(url)
}

impl QueryBuilder for Url {
    fn append(&mut self, param: QueryParam) -> Result<(), QueryBuilderError> {
        self.query_pairs_mut().append_pair(param.key, param.value);

        Ok(())
    }
}
