use std::env;

use anyhow::Result;
use dotenvy::dotenv;
use gradescope_api::client::{Auth, Client};
use gradescope_api::course::Course;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, registry};

pub async fn init_from_env() -> Result<InitFromEnv> {
    dotenv().unwrap();

    let course_name = course_name_from_env();

    let gradescope = Client::from_env().await?.login().await?;

    let (instructor_courses, _student_courses) = gradescope.get_courses().await?;
    let course = Course::find_by_short_name(&course_name, instructor_courses)?;

    Ok(InitFromEnv {
        course,
        gradescope,
        course_name,
    })
}

pub struct InitFromEnv {
    pub course: Course,
    pub gradescope: Client<Auth>,
    pub course_name: String,
}

pub fn db_url_from_env() -> String {
    env::var("DATABASE_URL").unwrap()
}

fn course_name_from_env() -> String {
    env::var("COURSE_NAME").unwrap()
}

pub fn init_tracing() {
    registry()
        .with(fmt::layer().with_filter(LevelFilter::INFO))
        .init();
}
