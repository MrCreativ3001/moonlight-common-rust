use pem::Pem;
use thiserror::Error;

pub trait RequestError {
    /// The machine cannot be reached: timeout, connection refused
    fn is_connect(&self) -> bool;
    /// The sunshine encryption is invalid (e.g. the host removed our client -> we're unpaired)
    fn is_encryption(&self) -> bool;
}

pub trait RequestClient: Sized {
    type Error: RequestError;

    type Text: AsRef<str>;
    type Bytes: AsRef<[u8]>;

    fn with_defaults() -> Result<Self, Self::Error>;
    fn with_defaults_long_timeout() -> Result<Self, Self::Error>;

    fn with_certificates(
        client_private_key: &Pem,
        client_certificate: &Pem,
        server_certificate: &Pem,
    ) -> Result<Self, Self::Error>;

    fn send_http_request_text_response(
        &mut self,
        hostport: &str,
        path: &str,
        query_params: &QueryParamsRef,
    ) -> impl std::future::Future<Output = Result<Self::Text, Self::Error>> + Send;

    fn send_https_request_text_response(
        &mut self,
        hostport: &str,
        path: &str,
        query_params: &QueryParamsRef,
    ) -> impl std::future::Future<Output = Result<Self::Text, Self::Error>> + Send;

    fn send_https_request_data_response(
        &mut self,
        hostport: &str,
        path: &str,
        query_params: &QueryParamsRef,
    ) -> impl std::future::Future<Output = Result<Self::Bytes, Self::Error>> + Send;
}
