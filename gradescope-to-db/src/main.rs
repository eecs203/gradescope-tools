use anyhow::Result;
use app_utils::{db_url_from_env, init_from_env, InitFromEnv};
use gradescope_api::assignment::Assignment;
use gradescope_api::client::{Auth, Client as GsConnection};
use gradescope_api::course::Course;
use gradescope_api::regrade::Regrade;
use sqlx::SqlitePool;

#[tokio::main]
async fn main() -> Result<()> {
    let InitFromEnv {
        course,
        gradescope,
        course_name: _,
    } = init_from_env().await?;

    let db_pool = SqlitePool::connect(&db_url_from_env()).await?;

    add_course(&db_pool, &gradescope, &course).await?;

    Ok(())
}

async fn add_course(
    db_pool: &SqlitePool,
    gradescope: &GsConnection<Auth>,
    course: &Course,
) -> Result<()> {
    insert_course(db_pool, course).await?;

    let assignments = gradescope.get_assignments(course).await?;
    for assignment in &assignments {
        add_assignment(db_pool, gradescope, course, assignment).await?;
    }

    Ok(())
}
async fn add_assignment(
    db_pool: &SqlitePool,
    gradescope: &GsConnection<Auth>,
    course: &Course,
    assignment: &Assignment,
) -> Result<()> {
    insert_assignment(db_pool, course, assignment).await?;

    let regrades = gradescope.get_regrades(course, assignment).await?;
    for regrade in &regrades {
        insert_regrade(db_pool, assignment, regrade).await?;
    }

    Ok(())
}

async fn insert_course(db_pool: &SqlitePool, course: &Course) -> Result<()> {
    let mut db = db_pool.acquire().await?;
    let (id, short_name, name) = (course.id().as_str(), course.short_name(), course.name());

    sqlx::query!(
        "
        INSERT OR IGNORE INTO instructor_course (id, short_name, name)
        VALUES (?, ?, ?);
        ",
        id,
        short_name,
        name
    )
    .execute(&mut *db)
    .await?;

    Ok(())
}

async fn insert_assignment(
    db_pool: &SqlitePool,
    course: &Course,
    assignment: &Assignment,
) -> Result<()> {
    let mut db = db_pool.acquire().await?;
    let (id, course_id, name, points) = (
        assignment.id().as_str(),
        course.id().as_str(),
        assignment.name().as_str(),
        assignment.points().as_f32(),
    );

    sqlx::query!(
        "
        INSERT OR IGNORE INTO assignment (id, course_id, name, points)
        VALUES (?, ?, ?, ?);
        ",
        id,
        course_id,
        name,
        points
    )
    .execute(&mut *db)
    .await?;

    Ok(())
}

async fn insert_regrade(
    db_pool: &SqlitePool,
    assignment: &Assignment,
    regrade: &Regrade,
) -> Result<()> {
    let mut db = db_pool.acquire().await?;
    let (assignment_id, student_name, question_number, question_title, grader_name, completed) = (
        assignment.id().as_str(),
        regrade.student_name().as_str(),
        regrade.question_number().to_string(),
        regrade.question_title().as_str(),
        regrade.grader_name().as_str(),
        i8::from(regrade.completed()),
    );

    sqlx::query!(
        "
        INSERT OR IGNORE INTO regrade (assignment_id, student_name, question_number, question_title, grader_name, completed)
        VALUES (?, ?, ?, ?, ?, ?);
        ",
        assignment_id, student_name, question_number, question_title, grader_name, completed
    ).execute(&mut *db).await?;

    Ok(())
}
