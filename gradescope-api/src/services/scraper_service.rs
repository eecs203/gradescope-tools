use std::time::Duration;

use anyhow::Result;
use reqwest::redirect::Policy;
use reqwest::{Client as HttpClient, RequestBuilder, Response};
use tower::{Service, ServiceBuilder};

use crate::util::{BASE_DOMAIN, BASE_URL};

/// Specialized HTTP client for the app to interact with the internet close to how a human would.
/// Responsible for rate limiting and executing requests, but not anything at a higher level,
/// including authentication and abstracting specific requests for resources.
pub async fn service() -> Result<impl ScraperService> {
    Ok(ServiceBuilder::new()
        .concurrency_limit(1)
        .rate_limit(1, Duration::from_secs(1))
        .map_err(|err: reqwest::Error| err.into())
        .service_fn(|request_builder: RequestBuilder| request_builder.send()))
}

pub trait ScraperService:
    Service<RequestBuilder, Response = Response, Error = anyhow::Error>
{
}
impl<T: Service<RequestBuilder, Response = Response, Error = anyhow::Error>> ScraperService for T {}

pub(super) async fn http_client() -> Result<HttpClient> {
    let redirect_policy = Policy::custom(|attempt| {
        if attempt.url().domain() == Some(BASE_DOMAIN) {
            Policy::none().redirect(attempt)
        } else {
            Policy::default().redirect(attempt)
        }
    });

    let client = HttpClient::builder()
        .cookie_store(true)
        .redirect(redirect_policy)
        .timeout(Duration::from_secs(30))
        .build()?;

    // init cookies
    client.get(BASE_URL).send().await?;

    Ok(client)
}
