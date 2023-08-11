use url::Url;

use crate::types::{GraderName, QuestionNumber, QuestionTitle, StudentName};

#[derive(Debug, Clone)]
pub struct Regrade {
    student_name: StudentName,
    question_number: QuestionNumber,
    question_title: QuestionTitle,
    grader_name: GraderName,
    url: Url,
    completed: bool,
}

impl Regrade {
    pub fn new(
        student_name: StudentName,
        question_number: QuestionNumber,
        question_title: QuestionTitle,
        grader_name: GraderName,
        url: Url,
        completed: bool,
    ) -> Self {
        Self {
            student_name,
            question_number,
            question_title,
            grader_name,
            url,
            completed,
        }
    }

    pub fn student_name(&self) -> &StudentName {
        &self.student_name
    }

    pub fn question_number(&self) -> &QuestionNumber {
        &self.question_number
    }

    pub fn question_title(&self) -> &QuestionTitle {
        &self.question_title
    }

    pub fn grader_name(&self) -> &GraderName {
        &self.grader_name
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn completed(&self) -> bool {
        self.completed
    }
}
