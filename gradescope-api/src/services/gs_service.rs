use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::{Client as HttpClient, Method, RequestBuilder, Response};
use scraper::Html;
use tower::reconnect::Reconnect;
use tower::{service_fn, Service, ServiceBuilder, ServiceExt};

use crate::creds::Creds;
use crate::selectors;
use crate::services::scraper_service;
use crate::util::{gs_url, LOGIN_PATH};

use super::scraper_service::{http_client, ScraperService};

selectors! {
    AUTHENTICITY_TOKEN = "form[action='/login'] input[name=authenticity_token]",
}

/// Specialized HTTP client for interacting with Gradescope in particular. Responsible for
/// authentication.
pub async fn service(creds: Creds) -> Result<impl GsService> {
    let http_client = http_client().await?;

    let authed_service_maker = service_fn(move |_: ()| {
        let (http_client, creds) = (http_client.clone(), creds.clone());
        Box::pin(async move {
            let scraper = scraper_service::service().await?;
            let unauthed = unauthed_service(http_client, scraper);
            let authed = authed_service(creds, unauthed).await?;
            anyhow::Ok(authed)
        })
    });
    Ok(Reconnect::new::<(), ()>(authed_service_maker, ())
        .map_err(|err: Box<dyn std::error::Error + Send + Sync>| anyhow!(err)))
}

pub trait GsService: Service<GsRequest, Response = Response, Error = anyhow::Error> {
    fn as_html_service(&mut self) -> impl HtmlService
    where
        Self: Sized,
    {
        html_service(self)
    }
}
impl<T: Service<GsRequest, Response = Response, Error = anyhow::Error>> GsService for T {}

async fn authed_service(
    creds: Creds,
    mut unauthed: impl UnauthedService,
) -> Result<impl UnauthedService> {
    let auth_token = get_auth_token(&mut unauthed).await?;
    try_login(&mut unauthed, &auth_token, creds).await?;
    Ok(unauthed)
}

async fn try_login(
    unauthed: &mut impl UnauthedService,
    auth_token: &str,
    creds: Creds,
) -> Result<()> {
    let request = login_request(auth_token, creds);
    let response = unauthed.oneshot(request).await?;
    check_login_success(response)
}

fn check_login_success(response: Response) -> Result<()> {
    if response.status().is_redirection() {
        Ok(())
    } else {
        bail!("authentication failed")
    }
}

fn login_request(auth_token: &str, creds: Creds) -> GsRequest {
    let login_data = {
        let mut login_data = HashMap::new();
        login_data.insert("utf8", "✓");
        login_data.insert("session[email]", creds.email());
        login_data.insert("session[password]", creds.password());
        login_data.insert("session[remember_me]", "0");
        login_data.insert("commit", "Log In");
        login_data.insert("session[remember_me_sso]", "0");
        login_data.insert("authenticity_token", auth_token);
        login_data
    };
    GsRequest::new_direct(Method::POST, LOGIN_PATH.to_owned())
        .with_form_data(serde_json::to_value(login_data).unwrap())
}

async fn get_auth_token(unauthed: &mut impl UnauthedService) -> Result<String> {
    let request = HtmlRequest::new(LOGIN_PATH.to_owned());
    let login_page = html_service(unauthed).oneshot(request).await?;

    login_page
        .select(&AUTHENTICITY_TOKEN)
        .next()
        .and_then(|el| el.value().attr("value"))
        .context("could not find `authenticity_token`")
        .map(|token| token.to_owned())
}

fn html_service(unauthed: impl UnauthedService) -> impl HtmlService {
    ServiceBuilder::new()
        .map_request(HtmlRequest::gs_request)
        .service(unauthed)
        .and_then(|response: Response| async {
            let text = response.text().await.context("could not get HTML")?;
            Ok(Html::parse_document(&text))
        })
}

trait HtmlService: Service<HtmlRequest, Response = Html, Error = anyhow::Error> {}
impl<T: Service<HtmlRequest, Response = Html, Error = anyhow::Error>> HtmlService for T {}

#[derive(Debug)]
pub struct HtmlRequest {
    path: String,
}

impl HtmlRequest {
    pub fn new(path: String) -> Self {
        Self { path }
    }

    pub fn gs_request(self) -> GsRequest {
        GsRequest::new_html(self.path)
    }
}

impl From<String> for HtmlRequest {
    fn from(path: String) -> Self {
        Self { path }
    }
}

impl<'a> From<&'a str> for HtmlRequest {
    fn from(path: &'a str) -> Self {
        path.to_owned().into()
    }
}

fn unauthed_service<'a>(
    http_client: HttpClient,
    scraper: impl ScraperService + 'a,
) -> impl UnauthedService + 'a {
    ServiceBuilder::new()
        .map_request(move |gs_request: GsRequest| gs_request.request_builder(&http_client))
        .service(scraper)
}

trait UnauthedService: Service<GsRequest, Response = Response, Error = anyhow::Error> {}
impl<T: Service<GsRequest, Response = Response, Error = anyhow::Error>> UnauthedService for T {}

pub struct GsRequest {
    method: Method,
    path: String,
    headers: HashMap<String, String>,
    form: Option<serde_json::Value>,
    timeout: Option<Duration>,
}

impl GsRequest {
    pub fn new_direct(method: Method, path: String) -> Self {
        Self {
            method,
            path,
            headers: HashMap::new(),
            form: None,
            timeout: None,
        }
    }

    pub fn new_html(path: String) -> Self {
        let mut headers = HashMap::new();
        headers.insert("Accept".to_owned(), "text/html".to_owned());

        Self {
            method: Method::GET,
            path,
            headers,
            form: None,
            timeout: None,
        }
    }

    pub fn new_ajax(method: Method, path: String, csrf_token: String) -> Self {
        let mut headers = HashMap::new();
        headers.insert("X-Requested-With".to_owned(), "XMLHttpRequest".to_owned());
        headers.insert("X-CSRF-Token".to_owned(), csrf_token);

        Self {
            method,
            path,
            headers,
            form: None,
            timeout: None,
        }
    }

    pub fn with_form_data(mut self, data: serde_json::Value) -> Self {
        self.form = Some(data);
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn request_builder(&self, http_client: &HttpClient) -> RequestBuilder {
        let url = gs_url(&self.path);
        let base = http_client.request(self.method.clone(), url);

        let with_headers = self
            .headers
            .iter()
            .fold(base, |request_builder, (key, value)| {
                request_builder.header(key, value)
            });

        let with_form = if let Some(data) = &self.form {
            with_headers.form(data)
        } else {
            with_headers
        };

        if let Some(timeout) = &self.timeout {
            with_form.timeout(*timeout)
        } else {
            with_form
        }
    }
}
