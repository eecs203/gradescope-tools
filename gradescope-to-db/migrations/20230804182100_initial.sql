-- Add migration script here
CREATE TABLE instructor_course(
    id TEXT PRIMARY KEY NOT NULL, -- Gradescope ID
    short_name TEXT NOT NULL,
    name TEXT NOT NULL
);

CREATE TABLE assignment(
    id TEXT PRIMARY KEY NOT NULL,
    course_id TEXT NOT NULL,
    name TEXT NOT NULL,
    points REAL NOT NULL,
    FOREIGN KEY (course_id) REFERENCES instructor_course(id)
);

CREATE TABLE regrade(
    assignment_id TEXT NOT NULL,
    student_name TEXT NOT NULL,
    question_number TEXT NOT NULL,
    question_title TEXT NOT NULL,
    grader_name TEXT NOT NULL,
    FOREIGN KEY (assignment_id) REFERENCES assignment(id)
);
