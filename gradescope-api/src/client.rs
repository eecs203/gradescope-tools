use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use itertools::{Either, Itertools};
use reqwest::{Method, Response};
use scraper::{CaseSensitivity, Element, ElementRef, Html};
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tower::{Service, ServiceExt};
use tracing::{debug, info, warn};
use url::Url;

use crate::assignment::{Assignment, AssignmentsTableProps};
use crate::course::{Course, CourseId, Role};
use crate::creds::Creds;
use crate::question::{AssignmentOutline, Outline, QuestionTitle};
use crate::regrade::Regrade;
use crate::selectors;
use crate::services::gs_service::{self, GsRequest, GsService, HtmlRequest};
use crate::submission::SubmissionsManagerProps;
use crate::types::{GraderName, StudentName};
use crate::util::*;

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
    ASSIGNMENT_OUTLINE = "[data-react-class=AssignmentOutline]",
    CSRF_TOKEN_META = "meta[name='csrf-token']",
}

#[derive(Debug)]
pub struct Client<Service> {
    service: Mutex<Service>,
}

pub async fn client(creds: Creds) -> Result<Client<impl GsService>> {
    Ok(Client {
        service: Mutex::new(gs_service::service(creds).await?),
    })
}

pub async fn client_from_env() -> Result<Client<impl GsService>> {
    let creds = Creds::from_env()?;
    client(creds).await
}

impl<Service: GsService> Client<Service> {
    async fn request(&self, request: GsRequest) -> Result<Response> {
        self.service.lock().await.ready().await?.call(request).await
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn html_request(&self, request: impl Into<HtmlRequest> + Debug) -> Result<Html> {
        self.service
            .lock()
            .await
            .as_html_service()
            .ready()
            .await?
            .call(request.into())
            .await
    }

    pub async fn get_courses(&self) -> Result<Vec<Course>> {
        let account_page = self.html_request(ACCOUNT_PATH).await?;
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
            .html_request(gs_course_path(course, ASSIGNMENTS_COURSE_PATH))
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
            .html_request(gs_assignment_path(
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

    pub async fn get_outline(&self, course: &Course, assignment: &Assignment) -> Result<Outline> {
        let outline_page = self
            .html_request(gs_assignment_path(
                course,
                assignment,
                OUTLINE_ASSIGNMENT_PATH,
            ))
            .await?;

        let outline_elt = outline_page
            .select(&ASSIGNMENT_OUTLINE)
            .exactly_one()
            .map_err(|err_it| {
                anyhow!(
                    "not exactly one assignment outline element: found {}",
                    err_it.count()
                )
            })?;
        let outline_data = outline_elt
            .value()
            .attr("data-react-props")
            .context("missing assignment outline data")?;

        let assignment_outline: AssignmentOutline = serde_json::from_str(outline_data)?;

        Ok(assignment_outline.outline)
    }

    pub async fn get_submissions_metadata(
        &self,
        course: &Course,
        assignment: &Assignment,
    ) -> Result<SubmissionsManagerProps> {
        let manage_submissions_page = self
            .html_request(gs_manage_submissions_path(course, assignment))
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
    ) -> Result<Response> {
        let path = self.exported_submissions_path(course, assignment).await?;
        let response = self
            .request(
                GsRequest::new_direct(Method::GET, path).with_timeout(Duration::from_secs(60 * 60)),
            )
            .await?;
        Ok(response)
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
                .html_request(gs_review_grades_path(course, assignment))
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
                self.request_export_submissions(course, assignment, csrf_token)
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
        csrf_token: String,
    ) -> Result<String> {
        let path = gs_assignment_path(course, assignment, "/export");
        let response = self
            .request(GsRequest::new_ajax(Method::POST, path, csrf_token.clone()))
            .await?;

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
        csrf_token: String,
    ) -> Result<String> {
        loop {
            let response = self
                .request(GsRequest::new_ajax(
                    Method::GET,
                    status_path.to_owned(),
                    csrf_token.clone(),
                ))
                .await?;

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
