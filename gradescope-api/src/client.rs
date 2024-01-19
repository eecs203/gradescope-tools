use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use futures::Stream;
use itertools::{Either, Itertools};
use lazy_static::lazy_static;
use reqwest::redirect::Policy;
use reqwest::{Client as HttpClient, Method, RequestBuilder, Response};
use scraper::{CaseSensitivity, Element, ElementRef, Html};
use serde::Deserialize;
use tokio::sync::MutexGuard;
use tokio::time::sleep;
use tracing::{debug, info, warn};
use url::Url;

use crate::assignment::{Assignment, AssignmentsTableProps};
use crate::course::{Course, CourseId, Role};
use crate::creds::Creds;
use crate::export_submissions::{download_submission_export, files_as_submissions, read_zip};
use crate::rate_limit::RateLimited;
use crate::regrade::Regrade;
use crate::submission::{SubmissionId, SubmissionsManagerProps};
use crate::submission_export::pdf::SubmissionPdf;
use crate::submission_export::{submissions_export_from_response, SubmissionExport};
use crate::types::{GraderName, QuestionTitle, StudentName};
use crate::util::*;

macro_rules! selectors {
    ($name:ident = $x:expr $(,)?) => {
        lazy_static! { static ref $name: scraper::Selector = scraper::Selector::parse($x).unwrap(); }
    };

    ($name:ident = $x:expr, $($names:ident = $xs:expr),+ $(,)?) => {
        selectors! { $name = $x }
        selectors! {
            $($names = $xs),+
        }
    };
}

selectors! {
    AUTHENTICITY_TOKEN = "form[action='/login'] input[name=authenticity_token]",
    COURSE_LIST_HEADING = ".pageHeading",
    COURSE = ".courseBox",
    COURSE_SHORT_NAME = ".courseBox--shortname",
    COURSE_NAME = ".courseBox--name",
    ASSIGNMENT_ROW = "tr.js-assignmentTableAssignmentRow",
    TD = "td",
    A = "a",
    REGRADE_ROW = "table.js-regradeRequestsTable > tbody > tr",
    BULK_EXPORT_A = ".js-bulkExportModalDownload",
    SUBMISSIONS_MANAGER = "#main-content > [data-react-class=SubmissionsManager]",
    ASSIGNMENTS_TABLE = "[data-react-class=AssignmentsTable]",
    CSRF_TOKEN_META = "meta[name='csrf-token']",
}

#[derive(Debug)]
pub struct Client<State: ClientState> {
    client: RateLimited<HttpClient>,
    creds: Creds,
    _state: State,
}

impl<State: ClientState> Client<State> {
    async fn http_client(&self) -> MutexGuard<HttpClient> {
        self.client.get().await
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn gs(&self, method: Method, path: &str) -> RequestBuilder {
        let url = gs_url(path);
        info!(%url, %method, "preparing GS request");

        self.http_client().await.request(method, url)
    }

    #[tracing::instrument(level = "debug", skip(self, csrf_token))]
    async fn gs_ajax(&self, method: Method, path: &str, csrf_token: &str) -> RequestBuilder {
        self.gs(method, path)
            .await
            .header("X-Requested-With", "XMLHttpRequest")
            .header("X-CSRF-Token", csrf_token)
    }

    async fn send(&self, request: RequestBuilder) -> Result<Response> {
        let response = request
            .send()
            .await
            .context("GS request failed")?
            .error_for_status()
            .context("GS responded with an error")?;
        Ok(response)
    }

    async fn get_gs_html(&self, path: &str) -> Result<Html> {
        let request = self
            .gs(Method::GET, path)
            .await
            .header("Accept", "text/html");
        let response = self.send(request).await?;
        let text = response.text().await?;
        Ok(Html::parse_document(&text))
    }
}

impl Client<Init> {
    pub async fn from_env() -> Result<Self> {
        let creds = Creds::from_env()?;
        Client::new(creds).await
    }

    pub async fn new(creds: Creds) -> Result<Self> {
        let redirect_policy = Policy::custom(|attempt| {
            if attempt.url().domain() == Some("www.gradescope.com") {
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

        Ok(Self {
            client: RateLimited::new(client, Duration::from_secs(1)),
            creds,
            _state: Init,
        })
    }

    pub async fn login(self) -> Result<Client<Auth>> {
        let authenticity_token = self.get_authenticity_token().await?;

        let login_data = {
            let mut login_data = HashMap::new();
            login_data.insert("utf8", "✓");
            login_data.insert("session[email]", self.creds.email());
            login_data.insert("session[password]", self.creds.password());
            login_data.insert("session[remember_me]", "0");
            login_data.insert("commit", "Log In");
            login_data.insert("session[remember_me_sso]", "0");
            login_data.insert("authenticity_token", &authenticity_token);
            login_data
        };

        let request = self.gs(Method::POST, LOGIN_PATH).await.form(&login_data);
        let response = self.send(request).await?;

        if response.status().is_redirection() {
            Ok(Client {
                client: self.client,
                creds: self.creds,
                _state: Auth,
            })
        } else {
            bail!("authentication failed")
        }
    }

    async fn get_authenticity_token(&self) -> Result<String> {
        self.get_gs_html(LOGIN_PATH)
            .await?
            .select(&AUTHENTICITY_TOKEN)
            .next()
            .and_then(|el| el.value().attr("value"))
            .context("could not find `authenticity_token`")
            .map(|token| token.to_owned())
    }
}

impl Client<Auth> {
    pub async fn get_courses(&self) -> Result<Vec<Course>> {
        let account_page = self.get_gs_html(ACCOUNT_PATH).await?;
        let course_list_headings = account_page
            .select(&COURSE_LIST_HEADING)
            .filter_map(|el| {
                el.next_siblings()
                    .filter_map(ElementRef::wrap)
                    .find(|sib| {
                        sib.has_class(&("courseList".into()), CaseSensitivity::CaseSensitive)
                    })
                    .map(|list| (text(el), list))
            })
            .collect::<HashMap<_, _>>();

        let heading_account_types = [
            // Accounts that are students in some class(es) and instructors in some class(es) have
            // headings to differentiate classes by role
            ("Instructor Courses", Role::Instructor),
            ("Student Courses", Role::Student),
            // Accounts with only one role don't need to differentiate. We assume users are
            // instructors, so the role should be instructor.
            // TODO: properly handle student users?
            ("Your Courses", Role::Instructor),
        ];

        let courses = heading_account_types
            .iter()
            .flat_map(|(heading, role)| {
                course_list_headings
                    .get(*heading)
                    .into_iter()
                    .flat_map(|list| Self::parse_courses(*list, *role))
            })
            .collect();

        Ok(courses)
    }

    fn parse_courses(list: ElementRef, user_role: Role) -> impl Iterator<Item = Course> + '_ {
        list.select(&COURSE)
            .filter_map(move |course_box| Self::parse_course(course_box, user_role))
    }

    fn parse_course(course_box: ElementRef, user_role: Role) -> Option<Course> {
        let id = CourseId::new(id_from_link(course_box).ok()?);
        let short_name = text(course_box.select(&COURSE_SHORT_NAME).next()?);
        let name = text(course_box.select(&COURSE_NAME).next()?);
        Some(Course::new(id, short_name, name, user_role))
    }

    #[tracing::instrument(skip(self), ret, err)]
    pub async fn get_assignments(&self, course: &Course) -> Result<Vec<Assignment>> {
        let assignments_page = self
            .get_gs_html(&gs_course_path(course, ASSIGNMENTS_COURSE_PATH))
            .await?;

        let assignments_table = assignments_page
            .select(&ASSIGNMENTS_TABLE)
            .exactly_one()
            .map_err(|err_it| {
                anyhow!(
                    "not exactly one assignments table: found {}",
                    err_it.count()
                )
            })?;
        let assignments_table_data = assignments_table
            .value()
            .attr("data-react-props")
            .context("missing assignments table data")?;

        let assignments_table_props: AssignmentsTableProps =
            serde_json::from_str(assignments_table_data)?;

        Ok(assignments_table_props.assignments)
    }

    pub async fn get_regrades(
        &self,
        course: &Course,
        assignment: &Assignment,
    ) -> Result<Vec<Regrade>> {
        let regrade_page = self
            .get_gs_html(&gs_assignment_path(
                course,
                assignment,
                REGRADES_ASSIGNMENT_PATH,
            ))
            .await?;

        let regrades = regrade_page
            .select(&REGRADE_ROW)
            .map(Self::parse_regrade)
            .try_collect()?;

        Ok(regrades)
    }

    fn parse_regrade(row: ElementRef) -> Result<Regrade> {
        let mut entries = row.select(&TD);

        let student_entry = entries.next().context("missing student entry")?;
        let student_name = StudentName::new(text(student_entry));

        let _sections_entry = entries.next().context("missing sections entry")?;

        let question_entry = entries.next().context("missing question entry")?;
        let question_entry_text = text(question_entry);
        let (question_number_text, question_title_text) = question_entry_text
            .split_once(':')
            .with_context(|| format!("couldn't split question entry \"{question_entry_text}\""))?;
        let question_number = question_number_text
            .parse()
            .context("could not parse question number")?;
        let question_title = QuestionTitle::new(question_title_text.to_owned());

        let grader_entry = entries.next().context("missing grader entry")?;
        let grader_name = GraderName::new(text(grader_entry));

        let completed_entry = entries.next().context("missing completed entry")?;
        let completed = completed_entry.has_children();

        let link_entry = entries.next().context("missing link entry")?;
        let url_text = link_entry
            .select(&A)
            .next()
            .context("missing link element")?
            .value()
            .attr("href")
            .context("missing href attribute")?;
        let url = Url::parse(BASE_URL)?.join(url_text)?;

        Ok(Regrade::new(
            student_name,
            question_number,
            question_title,
            grader_name,
            url,
            completed,
        ))
    }

    pub async fn get_submissions_metadata(
        &self,
        course: &Course,
        assignment: &Assignment,
    ) -> Result<SubmissionsManagerProps> {
        let manage_submissions_page = self
            .get_gs_html(&gs_manage_submissions_path(course, assignment))
            .await
            .context("cannot get \"manage submissions\" page")?;

        let submissions_manager = manage_submissions_page
            .select(&SUBMISSIONS_MANAGER)
            .exactly_one()
            .map_err(|err_it| {
                anyhow!(
                    "not exactly one submissions manager: found {}",
                    err_it.count()
                )
            })?;
        let submissions_manager_data = submissions_manager
            .value()
            .attr("data-react-props")
            .context("missing submission manager data")?;

        let submissions_manager_props: SubmissionsManagerProps =
            serde_json::from_str(submissions_manager_data)
                .context("could not parse submissions manager data")?;

        if assignment.id() != submissions_manager_props.assignment_id() {
            bail!(
                "assignment id is `{}`, but the submissions manager is for assignment id `{}`",
                assignment.id(),
                submissions_manager_props.assignment_id()
            );
        }

        Ok(submissions_manager_props)
    }

    // TODO: reimplement checking/getting path
    pub async fn export_submissions(
        &self,
        course: &Course,
        assignment: &Assignment,
    ) -> Result<impl SubmissionExport> {
        let path = self.exported_submissions_path(course, assignment).await?;
        let request = self
            .gs(Method::GET, &path)
            .await
            .timeout(Duration::from_secs(60 * 60));
        let response = self.send(request).await?;
        let export = submissions_export_from_response(response);
        Ok(export)
    }

    pub async fn export_submissions_old(
        &self,
        course: &Course,
        assignment: &Assignment,
    ) -> Result<impl Stream<Item = Result<(SubmissionId, SubmissionPdf)>>> {
        let path = self.exported_submissions_path(course, assignment).await?;
        let request = self
            .gs(Method::GET, &path)
            .await
            .timeout(Duration::from_secs(60 * 60));
        let response = self.send(request).await?;

        let zip = download_submission_export(response).await?;
        let files = read_zip(zip);
        let submissions = files_as_submissions(files);

        Ok(submissions)
    }

    /// Get the path to the exported submissions
    async fn exported_submissions_path(
        &self,
        course: &Course,
        assignment: &Assignment,
    ) -> Result<String> {
        // `Html` is non-`Send`, and Rust complains if it's not dropped before an await point. The
        // function should be correct without this block, but the compiler can't tell.
        let result = {
            let review_grades_page = self
                .get_gs_html(&gs_review_grades_path(course, assignment))
                .await
                .context("getting review grades")?;

            let export_download_href = Self::export_download_href(&review_grades_page)?;
            debug!(?export_download_href);

            match export_download_href {
                Some(path) => {
                    info!("submissions were already exported");
                    Either::Left(path.to_owned())
                }
                None => {
                    info!("must request export");

                    let csrf_token = Self::csrf_token_from_meta(&review_grades_page)?;
                    debug!(csrf_token);

                    Either::Right(csrf_token.to_owned())
                }
            }
        };

        match result {
            Either::Left(path) => Ok(path),
            Either::Right(csrf_token) => {
                self.request_export_submissions(course, assignment, &csrf_token)
                    .await
            }
        }
    }

    fn export_download_href(review_grades_page: &Html) -> Result<Option<&str>> {
        let export_download_a = review_grades_page
            .select(&BULK_EXPORT_A)
            .exactly_one()
            .map_err(|err_it| anyhow!("not exactly one export link: found {}", err_it.count()))?;

        let href = export_download_a.value().attr("href").and_then(|href| {
            if href != "javascript:void(0);" {
                Some(href)
            } else {
                None
            }
        });

        Ok(href)
    }

    fn csrf_token_from_meta(review_grades_page: &Html) -> Result<&str> {
        let csrf_token_meta = review_grades_page
            .select(&CSRF_TOKEN_META)
            .exactly_one()
            .map_err(|err_it| {
                anyhow!("not exactly one CSRF token meta: found {}", err_it.count())
            })?;

        let csrf_token = csrf_token_meta
            .value()
            .attr("content")
            .ok_or_else(|| anyhow!("could not get CSRF token from meta: {csrf_token_meta:?}"))?;

        Ok(csrf_token)
    }

    /// Requests and waits for Gradescope to export submissions, returning the path to the export if
    /// successful. This can take substantial time (i.e. easily >10 minutes).
    async fn request_export_submissions(
        &self,
        course: &Course,
        assignment: &Assignment,
        csrf_token: &str,
    ) -> Result<String> {
        let path = gs_assignment_path(course, assignment, "/export");
        let request = self.gs_ajax(Method::POST, &path, csrf_token).await;
        let response = self.send(request).await?;

        let status_path = response
            .json::<ExportSubmissionsResponse>()
            .await?
            .status_path(course);

        self.await_export_completion(course, &status_path, csrf_token)
            .await
    }

    #[tracing::instrument(skip(self, csrf_token), err, ret)]
    async fn await_export_completion(
        &self,
        course: &Course,
        status_path: &str,
        csrf_token: &str,
    ) -> Result<String> {
        loop {
            let request = self.gs_ajax(Method::GET, status_path, csrf_token).await;
            let response = self.send(request).await?;

            let status = response.json::<ExportSubmissionsStatus>().await?;

            if status.completed() {
                info!("export complete!");
                break Ok(status.download_path(course));
            }

            info!(
                progress = status.progress(),
                status = status.status(),
                "still waiting on export..."
            );
            sleep(Duration::from_secs(10)).await;
        }
    }
}

#[derive(Debug)]
pub struct Init;
#[derive(Debug)]
pub struct Auth;

pub trait ClientState {}
impl ClientState for Init {}
impl ClientState for Auth {}

#[derive(Deserialize)]
struct ExportSubmissionsResponse {
    generated_file_id: u64,
}

impl ExportSubmissionsResponse {
    pub fn status_path(&self, course: &Course) -> String {
        gs_course_path(
            course,
            &format!("/generated_files/{}.json", self.generated_file_id),
        )
    }
}

#[derive(Debug, Deserialize)]
struct ExportSubmissionsStatus {
    id: u64,
    progress: f32,
    status: String,
}

impl ExportSubmissionsStatus {
    pub fn completed(&self) -> bool {
        match self.status.as_str() {
            "unprocessed" | "processing" => false,
            "completed" => true,
            status => {
                warn!(%status, complete_status = ?self, "unexpected export status");
                false
            }
        }
    }

    pub fn download_path(&self, course: &Course) -> String {
        gs_course_path(course, &format!("/generated_files/{}.zip", self.id))
    }

    pub fn progress(&self) -> f32 {
        self.progress
    }

    pub fn status(&self) -> &str {
        &self.status
    }
}
