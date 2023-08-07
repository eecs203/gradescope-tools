use std::collections::HashMap;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use itertools::Itertools;
use lazy_static::lazy_static;
use reqwest::redirect::Policy;
use reqwest::{Client as HttpClient, Response};
use scraper::{ElementRef, Html};
use tokio::time::sleep;
use url::Url;

use crate::assignment::{Assignment, AssignmentName};
use crate::course::{Course, Role};
use crate::creds::Creds;
use crate::regrade::Regrade;
use crate::types::{GraderName, Points, QuestionNumber, QuestionTitle, StudentName};
use crate::util::*;

macro_rules! selectors {
    ($name:ident = $x:expr) => {
        lazy_static! { static ref $name: scraper::Selector = scraper::Selector::parse($x).unwrap(); }
    };

    ($name:ident = $x:expr, $($names:ident = $xs:expr),+) => {
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
    REGRADE_ROW = "table.js-regradeRequestsTable > tbody > tr"
}

pub struct Client<State: ClientState> {
    client: HttpClient,
    creds: Creds,
    _state: State,
}

impl<State: ClientState> Client<State> {
    async fn get_gs_html(&self, path: &str) -> Result<Html> {
        let text = self.get_gs_response(path).await?.text().await?;
        Ok(Html::parse_document(&text))
    }

    async fn get_gs_response(&self, path: &str) -> Result<Response> {
        sleep(Duration::from_millis(1000)).await;

        let url = gs_url(path);
        println!("sending request to {url}");

        self.client
            .get(url)
            .send()
            .await
            .context("Gradescope request failed")?
            .error_for_status()
            .context("Gradescope responded with an error")
    }
}

impl Client<Init> {
    pub async fn from_env() -> Result<Self> {
        let creds = Creds::from_env()?;
        Client::new(creds).await
    }

    pub async fn new(creds: Creds) -> Result<Self> {
        let client = HttpClient::builder()
            .cookie_store(true)
            .redirect(Policy::none())
            .build()?;

        // init cookies
        client.get(BASE_URL).send().await?;

        Ok(Self {
            client,
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

        let response = self
            .client
            .post(gs_url(LOGIN_PATH))
            .form(&login_data)
            .send()
            .await?;

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
    pub async fn get_courses(&self) -> Result<(Vec<Course>, Vec<Course>)> {
        let account_page = self.get_gs_html(ACCOUNT_PATH).await?;
        let course_list_headings = account_page
            .select(&COURSE_LIST_HEADING)
            .filter_map(|el| {
                el.next_sibling()
                    .and_then(ElementRef::wrap)
                    .map(|list| (text(el), list))
            })
            .collect::<HashMap<_, _>>();

        let instructor_course_list = course_list_headings.get("Instructor Courses");
        let student_course_list = course_list_headings.get("Student Courses");

        let instructor_courses = instructor_course_list
            .into_iter()
            .flat_map(|list| Self::parse_courses(*list, Role::Instructor))
            .collect();
        let student_courses = student_course_list
            .into_iter()
            .flat_map(|list| Self::parse_courses(*list, Role::Student))
            .collect();

        Ok((instructor_courses, student_courses))
    }

    fn parse_courses(list: ElementRef, user_role: Role) -> impl Iterator<Item = Course> + '_ {
        list.select(&COURSE)
            .filter_map(move |course_box| Self::parse_course(course_box, user_role))
    }

    fn parse_course(course_box: ElementRef, user_role: Role) -> Option<Course> {
        let id = id_from_link(course_box)?;
        let short_name = text(course_box.select(&COURSE_SHORT_NAME).next()?);
        let name = text(course_box.select(&COURSE_NAME).next()?);
        Some(Course::new(id, short_name, name, user_role))
    }

    pub async fn get_assignments(&self, course: &Course) -> Result<Vec<Assignment>> {
        let assignments_page = self
            .get_gs_html(&gs_course_path(course, ASSIGNMENTS_COURSE_PATH))
            .await?;

        let assignments = assignments_page
            .select(&ASSIGNMENT_ROW)
            .filter_map(Self::parse_assignment)
            .collect();

        Ok(assignments)
    }

    fn parse_assignment(row: ElementRef) -> Option<Assignment> {
        let mut entries = row.select(&TD);

        let name_entry = entries.next()?;
        let id = id_from_link(name_entry.select(&A).next()?)?;
        let name = AssignmentName::new(text(name_entry));

        let points_entry = entries.next()?;
        let points_value = text(points_entry).parse().ok()?;
        let points = Points::new(points_value).ok()?;

        Some(Assignment::new(id, name, points))
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
        let question_number = QuestionNumber::new(question_number_text.to_owned());
        let question_title = QuestionTitle::new(question_title_text.to_owned());

        let grader_entry = entries.next().context("missing grader entry")?;
        let grader_name = GraderName::new(text(grader_entry));

        let _completed_entry = entries.next().context("missing completed entry")?;

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
        ))
    }
}

pub struct Init;
pub struct Auth;

pub trait ClientState {}
impl ClientState for Init {}
impl ClientState for Auth {}
