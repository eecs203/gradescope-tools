use scraper::ElementRef;

use crate::assignment::Assignment;
use crate::course::Course;

pub const BASE_URL: &str = "https://www.gradescope.com";
pub const LOGIN_PATH: &str = "/login";
pub const ACCOUNT_PATH: &str = "/account";
pub const ASSIGNMENTS_COURSE_PATH: &str = "/assignments";
pub const REGRADES_ASSIGNMENT_PATH: &str = "/regrade_requests";

pub fn gs_url(path: &str) -> String {
    format!("{BASE_URL}{path}")
}

pub fn gs_course_path(course: &Course, path: &str) -> String {
    format!("/courses/{}{path}", course.id())
}

pub fn gs_assignment_path(course: &Course, assignment: &Assignment, path: &str) -> String {
    gs_course_path(course, &format!("/assignments/{}{path}", assignment.id()))
}

pub fn text(el: ElementRef) -> String {
    el.text().flat_map(|text| text.chars()).collect()
}

pub fn id_from_link(link: ElementRef) -> Option<String> {
    link.value()
        .attr("href")?
        .split('/')
        .last()
        .map(ToOwned::to_owned)
}
