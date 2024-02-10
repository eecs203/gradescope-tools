use std::time::Duration;

use anyhow::Result;
use reqwest::redirect::Policy;
use reqwest::{Client as HttpClient, RequestBuilder, Response};
use tower::{Service, ServiceBuilder, ServiceExt};

use crate::util::{BASE_DOMAIN, BASE_URL};

/// Specialized HTTP client for interacting with Gradescope. Responsible for rate limiting and
/// executing requests, but not anything at a higher level, including authentication and abstracting
/// specific requests for resources.
pub async fn gs_service(
) -> Result<impl Service<RequestBuilder, Response = Response, Error = anyhow::Error>> {
    let client = gs_client().await?;

    Ok(ServiceBuilder::new()
        .concurrency_limit(1)
        .rate_limit(1, Duration::from_secs(1))
        .map_err(|err: reqwest::Error| err.into())
        .layer_fn(|service| )
        .service_fn(|request: RequestBuilder| async { request.build() })
        .and_then(client))
}

async fn gs_client() -> Result<HttpClient> {
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
