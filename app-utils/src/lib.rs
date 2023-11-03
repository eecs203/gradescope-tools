use std::env;

use anyhow::{Context, Result};
use dotenvy::dotenv;
use gradescope_api::client::{Auth, Client};
use gradescope_api::course::Course;
use gradescope_api::course_selector::{self, CourseSelector};
use tracing::info;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, registry, EnvFilter};

pub async fn init_from_env() -> Result<InitFromEnv> {
    dotenv().unwrap();

    let course_selector = course_selector_from_env();

    let gradescope = Client::from_env().await?.login().await?;

    let courses = gradescope.get_courses().await?;
    let course = course_selector
        .select_from(&courses)
        .with_context(|| format!("could not find course with selector {course_selector:?}"))?
        .clone();

    Ok(InitFromEnv { course, gradescope })
}

pub struct InitFromEnv {
    pub course: Course,
    pub gradescope: Client<Auth>,
}

pub fn db_url_from_env() -> String {
    env::var("DATABASE_URL").unwrap()
}

fn course_selector_from_env() -> CourseSelector {
    CourseSelector::new(env::var("COURSE").unwrap())
}

pub fn init_tracing() {
    registry()
        .with(fmt::layer().event_format(format().pretty()))
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env()
                .unwrap(),
        )
        .init();
}
