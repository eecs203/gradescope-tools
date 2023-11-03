use crate::course::Course;

#[derive(Debug, Clone)]
pub struct CourseSelector {
    selector: String,
}

impl CourseSelector {
    pub fn new(selector: String) -> Self {
        Self { selector }
    }

    pub fn select_from<'a>(&self, courses: &'a [Course]) -> Option<&'a Course> {
        self.select_as_id(courses)
            .or_else(|| self.select_as_short_name(courses))
            .or_else(|| self.select_as_name(courses))
    }

    fn select_as_id<'a>(&self, courses: &'a [Course]) -> Option<&'a Course> {
        courses
            .iter()
            .find(|course| course.id().as_str() == self.selector)
    }

    fn select_as_short_name<'a>(&self, courses: &'a [Course]) -> Option<&'a Course> {
        courses
            .iter()
            .find(|course| course.short_name() == self.selector)
    }

    fn select_as_name<'a>(&self, courses: &'a [Course]) -> Option<&'a Course> {
        courses.iter().find(|course| course.name() == self.selector)
    }
}
