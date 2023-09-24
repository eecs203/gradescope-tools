use crate::assignment::Assignment;

#[derive(Debug, Clone)]
pub struct AssignmentSelector {
    selector: String,
}

impl AssignmentSelector {
    pub fn new(selector: String) -> Self {
        Self { selector }
    }

    pub fn select_from<'a>(&self, assignments: &'a [Assignment]) -> Option<&'a Assignment> {
        self.select_as_id(assignments)
            .or_else(|| self.select_as_name(assignments))
    }

    fn select_as_id<'a>(&self, assignments: &'a [Assignment]) -> Option<&'a Assignment> {
        assignments
            .iter()
            .find(|assignment| assignment.id().as_str() == self.selector)
    }

    fn select_as_name<'a>(&self, assignments: &'a [Assignment]) -> Option<&'a Assignment> {
        assignments
            .iter()
            .find(|assignment| assignment.name().as_str() == self.selector)
    }
}
